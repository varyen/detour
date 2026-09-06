/* Рендер картинки для соцсетей: landing/og.html → landing/img/og.png, 1200×630.
 *
 *   node landing/shoot/og.cjs
 *
 * Запускать ПОСЛЕ pack.cjs: страница показывает уже собранный img/hero.webp. */
const { chromium } = require('playwright-core');
const fs = require('fs');
const path = require('path');

const LANDING = path.join(__dirname, '..');

function findChrome() {
  const roots = [
    process.env.PLAYWRIGHT_BROWSERS_PATH,
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'ms-playwright'),
    process.env.HOME && path.join(process.env.HOME, '.cache', 'ms-playwright'),
  ].filter(Boolean);
  for (const root of roots) {
    if (!fs.existsSync(root)) continue;
    for (const d of fs.readdirSync(root).filter((x) => x.startsWith('chromium-')).sort().reverse()) {
      for (const rel of ['chrome-win64/chrome.exe', 'chrome-win/chrome.exe', 'chrome-linux64/chrome', 'chrome-linux/chrome']) {
        const p = path.join(root, d, rel);
        if (fs.existsSync(p)) return p;
      }
    }
  }
  throw new Error('не нашёл chromium: задайте CHROME_BIN');
}

(async () => {
  const b = await chromium.launch({ executablePath: process.env.CHROME_BIN || findChrome() });
  const p = await b.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });
  await p.goto('file:///' + path.join(LANDING, 'og.html').replace(/\\/g, '/'), { waitUntil: 'load' });
  await p.waitForTimeout(700);
  await p.screenshot({ path: path.join(LANDING, 'img', 'og.png') });
  await b.close();
  console.log('img/og.png 1200×630');
})();
