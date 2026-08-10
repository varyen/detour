import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import "./styles/tokens.css";
import "./styles/base.css";
import { registerPanelSW } from "./pwa";

createApp(App).use(createPinia()).use(router).mount("#app");

/* Service worker регистрируем сами: по http:// (панель на 192.168.8.1:8080)
   браузер его не отдаст, и автоматическая регистрация из плагина сыпала бы
   ошибкой в консоль на каждом заходе с LAN. */
void registerPanelSW();
