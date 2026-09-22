// Проверки профилей, пинг, RU-подсети и обновление sing-box против живой службы:
//   detour-svc run --dev-http 127.0.0.1:18080   (DETOUR_DATA — пустой каталог, sing-box в <data>/bin)
//   node client/scripts/e2e-health.mjs <путь к sing-box.exe>
// «VPN-сервер» — локальный SOCKS на отдельном sing-box (127.0.0.1:18181).
// RU-подсети и обновление ходят в интернет (GitHub).
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const API = "http://127.0.0.1:18080/cgi-bin/detour-api";
const SINGBOX = process.argv[2];
if (!SINGBOX) throw new Error("путь к sing-box обязателен");

const dir = mkdtempSync(join(tmpdir(), "detour-socks-"));
writeFileSync(
  join(dir, "c.json"),
  JSON.stringify({
    log: { level: "error" },
    inbounds: [{ type: "mixed", listen: "127.0.0.1", listen_port: 18181 }],
    outbounds: [{ type: "direct", tag: "direct" }],
  }),
);
const upstream = spawn(SINGBOX, ["run", "-c", join(dir, "c.json"), "-D", dir], { stdio: "ignore" });
await new Promise((r) => setTimeout(r, 1500));

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

const socks = (port) => ({ type: "socks", server: "127.0.0.1", server_port: port, version: "5" });
await call("profile_save", { body: { id: "local_ok", name: "Локальный SOCKS", outbound: socks(18181) } });
await call("profile_save", { body: { id: "local_dead", name: "Мёртвый", outbound: socks(18199) } });
await call("profile_save", { body: { id: "broken", name: "Битый", outbound: { type: "vless", server: "vpn.example.com", server_port: 443, uuid: "not-a-uuid", flow: "no-such-flow" } } });
await call("profile_save", { body: { id: "pingable", name: "1.1.1.1", outbound: { type: "trojan", server: "1.1.1.1", server_port: 443, password: "x", tls: { enabled: true } } } });

await call("health_urls", { body: "Google|https://www.google.com/generate_204\nCloudflare|https://cp.cloudflare.com/generate_204\n" });
await call("health_config", { body: { speed: 1, speed_bytes: 1000000 } });

let t = Date.now();
let r = await call("health_check", { params: { id: "local_ok" } });
expect("локальный SOCKS проходит обе цели", r?.result?.ok === true && r.result.delays.length === 2 && r.result.delays.every((d) => d > 0), `${JSON.stringify(r?.result)} за ${Date.now() - t} мс`);
expect("скорость померена", r?.result?.dl > 0, `dl=${r?.result?.dl} кбит/с`);

r = await call("health_check", { params: { id: "local_dead" } });
expect("мёртвый профиль — ok=false, остальные цели не запрашивались", r?.result?.ok === false && r.result.delays.every((d) => d === -1), JSON.stringify(r?.result));

r = await call("health_check", { params: { id: "broken" } });
expect("битый конфиг — вердикт down, без падения", r?.ok === true && r?.result?.ok === false, JSON.stringify(r?.result));

t = Date.now();
r = await call("health_check", {});
expect("полная проверка запущена", r?.started === true);
let st;
for (let i = 0; i < 40; i++) {
  await new Promise((res) => setTimeout(res, 1000));
  st = await call("health_status");
  if (Object.keys(st?.results ?? {}).length >= 4 && Object.values(st.results).every((v) => v.ts * 1000 >= t - 1000)) break;
}
const res = st?.results ?? {};
expect("полная проверка: битый отсечён, остальные проверены", res.local_ok?.ok === true && res.local_dead?.ok === false && res.broken?.ok === false && "pingable" in res, Object.entries(res).map(([k, v]) => `${k}:${v.ok}`).join(" "));
expect("health_status: цели и настройки", st?.urls?.length === 2 && st?.speed === true && st?.speed_bytes === 1000000 && st?.supported === true);

r = await call("ping_check", { params: { id: "pingable" } });
expect("пинг 1.1.1.1", r?.ok === true && r?.rtt > 0, `${r?.method} ${r?.rtt} мс`);
r = await call("ping_check", { params: { id: "local_dead" } });
expect("пинг мёртвого локального порта", r?.id === "local_dead", `${r?.method} ok=${r?.ok}`);
const ps = await call("ping_status");
expect("ping_status хранит результаты", Object.keys(ps?.results ?? {}).length >= 2);

let rl = await call("rulist_status");
expect("rulist: по умолчанию включён, maxmind", rl?.supported === true && rl?.enabled === true && rl?.source === "maxmind", JSON.stringify({ c: rl?.count, e: rl?.error }));
t = Date.now();
rl = await call("rulist_update", { body: "" });
expect("rulist: скачано ≥1000 подсетей", rl?.count >= 1000 && !rl?.error, `${rl?.count} за ${Date.now() - t} мс ${rl?.error ?? ""}`);
await call("rulist_exclude", { body: "5.3.0.0/16\n" });
rl = await call("rulist_status");
expect("исключения учтены", rl?.excluded === 1);
await call("profile_activate", { params: { name: "local_ok" } });
await call("settings", { body: { routing_mode: "all-except" } });
const cfg = await call("singbox_config");
const tags = (cfg?.route?.rule_set ?? []).map((s) => s.tag);
expect("RU-подсети в конфиге", tags.includes("ru-subnets"), tags.join(","));
rl = await call("rulist_set", { body: { enabled: false } });
const cfg2 = await call("singbox_config");
expect("выключили — из конфига ушли", rl?.enabled === false && !(cfg2?.route?.rule_set ?? []).some((s) => s.tag === "ru-subnets"));

let b = await call("bins_update_check", { body: "" });
expect("проверка обновления sing-box", b?.current && b?.available && b?.asset?.endsWith(".zip"), `current=${b?.current} available=${b?.available} upstream=${b?.upstream} upstream_newer=${b?.upstream_newer}`);
const ov = await call("updates_overview");
expect("updates_overview", ov?.singbox?.current_version === b?.current && typeof ov?.singbox?.update_available === "boolean");
r = await call("bins_update_apply", { body: "" });
expect("установка запущена", r?.status === "started");
let log;
for (let i = 0; i < 120; i++) {
  await new Promise((res) => setTimeout(res, 2000));
  log = await call("apply_log");
  if (log?.done) break;
}
expect("sing-box скачан и поставлен", log?.done === true && log?.rc === "0", (log?.log ?? "").trim().split("\n").slice(-2).join(" | "));

const exited = new Promise((r) => upstream.once("exit", r));
upstream.kill();
await exited;
try {
  rmSync(dir, { recursive: true, force: true });
} catch {
  /* Windows отпускает каталог не сразу — временный мусор не повод валить проверку. */
}
console.log(failures ? `\n${failures} FAIL` : "\nвсё зелёное");
process.exit(failures ? 1 : 0);
