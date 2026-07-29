# Вендоренный QR-генератор (`router_files/qrcode-generator.min.js`)

Вторая (и последняя) сторонняя JS-библиотека в проекте. Нужна, чтобы **рисовать** QR
в модале «Доступ с другого устройства» — вендоренный [`jsQR`](vendor-jsqr.md) умеет
только читать. Нативного браузерного API для генерации QR не существует, поэтому
фолбэка «как с BarcodeDetector» тут нет.

- Источник: [`qrcode-generator`](https://www.npmjs.com/package/qrcode-generator) v2.0.4
  (Kazuhiko Arase)
- Лицензия: MIT (шапка с атрибуцией — в самом файле)
- Собрано из `dist/qrcode.js` + `dist/qrcode_UTF8.js`, минифицировано `terser 5 -c -m`
- Размер: 57 КБ → **20 КБ** (~7 КБ gzip). Изменений в логике **нет**
- Грузится лениво — только при открытии модала

## Как пересобрать

```sh
curl -sSL -o qrgen.tgz https://registry.npmjs.org/qrcode-generator/-/qrcode-generator-2.0.4.tgz
tar xzf qrgen.tgz
npx --yes terser@5 package/dist/qrcode.js package/dist/qrcode_UTF8.js \
    -c -m --comments false -o qrgen.min.js
# приклеить шапку из текущего router_files/qrcode-generator.min.js и положить на место
```

## Проверка round-trip (генератор ↔ наш декодер)

Самый полезный тест: закодировать реальный pairing-URL и прочитать его вендоренным
jsQR — если оба наших файла согласованы, скан на втором телефоне заработает.

```sh
node -e '
const qrcode=require("./router_files/qrcode-generator.min.js");
const jsQR=require("./router_files/jsQR.min.js");
const u="https://panel.example.com/detour/index.html#pair="+"f".repeat(64);
const qr=qrcode(0,"M"); qr.addData(u); qr.make();
const n=qr.getModuleCount(), S=6, Q=4, W=(n+Q*2)*S;
const buf=Buffer.alloc(W*W*4,255);
for(let y=0;y<n;y++)for(let x=0;x<n;x++){ if(!qr.isDark(y,x))continue;
  for(let dy=0;dy<S;dy++)for(let dx=0;dx<S;dx++){
    const p=((y*S+Q*S+dy)*W+(x*S+Q*S+dx))*4; buf[p]=buf[p+1]=buf[p+2]=0; }}
const r=jsQR(new Uint8ClampedArray(buf),W,W);
console.log(r && r.data===u ? "OK" : "FAIL");
'
```

Проверялось на трёх реальных вариантах URL (HTTPS-домен, LAN-адрес с портом, длинный
поддомен с портом) — 3/3, версии QR 45×45 и 49×49 при EC-уровне `M`.

## Упаковка

Как и `jsQR.min.js`, файл перечислен в **обоих** манифестах: `build_release.py` →
`PANEL_FILES` и `keenetic/build-ipk.py` → `FILES`.
