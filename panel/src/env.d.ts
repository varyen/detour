/// <reference types="vite/client" />
/// <reference types="vite-plugin-pwa/client" />

/** Версия панели, зашитая на сборке (см. define в vite.config.ts). */
declare const __PANEL_BUILD__: string;

/** Сборка для приложения Detour (`--mode client`), а не для роутера. */
declare const __CLIENT__: boolean;
