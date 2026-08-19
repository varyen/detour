/* Detour panel service worker — Web Push receiver.
 *
 * The router sends payload-LESS pushes (it can't AES-GCM-encrypt a payload from
 * BusyBox/OpenSSL CLI), so `event.data` is empty. On a push we fetch the latest
 * message text from the panel CGI and show it.
 *
 * AUTH: we present our OWN push endpoint (POST body) as the credential, not just
 * the detour_session cookie. The cookie lives 7 days and a push routinely arrives
 * after that — cookie-only auth made the CGI 401 and every alert collapsed into the
 * generic fallback below ("Состояние VPN изменилось") instead of "«A» → «B»". The
 * endpoint is the capability the push service issued to this browser and the router
 * only knows it from an authenticated subscribe. Cookie auth is kept as a fallback
 * (older routers have no endpoint-authed branch). */

self.addEventListener("install", function () {
  self.skipWaiting();
});

self.addEventListener("activate", function (event) {
  event.waitUntil(self.clients.claim());
});

/* One attempt at the message text. Returns the {title,body,ts} object or null —
 * an unauthenticated CGI answers {"ok":false,"error":"auth"} (no title). */
async function fetchMessage(init) {
  init.cache = "no-store";
  init.credentials = "same-origin";
  var r = await fetch("/cgi-bin/detour-api?action=push_message", init);
  if (!r || !r.ok) return null;
  var j = await r.json();
  return j && j.title ? j : null;
}

self.addEventListener("push", function (event) {
  event.waitUntil(
    (async function () {
      var title = "Detour VPN";
      var body = "Состояние VPN изменилось — откройте панель Detour.";
      var msg = null;
      // Prefer a payload if one ever arrives; otherwise fetch the last message.
      try {
        if (event.data) {
          var p = event.data.json();
          if (p && p.title) msg = p;
        }
      } catch (e) {
        /* not JSON — ignore */
      }
      if (!msg) {
        var endpoint = null;
        try {
          var sub = await self.registration.pushManager.getSubscription();
          endpoint = sub && sub.endpoint;
        } catch (e) {
          /* no subscription → cookie-only below */
        }
        // 1) endpoint-authed (survives an expired session); 2) the same once more,
        // because a push wakes the SW before the radio is necessarily usable and a
        // first-shot network error would otherwise cost us the real text; 3) the
        // legacy cookie-authed GET (routers without the endpoint-authed branch).
        var attempts = [];
        if (endpoint) {
          attempts.push({ method: "POST", body: endpoint });
          attempts.push({ method: "POST", body: endpoint, delay: 1500 });
        }
        attempts.push({});
        for (var i = 0; i < attempts.length && !msg; i++) {
          var a = attempts[i];
          if (a.delay) {
            var d = a.delay;
            delete a.delay;
            await new Promise(function (res) {
              setTimeout(res, d);
            });
          }
          try {
            msg = await fetchMessage(a);
          } catch (e) {
            /* offline / unauthenticated — try the next attempt */
          }
        }
      }
      if (msg) {
        title = msg.title;
        body = msg.body || body;
      }
      await self.registration.showNotification(title, {
        body: body,
        icon: "/detour-old/favicon.svg",
        badge: "/detour-old/favicon.svg",
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
      /* Только СВОЯ вкладка: этот SW обслуживает старую панель на /detour-old/,
       * а на /detour/ теперь живёт новая со своим воркером. Открытая новая панель
       * не должна перехватывать клик по уведомлению старой. */
      for (var i = 0; i < all.length; i++) {
        if (all[i].url.indexOf("/detour-old/") >= 0 && "focus" in all[i]) {
          return all[i].focus();
        }
      }
      if (self.clients.openWindow) return self.clients.openWindow("/detour-old/");
    })()
  );
});
