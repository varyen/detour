/* Самохост шрифтов лендинга: тянет woff2 с Google Fonts один раз и кладёт
 * рядом, чтобы страница не ходила к Google при каждом открытии.
 *
 *   node landing/fonts/fetch.cjs
 *
 * Берутся только подмножества cyrillic и latin (остальные — балласт);
 * на выходе landing/fonts/*.woff2 и fonts.css с @font-face на относительные
 * адреса. Оба идут в git: это ровно те байты, что уезжают на сервер.
 */
const fs = require('fs');
const path = require('path');

const FAMILIES = [
  'family=Commissioner:wght@400..800', /* вариативный: один файл на диапазон */
  'family=JetBrains+Mono:wght@400;500',
];
const SUBSETS = new Set(['cyrillic', 'latin']);
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36';

(async () => {
  const url = 'https://fonts.googleapis.com/css2?' + FAMILIES.join('&') + '&display=swap';
  const css = await (await fetch(url, { headers: { 'User-Agent': UA } })).text();
  const blocks = css.split('@font-face').slice(1);
  let out = '';
  let n = 0;
  for (const raw of blocks) {
    const subset = (raw.match(/\/\*\s*(\S+)\s*\*\//) || [])[1];
    if (!SUBSETS.has(subset)) continue;
    const fam = raw.match(/font-family:\s*'([^']+)'/)[1];
    const wght = raw.match(/font-weight:\s*([\d ]+);/)[1].trim(); /* «400 800» у вариативного */
    const src = raw.match(/url\(([^)]+\.woff2)\)/)[1];
    const range = raw.match(/unicode-range:\s*([^;]+);/)[1];
    const file = `${fam.toLowerCase().replace(/\s+/g, '-')}-${wght.replace(' ', '-')}-${subset}.woff2`;
    const buf = Buffer.from(await (await fetch(src)).arrayBuffer());
    fs.writeFileSync(path.join(__dirname, file), buf);
    out += `@font-face{font-family:"${fam}";font-style:normal;font-weight:${wght};font-display:swap;` +
      `src:url(${file}) format("woff2");unicode-range:${range}}\n`;
    n++;
    console.log(`${file} ${Math.round(buf.length / 1024)} КБ`);
  }
  fs.writeFileSync(path.join(__dirname, 'fonts.css'), out);
  console.log(`fonts.css: ${n} начертаний`);
})().catch((e) => { console.error(e); process.exit(1); });
