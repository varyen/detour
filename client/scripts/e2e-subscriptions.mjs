// Сквозная проверка подписок и резервной копии против живой службы:
//   detour-svc run --dev-http 127.0.0.1:18080   (DETOUR_DATA — пустой каталог, sing-box в <data>/bin)
//   node client/scripts/e2e-subscriptions.mjs
// Поставщик подписок поднимается здесь же, на 127.0.0.1:18099.
import http from "node:http";

const API = process.argv[2] ?? "http://127.0.0.1:18080/cgi-bin/detour-api";
const U = "00000000-0000-0000-0000-000000000001";
const b64 = (s) => Buffer.from(s).toString("base64");

let feedVersion = 1;
const feeds = {
  "/sub-b64": () =>
    b64(
      [
        `vless://${U}@vpn.example.com:443?security=reality&sni=www.example.com&fp=chrome&pbk=K&sid=ab#🇳🇱 Нидерланды`,
        `trojan://pw@vpn.example.com:443?sni=vpn.example.com#DE Trojan`,
        ...(feedVersion === 1 ? [`hysteria2://pw@vpn.example.com:8443?sni=vpn.example.com#FI Hy2`] : []),
        `vless://${U}@vpn.example.com:443?type=xhttp&security=tls#XHTTP`,
        `# комментарий`,
      ].join("\n"),
    ),
  "/sub-singbox": () =>
    JSON.stringify({
      outbounds: [
        { type: "selector", tag: "select", outbounds: ["🇩🇪 DE"] },
        { type: "vless", tag: "🇩🇪 DE", server: "vpn.example.com", server_port: 443, uuid: U, domain_resolver: "dns-remote", tls: { enabled: true, server_name: "vpn.example.com" } },
        { type: "direct", tag: "direct" },
      ],
    }),
  "/sub-v2ray": () =>
    JSON.stringify([
      {
        remarks: "V2 NL",
        outbounds: [
          { protocol: "vless", tag: "proxy", settings: { vnext: [{ address: "vpn.example.com", port: 443, users: [{ id: U, encryption: "none" }] }] }, streamSettings: { network: "ws", security: "tls", tlsSettings: { serverName: "vpn.example.com" }, wsSettings: { path: "/ws", host: "cdn.example.com" } } },
          { protocol: "freedom", tag: "direct" },
        ],
        routing: { rules: [{ outboundTag: "direct", domain: ["domain:ya.ru", "geosite:ru", "full:x.ru"] }] },
      },
    ]),
  "/html": () => "<!doctype html><html><body>login</body></html>",
};

const server = http.createServer((req, res) => {
  const f = feeds[req.url];
  if (!f) {
    res.writeHead(404).end("nope");
    return;
  }
  res.writeHead(200, {
    "content-type": "text/plain; charset=utf-8",
    "profile-title": "Example VPN",
    "subscription-userinfo": "upload=1; download=2; total=100; expire=0",
    "x-seen-ua": req.headers["user-agent"] ?? "",
  });
  res.end(f());
});
await new Promise((r) => server.listen(18099, "127.0.0.1", r));
const FEED = "http://127.0.0.1:18099";

async function call(action, { params = {}, body } = {}) {
  const u = new URL(API);
  u.searchParams.set("action", action);
  for (const [k, v] of Object.entries(params)) u.searchParams.set(k, v);
  const init = body === undefined ? {} : { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body) };
  const text = await (await fetch(u, init)).text();
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

let failures = 0;
const expect = (label, cond, extra = "") => {
  if (!cond) failures++;
  console.log(`${cond ? "ok  " : "FAIL"} ${label}${extra ? " — " + extra : ""}`);
};
const profiles = async () => (await call("profiles_list")).profiles ?? [];
const subs = async () => (await call("subscriptions_list")).subscriptions ?? [];

// Ручной профиль с тем же id, что даст подписка, но из другой папки.
await call("profile_save", { body: { id: "de_trojan", name: "Мой DE", group: "Ручные", outbound: { type: "trojan", server: "own.example.com", server_port: 443, password: "x" } } });

let r = await call("subscription_save_one", { body: { id: "s1", url: `${FEED}/sub-b64`, group: "Example VPN", autoupdate: true, interval_hours: 24, last_refresh: 999, last_status: "ok" } });
expect("save_one", r?.ok === true, r?.error);
let s1 = (await subs()).find((s) => s.id === "s1");
expect("last_* из тела не принимаются", s1 && s1.last_refresh === undefined, JSON.stringify(s1));

r = await call("subscription_fetch", { body: { url: `${FEED}/sub-b64` } });
expect("fetch: тело и заголовки поставщика", r?.ok === true && r.headers?.["profile-title"] === "Example VPN" && r.headers?.["subscription-userinfo"]?.includes("total=100"), JSON.stringify(r?.headers));

r = await call("subscription_refresh_one", { params: { id: "s1" } });
expect("refresh s1", r?.ok === true, r?.error ?? "");
expect("в выводе нет полного URL", typeof r?.output === "string" && !r.output.includes("/sub-b64"), r?.output?.split("\n")[0]);
let ps = await profiles();
const grp = ps.filter((p) => p.group === "Example VPN").map((p) => p.id).sort();
expect("3 профиля в группе (xhttp пропущен)", grp.join(",") === "de_trojan_2,fi_hy2,niderlandy", grp.join(","));
expect("ручной de_trojan не перезаписан", ps.find((p) => p.id === "de_trojan")?.group === "Ручные");
s1 = (await subs()).find((s) => s.id === "s1");
expect("состояние подписки", s1?.last_status === "ok" && s1?.last_saved === 3 && s1?.last_skipped === 1 && s1?.last_source_kind === "uri-list-b64" && s1?.last_via === "direct", JSON.stringify({ st: s1?.last_status, saved: s1?.last_saved, sk: s1?.last_skipped, kind: s1?.last_source_kind, via: s1?.last_via }));

r = await call("profile_activate", { params: { name: "fi_hy2" } });
expect("активировать профиль из подписки (check прошёл)", r?.ok === true || /Access is denied|завершился сразу|TUN в режиме разработки/.test(r?.error ?? ""), r?.error);

feedVersion = 2;
r = await call("subscription_refresh_one", { params: { id: "s1" } });
s1 = (await subs()).find((s) => s.id === "s1");
ps = await profiles();
expect("узел пропал у поставщика, но активный профиль остался", ps.some((p) => p.id === "fi_hy2") && s1?.last_kept_stale === 1, `kept=${s1?.last_kept_stale} removed=${s1?.last_removed}`);

await call("subscription_save_one", { body: { id: "s2", url: `${FEED}/sub-singbox`, group: "SB", user_agent: "sing-box/1.13.2" } });
r = await call("subscription_refresh_one", { params: { id: "s2" } });
ps = await profiles();
const sb = ps.find((p) => p.group === "SB");
expect("sing-box-конфиг: только vless, без selector/direct", r?.ok === true && ps.filter((p) => p.group === "SB").length === 1, r?.error);
const sbProfile = sb ? await call("profile_get", { params: { name: sb.id } }) : null;
expect("domain_resolver вырезан", sbProfile?.outbound && !("domain_resolver" in sbProfile.outbound), sb?.id);

await call("subscription_save_one", { body: { id: "s3", url: `${FEED}/sub-v2ray`, group: "V2", apply_routing: true } });
r = await call("subscription_refresh_one", { params: { id: "s3" } });
const v2 = (await profiles()).find((p) => p.group === "V2");
expect("v2ray JSON → профиль all-except", r?.ok === true && v2?.routing_mode === "all-except", r?.error);
const wl = (await call("whitelist")).whitelist ?? "";
expect("домены direct из фида в whitelist", wl.includes("// === subscription:V2 ===") && wl.includes("ya.ru") && !wl.includes("geosite"), wl.trim().split("\n").slice(-3).join(" | "));

await call("subscription_save_one", { body: { id: "bad", url: `${FEED}/html`, group: "Bad" } });
r = await call("subscription_refresh_one", { params: { id: "bad" } });
const bad = (await subs()).find((s) => s.id === "bad");
expect("HTML вместо подписки — понятная ошибка", r?.ok === false && /HTML page/.test(r?.error ?? "") && bad?.last_status === "error", r?.error);

r = await call("subscription_save_one", { body: { id: "bad id!", url: "ftp://x" } });
expect("валидация id/url", r?.ok === false, r?.error);

const exp = await call("panel_export_config");
expect("экспорт: подписки, списки, настройки", Array.isArray(exp?.subscriptions) && exp.subscriptions.length === 4 && typeof exp.whitelist_domains === "string" && exp.settings?.active_chain === "fi_hy2", `subs=${exp?.subscriptions?.length}`);

await call("whitelist", { body: "" });
await call("subscription_delete_one", { params: { id: "s3" } });
r = await call("panel_import_config", { body: { ...exp, password: undefined } });
expect("импорт своего экспорта", r?.ok === true, r?.error ?? r?.warning);
expect("подписки и whitelist вернулись", (await subs()).length === 4 && ((await call("whitelist")).whitelist ?? "").includes("ya.ru"));
r = await call("panel_import_config", { body: { settings: {}, password: "x" } });
expect("запрет ключей учётки", r?.ok === false && /forbidden key/.test(r?.error ?? ""), r?.error);
r = await call("panel_import_config", { body: { version: 1, settings: { routing_mode: "all-except", active_chain: "gone_profile" }, subscription: { url: `${FEED}/sub-b64`, group: "Legacy" } } });
const st = await call("status");
expect("роутерный бэкап: legacy-подписка, цепочка с чужими профилями не принята", r?.ok === true && (await subs()).some((s) => s.id === "legacy") && st.singbox?.active_chain?.join(",") === "fi_hy2", `chain=${st.singbox?.active_chain}`);

r = await call("subscription_delete_one", { params: { id: "bad" } });
expect("delete_one", r?.ok === true && !(await subs()).some((s) => s.id === "bad"));

server.close();
console.log(failures ? `\n${failures} FAIL` : "\nвсё зелёное");
process.exit(failures ? 1 : 0);
