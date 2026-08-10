/* Генерация иконок PWA из одного SVG.
 * Запускать вручную после правки favicon.svg: `node scripts/icons.mjs`.
 * Результат коммитится в public/icons — на сборке роутера ничего не рисуется.
 */
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const here = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(here, "../public/icons");
const SRC = resolve(OUT, "favicon.svg");

/* Маскируемая иконка обрезается системой по кругу/скруглению: рисунок должен
   умещаться в центральные 80%, иначе Android съест угол маршрута. */
const MASKABLE_INSET = 0.72;

async function render(svg, size, { inset = 1, bg = null } = {}) {
  const inner = Math.round(size * inset);
  const art = await sharp(Buffer.from(svg))
    .resize(inner, inner, { fit: "contain", background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();

  if (inset === 1 && !bg) return art;

  const pad = Math.round((size - inner) / 2);
  return sharp({
    create: {
      width: size,
      height: size,
      channels: 4,
      background: bg ?? { r: 0, g: 0, b: 0, alpha: 0 },
    },
  })
    .composite([{ input: art, top: pad, left: pad }])
    .png()
    .toBuffer();
}

const svg = await readFile(SRC, "utf8");
await mkdir(OUT, { recursive: true });

const jobs = [
  ["icon-192.png", 192, {}],
  ["icon-512.png", 512, {}],
  ["badge-96.png", 96, {}],
  /* Фон маскируемой — глубокий синий фона панели, чтобы на любой системе
     плитка выглядела как часть приложения, а не как наклейка. */
  [
    "icon-maskable-512.png",
    512,
    { inset: MASKABLE_INSET, bg: { r: 7, g: 12, b: 18, alpha: 1 } },
  ],
];

for (const [name, size, opts] of jobs) {
  await writeFile(resolve(OUT, name), await render(svg, size, opts));
  console.log(`${name} — ${size}×${size}`);
}
