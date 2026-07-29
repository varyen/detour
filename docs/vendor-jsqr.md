# Вендоренный jsQR (`router_files/jsQR.min.js`)

Единственная сторонняя JS-библиотека в проекте. Нужна как фолбэк-декодер QR там, где
нет нативного `BarcodeDetector` — прежде всего Safari на iOS и Firefox. На Android
Chrome/Edge библиотека не грузится вообще: панель использует встроенный API браузера.

- Источник: [`cozmo/jsQR`](https://github.com/cozmo/jsQR) v1.4.0, npm-пакет `jsqr`
- Лицензия: Apache-2.0 (шапка с атрибуцией — в самом файле, требование §4b соблюдено)
- Размер: 251 КБ исходника → **47 КБ** (~10 КБ gzip)

## Что изменено относительно upstream

1. **Вырезана таблица Shift-JIS** (Kanji-режим QR) — 137 КБ из 251 КБ. Ссылки
   `vless://`, `trojan://`, `ss://`, `vmess://` и URL подписок кодируются byte-режимом;
   кириллица в `#Name` — тоже byte-режим (UTF-8). Kanji-режим этой сборкой не читается.
2. Минификация `terser 5 -c -m`.

## Как пересобрать

```sh
curl -sSL -o jsqr.tgz https://registry.npmjs.org/jsqr/-/jsqr-1.4.0.tgz
tar xzf jsqr.tgz                       # -> package/dist/jsQR.js

# 1) вырезать литерал shiftJISTable
node -e '
const fs=require("fs");
let s=fs.readFileSync("package/dist/jsQR.js","utf8");
const m=/shiftJISTable\s*=\s*\{/.exec(s);
let j=s.indexOf("{",m.index+m[0].length-1),d=0,k=j;
for(;k<s.length;k++){if(s[k]==="{")d++;else if(s[k]==="}"){d--;if(!d){k++;break}}}
fs.writeFileSync("jsQR.nokanji.js", s.slice(0,j)+"{}"+s.slice(k));
'

# 2) минифицировать
npx --yes terser@5 jsQR.nokanji.js -c -m --comments false -o jsQR.slim.min.js

# 3) приклеить шапку из текущего router_files/jsQR.min.js (первые 20 строк) и положить на место
```

## Как проверить, что сборка живая

Нужны `pngjs` и `qrcode` из npm:

```sh
npm i pngjs qrcode
node -e '
const fs=require("fs"), {PNG}=require("pngjs"), QRCode=require("qrcode");
const jsQR=require("./router_files/jsQR.min.js");
const s="vless://11111111-2222-3333-4444-555555555555@example.com:443#Узел-Кириллица";
QRCode.toFile("q.png", s, {scale:4}).then(()=>{
  const p=PNG.sync.read(fs.readFileSync("q.png"));
  const r=jsQR(new Uint8ClampedArray(p.data), p.width, p.height);
  console.log(r && r.data===s ? "OK" : "FAIL");
});
'
```

Проверялось на `vless://`, `trojan://`, `ss://`, HTTPS-URL подписки и ссылке с кириллицей
в `#Name` — 5/5.

## Упаковка

Файл перечислен в обоих манифестах (вопреки формулировке в `CLAUDE.md`, где сказано про
один): `build_release.py` → `PANEL_FILES` (`www/detour/jsQR.min.js`) и
`keenetic/build-ipk.py` → `FILES` (`opt/share/www/detour/jsQR.min.js`).
