const base = process.argv[2] ?? "http://127.0.0.1:18080/cgi-bin/detour-api";

async function call(action, { params = {}, body } = {}) {
  const u = new URL(base);
  u.searchParams.set("action", action);
  for (const [k, v] of Object.entries(params)) u.searchParams.set(k, v);
  const init = body === undefined ? {} : { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body) };
  const res = await fetch(u, init);
  const text = await res.text();
  let v;
  try { v = JSON.parse(text); } catch { v = text; }
  return v;
}

let failures = 0;
function expect(label, cond, extra = "") {
  if (!cond) failures++;
  console.log(`${cond ? "ok  " : "FAIL"} ${label}${extra ? " — " + extra : ""}`);
}

// ok:true или ошибка только из-за прав на TUN (служба в консоли без админа).
const applied = (r) =>
  r?.ok === true || /sing-box завершился сразу|access is denied|отказано|elevat|permission|TUN в режиме разработки/i.test(r?.error ?? "");

const vless = { id: "nl_1", name: "🇳🇱 NL 1", group: "Example VPN", uri: "vless://x", outbound: { type: "vless", server: "vpn.example.com", server_port: 443, uuid: "00000000-0000-0000-0000-000000000000", tls: { enabled: true, server_name: "vpn.example.com", utls: { enabled: true, fingerprint: "chrome" } } } };
const hy2 = { id: "de_hy", name: "DE hy2", outbound: { type: "hysteria2", server: "vpn.example.com", server_port: 8443, password: "p", tls: { enabled: true, server_name: "vpn.example.com", utls: { enabled: true, fingerprint: "chrome" } } } };
const wg = { id: "wg_1", name: "WG", outbound: { type: "wireguard", server: "vpn.example.com", server_port: 51820, private_key: "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=", peer_public_key: "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=", local_address: ["10.0.0.2/32"], mtu: 1280 } };
const socks = { id: "px", name: "SOCKS", outbound: { type: "socks", server: "proxy.example.com", server_port: 1080, version: "5" } };

for (const p of [vless, hy2, wg, socks]) {
  const r = await call("profile_save", { body: p });
  expect(`profile_save ${p.id}`, r?.ok === true, JSON.stringify(r));
}
const list = await call("profiles_list");
expect("profiles_list: 4 профиля, name и type", list.profiles?.length === 4 && list.profiles.every((p) => p.name && p.type), list.profiles?.map((p) => `${p.id}:${p.type}`).join(" "));

const trunc = await call("profile_save", { body: { id: "nl_1", name: "x", group: "g" } });
expect("защита от потери параметров", trunc?.ok === false, trunc?.error);

let r = await call("profile_activate", { params: { name: "nl_1" } });
expect("profile_activate nl_1 (check прошёл)", applied(r), r?.error);
const cfg = await call("singbox_config");
expect("конфиг: tun + proxy + final direct", cfg?.inbounds?.[0]?.type === "tun" && cfg?.outbounds?.[0]?.tag === "proxy" && cfg?.route?.final === "direct");

r = await call("chain_save", { body: { id: "wg_hy", name: "WG→HY2", hops: ["wg_1", "de_hy"] } });
expect("chain_save", r?.ok === true, r?.error);
r = await call("chain_activate", { body: "wg_1,de_hy" });
expect("chain_activate wg→hy2 (endpoint + utls снят)", applied(r), r?.error);
const cfg2 = await call("singbox_config");
expect("wg — endpoint chain_1, hy2 — proxy без utls", cfg2?.endpoints?.[0]?.tag === "chain_1" && cfg2?.outbounds?.[0]?.detour === "chain_1" && !cfg2?.outbounds?.[0]?.tls?.utls, JSON.stringify(cfg2?.endpoints?.[0]?.peers));

r = await call("route_map", { body: "// === route:px ===\nexample.com\n*.example.org\n\n// === route:nl_1 ===\n// meta: via_chain=1\n*.example.net\n203.0.113.0/24\n\n// === route:gone ===\nlost.example.com\n" });
expect("route_map: socks (override_address), профиль вне цепочки, пропавшая цель", applied(r), r?.error);
r = await call("domains_save_restart", { body: "// VPN\nexample.io\n198.51.100.7\n" });
expect("domains_save_restart", applied(r), r?.error);
r = await call("whitelist_save_restart", { body: "ya.ru\n*.example.ru\n10.10.0.0/16\n" });
expect("whitelist_save_restart", applied(r), r?.error);
r = await call("egress_blocklist", { body: "192.0.2.1\nмусор\n" });
expect("egress_blocklist", applied(r), r?.error);

for (const mode of ["list", "all", "off"]) {
  r = await call("udp_vpn", { body: { mode } });
  expect(`udp_vpn ${mode}`, applied(r), r?.error);
}
r = await call("settings", { body: { routing_mode: "all-except" } });
expect("settings all-except", applied(r), r?.error);
r = await call("udp_vpn", { body: { mode: "all" } });
expect("all-except + udp all", applied(r), r?.error);
r = await call("settings", { body: { singbox_mode: "multi" } });
expect("multi отклонён", r?.ok === false, r?.error);

const cfg3 = await call("singbox_config");
const tags = (cfg3?.route?.rule_set ?? []).map((s) => s.tag).join(",");
expect("all-except: final proxy, rule-set whitelist(+dns)", cfg3?.route?.final === "proxy" && tags.includes("whitelist-dns"), tags);

r = await call("profile_delete", { params: { name: "de_hy" } });
expect("нельзя удалить профиль активной цепочки", r?.ok === false, r?.error);
r = await call("chain_delete", { body: { id: "wg_hy" } });
expect("нельзя удалить активную цепочку", r?.ok === false, r?.error);

const st = await call("status");
expect("status: windows, цепочка, режим", st.platform === "windows" && st.singbox?.active_chain?.join(",") === "wg_1,de_hy" && st.singbox?.routing_mode === "all-except", `sing-box ${st.binaries?.singbox_version}, running=${st.singbox?.running}`);

// --- обход DPI и защита от утечки ---
// Без winws2 рядом движок не поддерживается: проверяем, что это видно в ответах
// и что включение возвращает понятную ошибку, а не «ок» на пустом месте.
r = await call("zapret_domains", { body: "dpi.example.com\n5.5.5.0/24\n" });
expect("список доменов обхода сохранён", r?.ok === true, r?.error);
let bs = await call("bypass_status");
const hasWinws = bs?.zapret2_supported === true;
expect("bypass_status: режим, автозапуск, стратегия", bs?.mode === "off" && bs?.autostart === 0 && String(bs?.strategy).includes("--lua-desync="),
  `supported=${hasWinws} strategy=${String(bs?.strategy).slice(0, 40)}`);
r = await call("bypass_set", { params: { mode: "zapret" }, body: "" });
expect("режим zapret (tpws) отклонён", r?.ok === false, r?.error);
r = await call("bypass_set", { params: { mode: "zapret2" }, body: "" });
expect("включение обхода: " + (hasWinws ? "движок поднялся" : "честная ошибка без winws2"),
  hasWinws ? r?.ok === true : r?.ok === false, r?.error ?? r?.mode);
r = await call("bypass_autostart", { params: { on: "1" }, body: "" });
bs = await call("bypass_status");
expect("автозапуск обхода запомнен", r?.ok === true && bs?.autostart === 1, JSON.stringify(bs?.autostart));
const strat = await call("bypass_strategy");
expect("стратегия отдаётся голой строкой", typeof strat === "string" && strat.includes("--lua-desync="), String(strat).slice(0, 50));
r = await call("bypass_strategy", { body: "--filter-tcp=443 без десинхронизации" });
expect("стратегия без --lua-desync отклонена", r?.ok === false, r?.error);
r = await call("bypass_stop", { body: "" });
expect("обход остановлен", r?.ok === true, r?.error);
const ks = await call("killswitch_status");
expect("kill-switch: выключен и поддерживается", ks?.supported === true && ks?.enabled === false, JSON.stringify(ks));
r = await call("killswitch_set", { params: { on: "1" }, body: "" });
const ks2 = await call("killswitch_status");
expect("kill-switch включается", r?.ok === true && ks2?.enabled === true, JSON.stringify(ks2));
await call("killswitch_set", { params: { on: "0" }, body: "" });

const st2 = await call("status");
expect("status: движок обхода и его список", st2.zapret?.domains === 1 && st2.zapret?.ips === 1 && st2.binaries?.nfqws2_supported === hasWinws,
  `domains=${st2.zapret?.domains} ips=${st2.zapret?.ips} supported=${st2.binaries?.nfqws2_supported}`);

console.log(failures ? `\n${failures} FAIL` : "\nвсё зелёное");
process.exit(failures ? 1 : 0);
