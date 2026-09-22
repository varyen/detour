// Сценарии с РАБОТАЮЩИМ sing-box без прав администратора: служба запущена с
// DETOUR_DEV_NO_TUN=1 (вместо TUN — вход dev-in 127.0.0.1:18282 с теми же правилами).
//   $env:DETOUR_DEV_NO_TUN=1; detour-svc run --dev-http 127.0.0.1:18080
//   node client/scripts/e2e-live.mjs <путь к sing-box.exe>
// «VPN-серверы» — два локальных SOCKS (:18181, :18182); поставщик подписки — :18099.
// Ходит в интернет: speed.cloudflare.com, www.google.com, база DB-IP.
import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

const API = "http://127.0.0.1:18080/cgi-bin/detour-api";
const SINGBOX = process.argv[2];
if (!SINGBOX) throw new Error("путь к sing-box обязателен");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function upstream(port) {
  const dir = mkdtempSync(join(tmpdir(), `detour-up${port}-`));
  writeFileSync(join(dir, "c.json"), JSON.stringify({
    log: { level: "error" },
    inbounds: [{ type: "mixed", listen: "127.0.0.1", listen_port: port }],
    outbounds: [{ type: "direct", tag: "direct" }],
  }));
  return spawn(SINGBOX, ["run", "-c", join(dir, "c.json"), "-D", dir], { stdio: "ignore" });
}
const up1 = upstream(18181);
const up2 = upstream(18182);

let seenVia = [];
const feed = http.createServer((req, res) => {
  seenVia.push(req.socket.remoteAddress);
  res.writeHead(200, { "content-type": "text/plain" });
  res.end(Buffer.from("trojan://pw@vpn.example.com:443?sni=vpn.example.com#FEED\n").toString("base64"));
});
await new Promise((r) => feed.listen(18099, "127.0.0.1", r));
await sleep(1500);

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
const curl = (url, extra = []) => {
  try {
    execFileSync("curl.exe", ["-s", "-o", "NUL", "--max-time", "20", "-x", "socks5h://127.0.0.1:18282", ...extra, url]);
    return true;
  } catch {
    return false;
  }
};

const socks = (port) => ({ type: "socks", server: "127.0.0.1", server_port: port, version: "5" });
await call("profile_save", { body: { id: "main", name: "Основной", outbound: socks(18181) } });
await call("profile_save", { body: { id: "spare", name: "Запасной", outbound: socks(18182) } });
await call("profile_save", { body: { id: "cf", name: "1.1.1.1", outbound: { type: "trojan", server: "1.1.1.1", server_port: 443, password: "x", tls: { enabled: true } } } });
await call("domains", { body: "cloudflare.com\n" });
await call("health_urls", { body: "Google|https://www.google.com/generate_204\n" });
await call("health_config", { body: { speed: 0, auto_switch: 1 } });

let r = await call("profile_activate", { params: { name: "main" } });
let st = await call("status");
expect("sing-box запущен без TUN", r?.ok === true && st.singbox?.running === true, r?.error ?? `pid ${st.singbox?.pid}`);

// --- трафик по полосам ---
await call("traffic_counters");
await sleep(3500);
const vpnOk = curl("https://speed.cloudflare.com/__down?bytes=3000000", ["--limit-rate", "600k"]);
// Медленно: соединение должно попасть в несколько опросов, иначе полоса угадывается.
const directOk = curl("https://www.google.com/", ["--limit-rate", "20k"]);
await sleep(4000);
const tc = await call("traffic_counters");
expect("загрузки через dev-in прошли", vpnOk && directOk);
expect("счётчики: VPN и напрямую разнесены", tc?.supported === true && tc.bytes?.vpn > 2_000_000 && tc.bytes?.direct > 0 && tc.span > 0,
  `vpn=${tc?.bytes?.vpn} direct=${tc?.bytes?.direct} rx=${tc?.bytes?.rx} доли ${tc?.vpn}/${tc?.direct}/${tc?.bypass}`);

// --- подписка через VPN ---
await call("subscription_save_one", { body: { id: "viavpn", url: "http://127.0.0.1:18099/sub", group: "Feed" } });
r = await call("subscription_refresh_one", { params: { id: "viavpn" } });
const sub = (await call("subscriptions_list")).subscriptions?.find((s) => s.id === "viavpn");
expect("подписка пришла через VPN (fetch-in → proxy)", r?.ok === true && sub?.last_via === "vpn", r?.error ?? sub?.last_via);

// --- keepalive ---
r = await call("keepalive_check");
expect("keepalive активного профиля", r?.ok === true && r?.profile === "main" && r?.server === "127.0.0.1", `${r?.method} ${r?.rtt} мс`);

// --- страны узлов ---
r = await call("geo_scan", { body: "" });
const list = (await call("profiles_list")).profiles ?? [];
const cf = list.find((p) => p.id === "cf");
expect("geo: страна 1.1.1.1 найдена, атрибуция DB-IP", r?.known >= 1 && /^[A-Z]{2}$/.test(cf?.cc ?? "") && (await call("geo_status"))?.attribution_url === "https://db-ip.com", `cc=${cf?.cc} known=${r?.known}/${r?.count} ${r?.error ?? ""}`);

// --- автопереключение ---
r = await call("health_check", { params: { id: "spare" } });
expect("запасной проверен и рабочий", r?.result?.ok === true);
up1.kill();
console.log("     гашу сервер основного профиля, жду переключения (до 150 с)…");
let switched = null;
for (let i = 0; i < 75; i++) {
  await sleep(2000);
  const hs = await call("health_status");
  if (hs?.switch?.to) {
    switched = hs.switch;
    break;
  }
}
st = await call("status");
expect("автопереключение main → spare", switched?.from === "main" && switched?.to === "spare" && st.singbox?.active_profile === "spare" && st.singbox?.running === true, JSON.stringify(switched));

// --- ряд графика (первая точка — через две минуты после старта службы) ---
let ser;
for (let i = 0; i < 70; i++) {
  ser = await call("traffic_series", { params: { range: "minute" } });
  if ((ser?.points ?? []).length) break;
  await sleep(2000);
}
expect("минутный ряд копится", ser?.step === 60 && ser.points.length >= 1 && ser.points.some((p) => p.vpn > 0 || p.rx > 0), JSON.stringify(ser?.points?.[0]));

await call("singbox_stop");
st = await call("status");
expect("остановка", st.singbox?.running === false);
up2.kill();
feed.close();
console.log(failures ? `\n${failures} FAIL` : "\nвсё зелёное");
process.exit(failures ? 1 : 0);
