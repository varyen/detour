// Правила приложений на РАБОТАЮЩЕМ sing-box без прав администратора: служба с
// DETOUR_DEV_NO_TUN=1 (вход dev-in 127.0.0.1:18282 вместо TUN). Процесс,
// подключившийся ко входу по loopback, sing-box узнаёт так же, как из TUN.
//   $env:DETOUR_DEV_NO_TUN=1; detour-svc run --dev-http 127.0.0.1:18080
//   node client/scripts/e2e-apps.mjs <путь к sing-box.exe>
// «VPN-серверы» — два локальных SOCKS (:18181, :18182); куда ушло соединение,
// видно по их журналам. Ходит в интернет: example.com.
import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const API = "http://127.0.0.1:18080/cgi-bin/detour-api";
const SINGBOX = process.argv[2];
if (!SINGBOX) throw new Error("путь к sing-box обязателен");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const logs = {};
function upstream(port) {
  const dir = mkdtempSync(join(tmpdir(), `detour-up${port}-`));
  logs[port] = join(dir, "log.txt");
  writeFileSync(join(dir, "c.json"), JSON.stringify({
    log: { level: "info", output: logs[port] },
    inbounds: [{ type: "mixed", listen: "127.0.0.1", listen_port: port }],
    outbounds: [{ type: "direct", tag: "direct" }],
  }));
  return spawn(SINGBOX, ["run", "-c", join(dir, "c.json"), "-D", dir], { stdio: "ignore" });
}
const ups = [upstream(18181), upstream(18182)];
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
const seen = (port, host) => {
  try {
    return readFileSync(logs[port], "utf8").includes(host);
  } catch {
    return false;
  }
};
// Каждый раз новый поддомен — так по журналам видно именно это соединение.
let n = 0;
async function via(app = "curl") {
  const host = `t${Date.now()}${n++}.example.com`;
  const url = `http://${host}/`;
  try {
    if (app === "curl") {
      execFileSync("curl.exe", ["-s", "-o", "NUL", "--max-time", "8", "-x", "socks5h://127.0.0.1:18282", url]);
    } else {
      execFileSync("powershell.exe", ["-NoProfile", "-Command",
        `try { Invoke-WebRequest -UseBasicParsing -TimeoutSec 8 -Proxy http://127.0.0.1:18282 '${url}' | Out-Null } catch {}`]);
    }
  } catch {}
  await sleep(500);
  return [18181, 18182].filter((p) => seen(p, host)).join(",") || "direct";
}

try {
  const socks = (port) => ({ type: "socks", server: "127.0.0.1", server_port: port, version: "5" });
  await call("profile_save", { body: { id: "main", name: "Основной", outbound: socks(18181) } });
  await call("profile_save", { body: { id: "spare", name: "Запасной", outbound: socks(18182) } });
  await call("settings", { body: { routing_mode: "all-except" } });
  await call("app_rules", { body: "" });
  let r = await call("profile_activate", { params: { name: "main" } });
  expect("sing-box запущен без TUN", r?.ok === true, r?.error);

  const g = await call("app_rules");
  expect("app_rules: поддерживается, по процессу", g.supported === true && g.by === "process", JSON.stringify(g));
  const list = await call("apps_list");
  const names = (list.apps ?? []).map((a) => a.match.toLowerCase());
  expect("apps_list видит запущенные программы", list.source === "running" && names.includes("node.exe"), `${names.length} шт.`);

  expect("без правил curl идёт через основной VPN", (await via("curl")) === "18181");

  r = await call("app_rules", { body: "curl.exe|vpn|spare|curl\n" });
  expect("правило «curl → spare» применено", r?.ok === true, r?.error);
  expect("curl идёт через spare", (await via("curl")) === "18182");
  expect("PowerShell — по-прежнему через основной", (await via("ps")) === "18181");

  r = await call("app_rules", { body: "CURL.EXE|direct||\n" });
  expect("регистр имени не важен; «мимо VPN» применено", r?.ok === true, r?.error);
  expect("curl идёт напрямую", (await via("curl")) === "direct");

  r = await call("allvpn_on");
  expect("«Всё через VPN» отменяет «мимо VPN»", (await via("curl")) === "18181");
  await call("allvpn_off");

  const curlDir = execFileSync("where.exe", ["curl.exe"]).toString().split(/\r?\n/)[0].replace(/\\[^\\]+$/, "");
  r = await call("app_rules", { body: `${curlDir}|vpn|spare|папка\n` });
  expect("правило по папке применено", r?.ok === true, r?.error);
  expect("curl из этой папки идёт через spare", (await via("curl")) === "18182");

  r = await call("app_rules", { body: "curl.exe|vpn|gone|\n" });
  expect("пропавшая цель — общий путь", r?.ok === true && (await via("curl")) === "18181", r?.error);

  const cfg = await call("singbox_config");
  expect("в конфиге нет мусора от правил", !JSON.stringify(cfg).includes("gone"));
} finally {
  await call("app_rules", { body: "" });
  await call("singbox_stop");
  ups.forEach((p) => p.kill());
}
console.log(failures ? `\nошибок: ${failures}` : "\nвсё зелёное");
process.exit(failures ? 1 : 0);
