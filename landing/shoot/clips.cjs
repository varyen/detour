/* Съёмка коротких роликов панели для лендинга (десктоп).
 *
 *   node clips.cjs                 — все ролики
 *   node clips.cjs paste-link      — только перечисленные
 *
 * Панель — из dev-сервера vite, API — через обезличивающий прокси (как в
 * shots.cjs). Пишет Playwright (recordVideo, webm), дальше ffmpeg переводит в
 * постоянный fps, режет точную длину и кладёт mp4 + webm + постер в
 * landing/video/. Сырые webm остаются в landing/raw-video/ (gitignored).
 *
 * Курсора в записи Playwright нет — рисуем свой поверх страницы и двигаем его
 * по событиям mousemove/mousedown, которые Playwright и так рассылает.
 *
 * Ролики только показывают, ничего не сохраняют: ссылка разбирается кнопкой
 * «Разобрать» без сохранения профиля, раскладка главной живёт в localStorage
 * свежего контекста и дальше него не уходит.
 */
const { chromium } = require('playwright-core');
const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const CHROME = process.env.CHROME_BIN || findChrome();
const BASE = process.env.BASE || 'http://localhost:5199/detour/';
const RAW = path.join(__dirname, '..', 'raw-video');
const OUT = path.join(__dirname, '..', 'video');
const W = 1280, H = 800;

function findChrome() {
  const roots = [
    process.env.PLAYWRIGHT_BROWSERS_PATH,
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'ms-playwright'),
    process.env.HOME && path.join(process.env.HOME, '.cache', 'ms-playwright'),
  ].filter(Boolean);
  for (const root of roots) {
    if (!fs.existsSync(root)) continue;
    const dirs = fs.readdirSync(root).filter((d) => d.startsWith('chromium-')).sort().reverse();
    for (const d of dirs) {
      for (const rel of ['chrome-win64/chrome.exe', 'chrome-win/chrome.exe', 'chrome-linux64/chrome', 'chrome-linux/chrome']) {
        const p = path.join(root, d, rel);
        if (fs.existsSync(p)) return p;
      }
    }
  }
  throw new Error('не нашёл chromium: задайте CHROME_BIN');
}

const only = process.argv.slice(2);
const want = (n) => !only.length || only.includes(n);

/* Подмешать флаги (см. shots.cjs) и нарисовать курсор. */
const CURSOR = `
(() => {
  const c = document.createElement('div');
  c.id = '__cur';
  c.innerHTML = '<svg width="22" height="30" viewBox="0 0 22 30"><path d="M2 2 L2 24 L8 18 L12 28 L16 26 L12 17 L20 17 Z" fill="#fff" stroke="#111" stroke-width="1.6" stroke-linejoin="round"/></svg>';
  Object.assign(c.style, { position: 'fixed', left: '0', top: '0', zIndex: 2147483647, pointerEvents: 'none',
    transform: 'translate(-40px,-40px)', transition: 'scale .12s', filter: 'drop-shadow(0 1px 2px rgba(0,0,0,.6))' });
  document.documentElement.appendChild(c);
  const at = (e) => { c.style.transform = 'translate(' + e.clientX + 'px,' + e.clientY + 'px)'; };
  addEventListener('mousemove', at, true);
  addEventListener('mousedown', (e) => { at(e); c.style.scale = '0.82'; }, true);
  addEventListener('mouseup', (e) => { at(e); c.style.scale = '1'; }, true);
})();`;

async function prep(page) {
  await page.evaluate(async () => {
    localStorage.setItem('detour-theme', 'dark');
    document.documentElement.dataset.theme = 'dark';
    let src = '';
    try {
      const css = await (await fetch('https://fonts.googleapis.com/css2?family=Noto+Color+Emoji&display=swap')).text();
      src = (css.match(/url\(([^)]+\.woff2)\)/) || [])[1] || '';
    } catch (e) {}
    const orig = getComputedStyle(document.body).fontFamily;
    const st = document.createElement('style');
    st.textContent = (src ? `@font-face{font-family:"FlagEmoji";src:url(${src}) format("woff2");unicode-range:U+1F1E6-1F1FF;font-display:block}` : '')
      + `body{font-family:"FlagEmoji", ${orig} !important}`;
    document.head.appendChild(st);
    if (document.fonts && document.fonts.load) { try { await document.fonts.load('16px FlagEmoji', '\u{1F1F3}\u{1F1F1}'); } catch (e) {} }
  });
  await page.evaluate(CURSOR);
}

/* Плавно подвести курсор к центру элемента. */
async function glide(page, target, opts = {}) {
  const box = await target.boundingBox();
  if (!box) throw new Error('нет boundingBox у цели');
  const x = box.x + box.width * (opts.fx ?? 0.5), y = box.y + box.height * (opts.fy ?? 0.5);
  await page.mouse.move(x, y, { steps: opts.steps ?? 28 });
  await page.waitForTimeout(opts.pause ?? 350);
  return { x, y };
}

/* Один ролик: свой контекст (так у каждого свой webm), отметка времени
   начала действия — с неё ffmpeg и режет. */
async function clip(browser, name, len, body) {
  if (!want(name)) return;
  const ctx = await browser.newContext({
    viewport: { width: W, height: H }, deviceScaleFactor: 1,
    serviceWorkers: 'block', locale: 'ru-RU', timezoneId: 'Europe/Moscow',
    recordVideo: { dir: RAW, size: { width: W, height: H } },
  });
  const t0 = Date.now();
  const page = await ctx.newPage();
  await page.mouse.move(W - 60, H - 60);
  let start = 0;
  const mark = () => { start = (Date.now() - t0) / 1000; };
  try {
    await body(page, mark);
    if (!start) throw new Error(`${name}: не вызван mark()`);
    /* добрать хвост, чтобы было из чего резать len секунд */
    const need = start + len + 0.6 - (Date.now() - t0) / 1000;
    if (need > 0) await page.waitForTimeout(need * 1000);
  } finally {
    const v = page.video();
    await ctx.close();
    const src = await v.path();
    const raw = path.join(RAW, name + '.webm');
    fs.renameSync(src, raw);
    encode(name, raw, start, len);
  }
}

function encode(name, raw, ss, len) {
  fs.mkdirSync(OUT, { recursive: true });
  const common = ['-y', '-hide_banner', '-loglevel', 'error', '-ss', ss.toFixed(2), '-i', raw, '-t', String(len),
    '-r', '30', '-fps_mode', 'cfr', '-an'];
  execFileSync('ffmpeg', [...common, '-c:v', 'libx264', '-preset', 'slow', '-crf', '24', '-pix_fmt', 'yuv420p',
    '-movflags', '+faststart', path.join(OUT, name + '.mp4')], { stdio: 'inherit' });
  execFileSync('ffmpeg', [...common, '-c:v', 'libvpx-vp9', '-b:v', '0', '-crf', '34', '-row-mt', '1',
    path.join(OUT, name + '.webm')], { stdio: 'inherit' });
  execFileSync('ffmpeg', ['-y', '-hide_banner', '-loglevel', 'error', '-ss', (ss + 0.3).toFixed(2), '-i', raw,
    '-frames:v', '1', '-q:v', '4', path.join(OUT, name + '.jpg')], { stdio: 'inherit' });
  const kb = (f) => Math.round(fs.statSync(path.join(OUT, f)).size / 1024);
  console.log(`${name}: mp4 ${kb(name + '.mp4')} КБ · webm ${kb(name + '.webm')} КБ · poster ${kb(name + '.jpg')} КБ (${len} с с ${ss.toFixed(1)} с)`);
}

async function go(page, hash, wait = 4000) {
  await page.goto(BASE + hash, { waitUntil: 'domcontentloaded' });
  await prep(page);
  await page.waitForTimeout(wait);
}

const LINK = 'vless://00000000-0000-4000-8000-000000000000@srv-07.example-vpn.net:443'
  + '?type=tcp&security=reality&sni=panel.example.com&fp=chrome&flow=xtls-rprx-vision'
  + '&pbk=EXAMPLEPUBLICKEY0000000000000000000000000000&sid=0a1b2c#%F0%9F%87%B3%F0%9F%87%B1%20%D0%90%D0%BC%D1%81%D1%82%D0%B5%D1%80%D0%B4%D0%B0%D0%BC';

(async () => {
  fs.mkdirSync(RAW, { recursive: true });
  const b = await chromium.launch({ executablePath: CHROME });

  /* 1. Вставили ссылку — форма заполнилась сама. */
  await clip(b, 'paste-link', 8, async (page, mark) => {
    await go(page, '#/profiles', 4500);
    const add = page.getByRole('button', { name: 'Добавить профиль' }).first();
    await glide(page, add, { steps: 20 });
    mark();
    await page.waitForTimeout(500);
    await add.click({ force: true });
    await page.waitForTimeout(900);
    const inp = page.getByPlaceholder(/Вставьте ссылку/).first();
    await glide(page, inp, { fx: 0.3 });
    await inp.click({ force: true });
    await page.waitForTimeout(350);
    await page.keyboard.insertText(LINK);
    await page.waitForTimeout(900);
    const parse = page.getByRole('button', { name: 'Разобрать' }).first();
    await glide(page, parse);
    await parse.click({ force: true });
    await page.waitForTimeout(600);
    /* показать заполненные поля: курсор отходит, страница слегка листается */
    await page.mouse.move(W * 0.62, H * 0.75, { steps: 30 });
    await page.mouse.wheel(0, 260);
  });

  /* 2. «Сменить VPN» с обзора — список профилей уже по скорости. */
  await clip(b, 'switch-vpn', 7, async (page, mark) => {
    await go(page, '#/', 4500);
    const btn = page.getByRole('button', { name: 'Сменить VPN' }).first();
    await glide(page, btn, { steps: 26 });
    mark();
    await page.waitForTimeout(500);
    await btn.click({ force: true });
    await page.waitForTimeout(2600);
    const rows = page.locator('.row .nm');
    await glide(page, rows.nth(0), { fx: 0.2, steps: 24 });
    await glide(page, rows.nth(2), { fx: 0.2, steps: 24, pause: 500 });
  });

  /* 3. Карточки главной — перетаскиванием. */
  await clip(b, 'dash-drag', 8, async (page, mark) => {
    await go(page, '#/', 4500);
    const cfg = page.getByRole('button', { name: /Настроить главную/ }).first();
    await glide(page, cfg, { steps: 22 });
    mark();
    await page.waitForTimeout(500);
    await cfg.click({ force: true });
    await page.waitForTimeout(1100);
    const tiles = page.locator('[data-tile]');
    const grab = tiles.nth(1).locator('.grab').first();
    const from = await glide(page, grab, { steps: 26 });
    const target = await tiles.nth(0).boundingBox();
    await page.mouse.down();
    await page.waitForTimeout(250);
    const tx = target.x + target.width * 0.45, ty = target.y + target.height * 0.5;
    const steps = 34;
    for (let i = 1; i <= steps; i++) {
      const k = i / steps, e = 1 - Math.pow(1 - k, 3);
      await page.mouse.move(from.x + (tx - from.x) * e, from.y + (ty - from.y) * e);
      await page.waitForTimeout(16);
    }
    await page.waitForTimeout(300);
    await page.mouse.up();
    await page.waitForTimeout(900);
    const done = page.getByRole('button', { name: 'Готово' }).first();
    await glide(page, done, { steps: 24 });
    await done.click({ force: true });
  });

  await b.close();
})().catch((e) => { console.error(e); process.exit(1); });
