import { ref } from "vue";

/* PWA-возможности доступны только в защищённом контексте: по HTTPS (панель
   через nginx/KeenDNS) или на localhost. С LAN-адреса http://192.168.8.1:8080
   браузер откажет — это не ошибка, панель просто работает как обычный сайт,
   без установки, офлайна и уведомлений. */
export const swReady = ref(false);
export const swSupported = ref(false);
export const updateAvailable = ref(false);

let registration: ServiceWorkerRegistration | null = null;

export function getRegistration() {
  return registration;
}

export async function registerPanelSW(): Promise<void> {
  swSupported.value = "serviceWorker" in navigator && window.isSecureContext;
  if (!swSupported.value) return;
  if (import.meta.env.DEV) return;

  try {
    const { registerSW } = await import("virtual:pwa-register");
    registerSW({
      immediate: true,
      onRegisteredSW(_url, reg) {
        registration = reg ?? null;
        swReady.value = true;
      },
      onNeedRefresh() {
        /* Новая версия панели уже скачана — предлагаем перезагрузить, но не
           дёргаем страницу под руками у человека посреди операции. */
        updateAvailable.value = true;
      },
    });
  } catch {
    /* Воркер не зарегистрировался — панель остаётся полностью рабочей. */
    swSupported.value = false;
  }
}

/** Применить обновление оболочки: активировать нового воркера и перезагрузиться. */
export async function applyPanelUpdate(): Promise<void> {
  const reg = registration ?? (await navigator.serviceWorker?.getRegistration());
  await reg?.waiting?.postMessage({ type: "SKIP_WAITING" });
  location.reload();
}
