/* Ссылка подписки в любом из видов, которые раздают сервисы: голый https://…
   или обёртка «открыть в приложении» вокруг него —
     sing-box://import-remote-profile?url=<https…>#Имя
     happ://add/<https…>
     clash://install-config?url=…, v2rayng://install-config?url=…,
     hiddify://import/<https…>#Имя, streisand://import/…, karing://…
   Обёртка разворачивается до вложенного адреса; название (#фрагмент или
   параметр name) отдаётся отдельно — оно станет названием подписки. */

export type SubLink = { url: string; name: string } | { error: string };

/** Голый http(s) адрес подписки: с путём или запросом, без логина в адресе
    (http://user:pass@host:port — это HTTP-прокси, а не подписка). */
function plainSubscription(t: string): string | null {
  try {
    const u = new URL(t);
    if (u.protocol !== "https:" && u.protocol !== "http:") return null;
    if (u.username || u.password) return null;
    if (u.pathname.length <= 1 && !u.search) return null;
    return u.toString();
  } catch {
    return null;
  }
}

function decodeMaybe(s: string): string {
  let v = s;
  /* Бывает закодировано дважды (https%253A%252F…) — снимаем, пока меняется. */
  for (let i = 0; i < 3 && /%[0-9a-f]{2}/i.test(v); i++) {
    try {
      const d = decodeURIComponent(v);
      if (d === v) break;
      v = d;
    } catch {
      break;
    }
  }
  return v;
}

export function parseSubscriptionLink(raw: string): SubLink | null {
  const t = raw.trim();
  if (!t || /\s/.test(t)) return null;
  const plain = plainSubscription(t);
  if (plain) return { url: plain, name: "" };

  const m = /^([a-z][a-z0-9+.-]*):\/\/(.*)$/i.exec(t);
  if (!m) return null;
  const scheme = m[1].toLowerCase();
  if (["vless", "vmess", "trojan", "ss", "ssr", "hysteria", "hysteria2", "hy2", "tuic", "socks", "socks5", "wireguard", "wg", "anytls", "vpn", "http", "https"].includes(scheme)) {
    return null;
  }
  let rest = m[2];
  let name = "";
  const hash = rest.indexOf("#");
  if (hash >= 0) {
    name = decodeMaybe(rest.slice(hash + 1)).trim();
    rest = rest.slice(0, hash);
  }

  if (scheme === "happ" && /^crypt\d*\//i.test(rest)) {
    return {
      error:
        "Это зашифрованная ссылка Happ (happ://crypt…) — расшифровать её может только само приложение Happ. Попросите у поставщика обычную ссылку подписки https://…",
    };
  }

  /* Адрес в параметре: ?url= / ?config= / ?sub= (sing-box, clash, v2rayNG…). */
  const q = rest.indexOf("?");
  if (q >= 0) {
    const params = new URLSearchParams(rest.slice(q + 1));
    for (const k of ["url", "config", "sub", "link"]) {
      const v = params.get(k);
      if (!v) continue;
      const u = plainSubscription(decodeMaybe(v).trim());
      if (u) return { url: u, name: name || (params.get("name") ?? "").trim() };
    }
  }

  /* Адрес в пути: happ://add/https://…, hiddify://import/https%3A%2F%2F… */
  const decoded = decodeMaybe(rest);
  const at = decoded.search(/https?:\/\//i);
  if (at >= 0) {
    const u = plainSubscription(decoded.slice(at));
    if (u) return { url: u, name };
  }
  return null;
}
