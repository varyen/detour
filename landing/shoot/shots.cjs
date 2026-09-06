/* Съёмка кадров панели для лендинга.
 *
 *   node shots.cjs               — все кадры
 *   node shots.cjs overview m-*  — только совпадающие по имени
 *
 * Панель берётся из dev-сервера vite (текущая версия), API — через
 * обезличивающий прокси proxy.py, поэтому на кадрах нет реальных имён
 * провайдеров, серверов и адресов.
 */
const { chromium } = require('playwright-core');
const fs = require('fs');
const path = require('path');

/* Chromium берём тот, что уже лежит в кэше playwright: пакет playwright-core
   ставится без браузеров, а версия сборки в кэше обычно не совпадает с той,
   которую он ждёт по умолчанию — отсюда явный executablePath. */
const CHROME = process.env.CHROME_BIN || findChrome();
const BASE = process.env.BASE || 'http://localhost:5173/detour/';
const OUT = process.env.OUT || path.join(__dirname, '..', 'raw');
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
const want = (n) => !only.length || only.some((p) => n === p || (p.endsWith('*') && n.startsWith(p.slice(0, -1))));


async function prep(page, theme) {
  /* Флаги: Windows не рисует региональные индикаторы вообще, поэтому
     подмешиваем Noto Color Emoji — но ТОЛЬКО на диапазон флагов
     (unicode-range). Без ограничения цифры уехали бы в эмодзи-шрифт: в нём
     есть глифы клавиш-цифр, и «183 Мбит/с» рендерилось враскоряку. */
  await page.evaluate(async (t) => {
    localStorage.setItem('detour-theme', t);
    document.documentElement.dataset.theme = t;
    let src = '';
    try {
      const css = await (
        await fetch('https://fonts.googleapis.com/css2?family=Noto+Color+Emoji&display=swap')
      ).text();
      src = (css.match(/url\(([^)]+\.woff2)\)/) || [])[1] || '';
    } catch (e) { /* без сети останутся буквенные пары */ }
    const orig = getComputedStyle(document.body).fontFamily;
    const st = document.createElement('style');
    st.id = 'emoji-fix';
    st.textContent =
      (src
        ? `@font-face{font-family:"FlagEmoji";src:url(${src}) format("woff2");` +
          `unicode-range:U+1F1E6-1F1FF;font-display:block}`
        : '') + `body{font-family:"FlagEmoji", ${orig} !important}`;
    document.head.appendChild(st);
    if (document.fonts && document.fonts.load) {
      try { await document.fonts.load('16px FlagEmoji', '\u{1F1F3}\u{1F1F1}'); } catch (e) {}
    }
  }, theme);
}

async function go(page, hash, theme = 'dark', wait = 3800) {
  await page.goto(BASE + hash, { waitUntil: 'domcontentloaded' });
  await prep(page, theme);
  await page.waitForTimeout(wait);
}

async function shot(page, name, opts = {}) {
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${OUT}/${name}.png`, ...opts });
  console.log('  ✓', name);
}

/* Клик по строке-аккордеону раздела: заголовки уникальны в пределах экрана. */
async function open(page, text) {
  /* Кликать надо по самой кнопке-заголовку: force-клик по текстовому узлу
     внутри неё события раскрытия не даёт (проверено на «Сервисах» — секции
     оставались закрытыми, а кадр выходил пустым). */
  const before = await page.evaluate(() => document.body.scrollHeight);
  const btn = page.locator('button, summary, [role="button"]').filter({ hasText: text }).first();
  await btn.scrollIntoViewIfNeeded();
  await btn.click({ force: true, timeout: 20000 });
  await page.waitForTimeout(1800);
  const after = await page.evaluate(() => document.body.scrollHeight);
  if (after <= before) console.log('    (не раскрылось:', text + ')');
}

async function step(name, fn) {
  if (!want(name)) return;
  try {
    await fn();
  } catch (e) {
    console.log('  ✗', name, String(e).split('\n')[0].slice(0, 120));
  }
}

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const b = await chromium.launch({ executablePath: CHROME });

  /* ---------------- десктоп ---------------- */
  const dctx = await b.newContext({
    viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2,
    serviceWorkers: 'block', locale: 'ru-RU', timezoneId: 'Europe/Moscow',
  });
  const page = await dctx.newPage();
  await page.goto(BASE, { waitUntil: 'domcontentloaded' });
  await prep(page, 'dark');
  await page.waitForTimeout(4000);

  await step('overview', async () => {
    await go(page, '#/');
    await shot(page, 'overview', { fullPage: true });
  });

  await step('overview-top', async () => {
    await go(page, '#/');
    await shot(page, 'overview-top');
  });

  await step('overview-light', async () => {
    await go(page, '#/', 'light');
    await shot(page, 'overview-light', { fullPage: true });
  });

  await step('palette', async () => {
    await go(page, '#/');
    await page.keyboard.press('Control+KeyK');
    await page.waitForTimeout(1400);
    await shot(page, 'palette');
    await page.keyboard.press('Escape');
  });

  await step('dash-edit', async () => {
    await go(page, '#/');
    await page.getByRole('button', { name: /Настроить главную/i }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(1500);
    await shot(page, 'dash-edit', { fullPage: true });
  });

  await step('profiles', async () => {
    await go(page, '#/profiles?sort=speed', 'dark', 5000);
    await shot(page, 'profiles');
  });

  await step('profiles-country', async () => {
    await go(page, '#/profiles?sort=speed', 'dark', 5000);
    const sel = page.locator('select').nth(1);
    await sel.selectOption({ index: 3 }).catch(() => {});
    await page.waitForTimeout(1200);
    await shot(page, 'profiles-country');
  });

  await step('profile-sheet', async () => {
    await go(page, '#/profiles?sort=speed', 'dark', 5000);
    await page.locator('.row .nm').nth(1).click({ force: true, timeout: 20000 });
    await page.waitForTimeout(2200);
    await shot(page, 'profile-sheet');
  });

  await step('chains', async () => {
    await go(page, '#/profiles', 'dark', 4200);
    await page.getByRole('button', { name: 'Цепочки' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(1800);
    await shot(page, 'chains');
  });

  await step('chain-editor', async () => {
    await go(page, '#/profiles', 'dark', 4200);
    await page.getByRole('button', { name: 'Цепочки' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(1800);
    await page.getByRole('button', { name: 'Править' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(2000);
    await shot(page, 'chain-editor');
  });

  await step('subs', async () => {
    await go(page, '#/profiles', 'dark', 4200);
    await page.getByRole('button', { name: 'Подписки' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(1800);
    await shot(page, 'subs');
  });

  await step('warp', async () => {
    await go(page, '#/profiles', 'dark', 4200);
    await page.getByRole('button', { name: 'WARP' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(1800);
    await shot(page, 'warp');
  });

  await step('rules', async () => {
    await go(page, '#/rules');
    await shot(page, 'rules', { fullPage: true });
  });

  await step('rules-routes', async () => {
    await go(page, '#/rules');
    await open(page, 'Отдельные маршруты');
    await shot(page, 'rules-routes', { fullPage: true });
  });

  await step('rules-tunnel', async () => {
    await go(page, '#/rules');
    await open(page, 'Что уходит в туннель');
    await shot(page, 'rules-tunnel', { fullPage: true });
  });

  await step('rules-dpi', async () => {
    await go(page, '#/rules');
    await open(page, 'Сайты через обход DPI');
    await shot(page, 'rules-dpi', { fullPage: true });
  });

  await step('rules-ru', async () => {
    await go(page, '#/rules');
    await open(page, 'Российские адреса');
    await shot(page, 'rules-ru', { fullPage: true });
  });

  await step('route-editor', async () => {
    await go(page, '#/rules');
    await open(page, 'Отдельные маршруты');
    await page.getByRole('button', { name: 'Настроить маршруты' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(2200);
    await shot(page, 'route-editor');
  });

  await step('list-editor', async () => {
    await go(page, '#/rules');
    /* Раскрытые секции панель помнит между заходами, поэтому кнопку берём
       внутри нужной секции, а не первую попавшуюся на странице. */
    await open(page, 'Сайты через обход DPI');
    await page
      .locator('section, article, .card, div')
      .filter({ hasText: /^Сайты через обход DPI/ })
      .last()
      .getByRole('button', { name: 'Редактировать список' })
      .click({ force: true, timeout: 20000 });
    await page.waitForSelector('.sheet', { timeout: 15000 });
    await page.waitForTimeout(1800);
    await shot(page, 'list-editor');
  });

  await step('whitelist-editor', async () => {
    await go(page, '#/rules');
    await open(page, 'Всегда напрямую');
    await page.getByRole('button', { name: 'Редактировать список' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(2000);
    await shot(page, 'whitelist-editor');
  });

  await step('cert-sheet', async () => {
    await go(page, '#/services');
    await open(page, 'Свой домен и защищённое соединение');
    await page.getByRole('button', { name: 'Выпустить заново' }).first().click({ force: true, timeout: 20000 });
    await page.waitForTimeout(2000);
    await shot(page, 'cert-sheet');
  });

  await step('services', async () => {
    await go(page, '#/services');
    await shot(page, 'services', { fullPage: true });
  });

  await step('services-portmap', async () => {
    await go(page, '#/services');
    await open(page, 'Доступ к домашним устройствам снаружи');
    await shot(page, 'services-portmap', { fullPage: true });
  });

  await step('services-cert', async () => {
    await go(page, '#/services');
    await open(page, 'Свой домен и защищённое соединение');
    await shot(page, 'services-cert', { fullPage: true });
  });

  await step('services-push', async () => {
    await go(page, '#/services');
    await open(page, 'Уведомления в браузер');
    await shot(page, 'services-push', { fullPage: true });
  });

  await step('journal', async () => {
    await go(page, '#/journal');
    await shot(page, 'journal', { fullPage: true });
  });

  await step('journal-fw', async () => {
    await go(page, '#/journal');
    await open(page, 'ФАЙРВОЛ');
    await shot(page, 'journal-fw', { fullPage: true });
  });

  await step('journal-config', async () => {
    await go(page, '#/journal');
    await open(page, 'КОНФИГУРАЦИЯ SING-BOX');
    await shot(page, 'journal-config', { fullPage: true });
  });

  await step('journal-updates', async () => {
    await go(page, '#/journal');
    await open(page, 'ОБНОВЛЕНИЯ');
    await shot(page, 'journal-updates', { fullPage: true });
  });

  await dctx.close();

  /* ---------------- телефон ---------------- */
  const mctx = await b.newContext({
    viewport: { width: 390, height: 844 }, deviceScaleFactor: 3, isMobile: true,
    hasTouch: true, serviceWorkers: 'block', locale: 'ru-RU', timezoneId: 'Europe/Moscow',
    userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Mobile Safari/537.36',
  });
  const m = await mctx.newPage();
  await m.goto(BASE, { waitUntil: 'domcontentloaded' });
  await prep(m, 'dark');
  await m.waitForTimeout(4000);

  const mgo = async (hash, theme = 'dark', wait = 4200) => {
    await m.goto(BASE + hash, { waitUntil: 'domcontentloaded' });
    await prep(m, theme);
    await m.waitForTimeout(wait);
  };

  await step('m-overview', async () => { await mgo('#/'); await shot(m, 'm-overview'); });
  await step('m-overview-full', async () => { await mgo('#/'); await shot(m, 'm-overview-full', { fullPage: true }); });
  await step('m-profiles', async () => { await mgo('#/profiles?sort=speed', 'dark', 5200); await shot(m, 'm-profiles'); });
  await step('m-profile-sheet', async () => {
    await mgo('#/profiles?sort=speed', 'dark', 5200);
    await m.locator('.row .nm').nth(1).click();
    await m.waitForTimeout(1800);
    await shot(m, 'm-profile-sheet');
  });
  await step('m-rules', async () => { await mgo('#/rules'); await shot(m, 'm-rules'); });
  await step('m-services', async () => { await mgo('#/services'); await shot(m, 'm-services'); });
  await step('m-journal', async () => { await mgo('#/journal'); await shot(m, 'm-journal'); });
  await step('m-overview-light', async () => { await mgo('#/', 'light'); await shot(m, 'm-overview-light'); });

  await mctx.close();
  await b.close();
  console.log('готово →', OUT);
})();
