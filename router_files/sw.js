/* Detour panel service worker — Web Push receiver.
 *
 * The router sends payload-LESS pushes (it can't AES-GCM-encrypt a payload from
 * BusyBox/OpenSSL CLI), so `event.data` is empty. On a push we fetch the latest
 * message text from the panel CGI (same-origin → the detour_session cookie is
 * sent automatically) and show it. If that fetch fails (e.g. session expired)
 * we fall back to generic text so the user still gets notified. */

self.addEventListener("install", function () {
  self.skipWaiting();
});

self.addEventListener("activate", function (event) {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("push", function (event) {
  event.waitUntil(
    (async function () {
      var title = "Detour VPN";
      var body = "Состояние VPN изменилось";
      // Prefer a payload if one ever arrives; otherwise fetch the last message.
      try {
        if (event.data) {
          var p = event.data.json();
          if (p && p.title) {
            title = p.title;
            body = p.body || body;
          }
        }
      } catch (e) {
        /* not JSON — ignore */
      }
      try {
        var r = await fetch("/cgi-bin/detour-api?action=push_message", {
          cache: "no-store",
          credentials: "same-origin",
        });
        if (r && r.ok) {
          var j = await r.json();
          if (j && j.title) {
            title = j.title;
            body = j.body || body;
          }
        }
      } catch (e) {
        /* offline / unauthenticated — keep the fallback text */
      }
      await self.registration.showNotification(title, {
        body: body,
        icon: "/detour/favicon.svg",
        badge: "/detour/favicon.svg",
        tag: "detour-vpn",
        renotify: true,
      });
    })()
  );
});

self.addEventListener("notificationclick", function (event) {
  event.notification.close();
  event.waitUntil(
    (async function () {
      var all = await self.clients.matchAll({
        type: "window",
        includeUncontrolled: true,
      });
      for (var i = 0; i < all.length; i++) {
        if (all[i].url.indexOf("/detour/") >= 0 && "focus" in all[i]) {
          return all[i].focus();
        }
      }
      if (self.clients.openWindow) return self.clients.openWindow("/detour/");
    })()
  );
});
