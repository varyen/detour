/* Сгенерированные фоны лендинга: landing/gen-src/*.jpg → landing/gen/*.webp.
 *
 *   node landing/pack-gen.cjs                 # всё
 *   node landing/pack-gen.cjs night-road      # только перечисленные
 *
 * Сырьё из нейросети (neuropix, nano-banana, 2752×1536, по 2–3 МБ) лежит в
 * gen-src/ и в git не идёт — ссылка на результат живёт 7 суток, поэтому файл
 * скачивается сразу. На сайт уезжает только этот каталог gen/: три размера под
 * srcset и крошечный размытый постер под LQIP, пока грузится основной кадр.
 *
 * Конвертер — sharp из panel/node_modules, как в pack.cjs.
 */
const path = require('path');
const fs = require('fs');

const ROOT = path.join(__dirname, '..');
const sharp = require(path.join(ROOT, 'panel', 'node_modules', 'sharp'));
const SRC = path.join(__dirname, 'gen-src');
const OUT = path.join(__dirname, 'gen');

const SIZES = [
  { suffix: '', width: 1920, quality: 72 },
  { suffix: '-m', width: 960, quality: 70 },
];
const POSTER = { suffix: '-lq', width: 48, quality: 40 };

async function one(name) {
  const src = path.join(SRC, name + '.jpg');
  const meta = await sharp(src).metadata();
  const lines = [];
  for (const s of [...SIZES, POSTER]) {
    const out = path.join(OUT, name + s.suffix + '.webp');
    let img = sharp(src).resize({ width: s.width, withoutEnlargement: true });
    if (s === POSTER) img = img.blur(1.2);
    await img.webp({ quality: s.quality, effort: 6 }).toFile(out);
    const kb = Math.round(fs.statSync(out).size / 1024);
    lines.push(`${path.basename(out)} ${s.width}w ${kb} КБ`);
  }
  console.log(`${name}.jpg ${meta.width}×${meta.height} → ${lines.join(' · ')}`);
}

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const only = process.argv.slice(2);
  const names = fs
    .readdirSync(SRC)
    .filter((f) => /\.jpe?g$/i.test(f))
    .map((f) => f.replace(/\.jpe?g$/i, ''))
    .filter((n) => !only.length || only.includes(n));
  if (!names.length) {
    console.error('нет исходников в landing/gen-src/');
    process.exit(1);
  }
  for (const n of names) await one(n);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
