import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { VitePWA } from "vite-plugin-pwa";

/* Панель — основная, ставится в /www/detour/ (OpenWrt) и /opt/share/www/detour/
   (Keenetic), адрес /detour/. Старая однофайловая переехала на /detour-old/.
   base зашивается в URL ассетов и в manifest, поэтому dist, собранный с этим
   значением, распакованный в /www/detour/, работает как есть. Переопределить
   можно на сборке — DETOUR_BASE=… (напр. для превью на другом префиксе). */
const BASE = process.env.DETOUR_BASE || "/detour/";

/* Для `npm run dev` весь /cgi-bin проксируется на живой роутер: фронт
   разрабатывается против настоящего CGI, а не против моков. */
const DEV_TARGET = process.env.DETOUR_DEV_TARGET || "http://192.168.8.1:8080";

/* Версия панели зашивается в сборку, чтобы приложение могло заметить, что
   браузер держит старую оболочку: uhttpd не шлёт Cache-Control, и после
   обновления пакета вкладка может ещё сутки грузить index.html из кэша. */
function panelVersion(): string {
  try {
    return readFileSync(new URL("../VERSION", import.meta.url), "utf8").trim();
  } catch {
    return "dev";
  }
}

/* Рядом со сборкой кладём build.json — по нему упаковщик проверяет, что dist
   собран из той же версии, которую он сейчас пакует. Иначе панель уехала бы с
   зашитой чужой версией и на роутере вечно считала бы себя устаревшей. */
function buildStamp() {
  return {
    name: "detour-build-stamp",
    generateBundle(this: { emitFile: (f: unknown) => void }) {
      this.emitFile({
        type: "asset",
        fileName: "build.json",
        source: JSON.stringify({ version: panelVersion() }) + "\n",
      });
    },
  };
}

export default defineConfig({
  base: BASE,
  define: {
    __PANEL_BUILD__: JSON.stringify(panelVersion()),
  },
  plugins: [
    vue(),
    buildStamp(),
    VitePWA({
      /* injectManifest, а не generateSW: свой воркер уже принимает Web Push
         (payload-less, с авторизацией по push-эндпоинту) — эту логику нельзя
         потерять, workbox только добавляет прекеш оболочки. */
      strategies: "injectManifest",
      srcDir: "src",
      filename: "sw.ts",
      registerType: "prompt",
      injectRegister: null, // регистрируем вручную, чтобы пережить http://
      /* uhttpd на роутере без /etc/mime.types и про .webmanifest не знает —
         отдаём манифест как .json. */
      manifestFilename: "manifest.json",
      injectManifest: {
        globPatterns: ["**/*.{js,css,html,svg,png,woff2}"],
      },
      manifest: {
        name: "Detour",
        short_name: "Detour",
        description: "Управление обходом блокировок на роутере",
        lang: "ru",
        start_url: BASE,
        scope: BASE,
        display: "standalone",
        orientation: "portrait-primary",
        background_color: "#070c12",
        theme_color: "#070c12",
        icons: [
          { src: "icons/icon-192.png", sizes: "192x192", type: "image/png" },
          { src: "icons/icon-512.png", sizes: "512x512", type: "image/png" },
          {
            src: "icons/icon-maskable-512.png",
            sizes: "512x512",
            type: "image/png",
            purpose: "maskable",
          },
        ],
      },
      devOptions: { enabled: false },
    }),
  ],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  build: {
    target: "es2020",
    /* uhttpd отдаёт файлы без сжатия и держит мало параллельных запросов —
       дробить бандл на десяток чанков контрпродуктивно. Один vendor + один
       app, имена с хешом (кеш-бастинг, которого не было у старой панели). */
    cssCodeSplit: false,
    chunkSizeWarningLimit: 700,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules")) return "vendor";
        },
      },
    },
  },
  server: {
    port: 5199,
    proxy: {
      "/cgi-bin": { target: DEV_TARGET, changeOrigin: true },
    },
  },
});
