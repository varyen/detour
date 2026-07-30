#!/usr/bin/env sh
# Пересборка landing/img/*.webp из landing/img/*.png.
#
#   ./landing/webp.sh            # все кадры
#   ./landing/webp.sh 05 13      # только совпадающие по префиксу имени
#
# Ширина ограничивается 1560 px (мобильный кадр — 620 px), качество 0.86.
# Конвертер — headless Chromium из playwright: отдельных пакетов (cwebp,
# ImageMagick, Pillow) на рабочей машине нет, а браузер уже стоит для съёмки.
set -eu

DIR="$(cd "$(dirname "$0")" && pwd)"
PW="${PLAYWRIGHT_MODULE:-/usr/local/lib/node_modules/playwright/index.js}"
CHROME="${CHROME_PATH:-/root/.cache/ms-playwright/chromium-1232/chrome-linux64/chrome}"

IMG_DIR="$DIR/img" PW_MODULE="$PW" CHROME_BIN="$CHROME" FILTER="$*" node - <<'EOF'
const { chromium } = require(process.env.PW_MODULE);
const fs = require('fs'), path = require('path');
const DIR = process.env.IMG_DIR + '/';
const filter = (process.env.FILTER || '').trim().split(/\s+/).filter(Boolean);
const MAXW = { '24-mobile': 620 };
const DEFAULT_MAXW = 1560;

(async () => {
  const files = fs.readdirSync(DIR)
    .filter(f => f.endsWith('.png') && f !== 'og.png')
    .filter(f => !filter.length || filter.some(x => f.startsWith(x)))
    .sort();
  if (!files.length) { console.log('нечего конвертировать'); return; }

  const b = await chromium.launch({ executablePath: process.env.CHROME_BIN, args: ['--no-sandbox'] });
  const p = await b.newPage();
  await p.goto('about:blank');
  for (const f of files) {
    const key = path.basename(f, '.png');
    const buf = fs.readFileSync(DIR + f);
    const out = await p.evaluate(async ([uri, maxw]) => {
      const img = new Image();
      img.src = uri;
      await img.decode();
      const scale = Math.min(1, maxw / img.width);
      const c = document.createElement('canvas');
      c.width = Math.round(img.width * scale);
      c.height = Math.round(img.height * scale);
      const g = c.getContext('2d');
      g.imageSmoothingQuality = 'high';
      g.drawImage(img, 0, 0, c.width, c.height);
      return { data: c.toDataURL('image/webp', 0.86).split(',')[1], w: c.width, h: c.height };
    }, ['data:image/png;base64,' + buf.toString('base64'), MAXW[key] || DEFAULT_MAXW]);
    const ob = Buffer.from(out.data, 'base64');
    fs.writeFileSync(DIR + key + '.webp', ob);
    console.log(key.padEnd(20), out.w + 'x' + out.h, (buf.length / 1024 | 0) + 'K → ' + (ob.length / 1024 | 0) + 'K');
  }
  await b.close();
})();
EOF

echo "готово. дальше: $DIR/deploy.sh"
