/// <reference lib="webworker" />
import { cleanupOutdatedCaches, precacheAndRoute } from "workbox-precaching";

declare const self: ServiceWorkerGlobalScope;

/* Воркер панели делает две несвязанные вещи, и обе обязательны:
   1) прекеш оболочки — панель открывается мгновенно и переживает момент,
      когда роутер перезагружает sing-box и веб-сервер недоступен;
   2) приём Web Push — роутер шлёт пуш БЕЗ содержимого (подписать AES-GCM из
      BusyBox нечем), поэтому текст воркер добирает сам из CGI.
   Именно из-за (2) сборка идёт через injectManifest: сгенерированный workbox
   воркер затёр бы push-логику. */

precacheAndRoute(self.__WB_MANIFEST);
cleanupOutdatedCaches();

self.addEventListener("install", () => {
  void self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("message", (event) => {
  if ((event.data as { type?: string } | undefined)?.type === "SKIP_WAITING") {
    void self.skipWaiting();
  }
});

interface PushMessage {
  title: string;
  body?: string;
}

/** База панели («/detour-next/») — подставляется на сборке. */
const BASE = self.registration.scope;
const API = "/cgi-bin/detour-api?action=push_message";

async function fetchMessage(init: RequestInit): Promise<PushMessage | null> {
  const res = await fetch(API, {
    ...init,
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!res.ok) return null;
  const j = (await res.json()) as PushMessage & { ok?: boolean };
  return j && j.title ? j : null;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

self.addEventListener("push", (event) => {
  event.waitUntil(
    (async () => {
      let title = "Detour VPN";
      let body = "Состояние VPN изменилось — откройте панель Detour.";
      let msg: PushMessage | null = null;

      /* Если содержимое вдруг придёт — используем его. */
      try {
        const p = event.data?.json() as PushMessage | undefined;
        if (p?.title) msg = p;
      } catch {
        /* не JSON — добираем текст запросом */
      }

      if (!msg) {
        let endpoint: string | null = null;
        try {
          const sub = await self.registration.pushManager.getSubscription();
          endpoint = sub?.endpoint ?? null;
        } catch {
          /* подписки нет — остаётся авторизация по cookie */
        }

        /* Порядок попыток важен: сессия живёт 7 дней, а пуш приходит и позже —
           поэтому сначала авторизуемся СВОИМ push-эндпоинтом (эту способность
           роутер выдал именно этому браузеру). Вторая попытка — та же, но с
           паузой: пуш будит воркер раньше, чем поднимается сеть, и первая
           ошибка иначе стоила бы настоящего текста. Третья — старый путь по
           cookie, для роутеров без ветки с эндпоинтом. */
        const attempts: { init: RequestInit; delay?: number }[] = [];
        if (endpoint) {
          attempts.push({ init: { method: "POST", body: endpoint } });
          attempts.push({ init: { method: "POST", body: endpoint }, delay: 1500 });
        }
        attempts.push({ init: {} });

        for (const a of attempts) {
          if (msg) break;
          if (a.delay) await sleep(a.delay);
          try {
            msg = await fetchMessage(a.init);
          } catch {
            /* офлайн или нет авторизации — пробуем следующий способ */
          }
        }
      }

      if (msg) {
        title = msg.title;
        body = msg.body || body;
      }

      await self.registration.showNotification(title, {
        body,
        icon: `${BASE}icons/icon-192.png`,
        badge: `${BASE}icons/badge-96.png`,
        tag: "detour-vpn",
      });
    })(),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  event.waitUntil(
    (async () => {
      const all = await self.clients.matchAll({
        type: "window",
        includeUncontrolled: true,
      });
      for (const c of all) {
        if (c.url.startsWith(BASE) && "focus" in c) return c.focus();
      }
      return self.clients.openWindow(BASE);
    })(),
  );
});
