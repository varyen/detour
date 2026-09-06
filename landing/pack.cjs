/* Сборка картинок лендинга: landing/raw/*.png → landing/img/*.webp.
 * Картинку для соцсетей собирает отдельно landing/shoot/og.cjs.
 *
 *   node landing/pack.cjs            # все кадры
 *   node landing/pack.cjs hero subs  # только перечисленные
 *
 * Кадры снимает landing/shoot/shots.cjs (см. landing/README.md) — там они
 * называются по экрану панели, здесь получают имя, под которым лежат на сайте,
 * обрезаются по содержимому и ужимаются.
 *
 * Конвертер — sharp из panel/node_modules: отдельного пакета ставить не надо,
 * а Chromium-canvas из прежнего webp.sh давал заметно худший результат на том
 * же весе.
 */
const path = require('path');
const fs = require('fs');

const ROOT = path.join(__dirname, '..');
const sharp = require(path.join(ROOT, 'panel', 'node_modules', 'sharp'));
const RAW = path.join(__dirname, 'raw');
const IMG = path.join(__dirname, 'img');

/* width  — ширина на сайте (2× от неё уже перебор для webp);
   crop   — сколько взять сверху от исходного кадра (в пикселях кадра), когда
            внизу пусто или начинается неинтересное. */
const SHOTS = [
  { out: 'hero', src: 'overview-top', width: 1560 },
  { out: 'overview-light', src: 'overview-light', width: 1560, crop: 1900 },
  { out: 'profiles', src: 'profiles', width: 1560 },
  { out: 'profile', src: 'profile-sheet', width: 1560 },
  { out: 'routes', src: 'route-editor', width: 1560 },
  { out: 'modes', src: 'rules-tunnel', width: 1560, crop: 1500 },
  { out: 'lists', src: 'list-editor', width: 1560 },
  { out: 'chain', src: 'chain-editor', width: 1560 },
  { out: 'subs', src: 'subs', width: 1560, crop: 2250 },
  { out: 'warp', src: 'warp', width: 1560, crop: 1300 },
  { out: 'portmap', src: 'services-portmap', width: 1560, crop: 2950 },
  { out: 'cert', src: 'cert-sheet', width: 1560 },
  { out: 'journal', src: 'journal-fw', width: 1560, crop: 1700 },
  { out: 'palette', src: 'palette', width: 1560 },
  { out: 'dash', src: 'dash-edit', width: 1560, crop: 2400 },
  { out: 'm-overview', src: 'm-overview', width: 620 },
  { out: 'm-profiles', src: 'm-profiles', width: 620 },
  { out: 'm-profile', src: 'm-profile-sheet', width: 620 },
  { out: 'm-rules', src: 'm-rules', width: 620 },
];

const only = process.argv.slice(2);
const want = (n) => !only.length || only.includes(n);

(async () => {
  fs.mkdirSync(IMG, { recursive: true });
  for (const s of SHOTS) {
    if (!want(s.out)) continue;
    const src = path.join(RAW, s.src + '.png');
    if (!fs.existsSync(src)) {
      console.log('  нет кадра:', s.src);
      continue;
    }
    let img = sharp(src);
    const meta = await img.metadata();
    if (s.crop && s.crop < meta.height) {
      img = img.extract({ left: 0, top: 0, width: meta.width, height: s.crop });
    }
    const buf = await img
      .resize({ width: s.width, withoutEnlargement: true })
      .webp({ quality: 86 })
      .toBuffer();
    fs.writeFileSync(path.join(IMG, s.out + '.webp'), buf);
    const m = await sharp(buf).metadata();
    console.log(
      '  ' + s.out.padEnd(16),
      (m.width + '×' + m.height).padEnd(12),
      (buf.length / 1024 | 0) + ' КБ',
    );
  }

  console.log('  дальше: node landing/shoot/og.cjs — соберёт img/og.png');
})();
