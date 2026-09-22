import { writeFileSync } from "node:fs";

const port = process.argv[2] ?? "9223";
const out = process.argv[3];
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) throw new Error("нет страницы: " + JSON.stringify(targets));

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r, { once: true }));
let seq = 0;
const pending = new Map();
const logs = [];
ws.addEventListener("message", (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) {
    pending.get(m.id)(m);
    pending.delete(m.id);
  } else if (m.method === "Runtime.exceptionThrown") {
    logs.push("exception: " + (m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text));
  } else if (m.method === "Runtime.consoleAPICalled" && ["error", "warning"].includes(m.params.type)) {
    logs.push(m.params.type + ": " + m.params.args.map((a) => a.value ?? a.description).join(" "));
  }
});
const send = (method, params = {}) =>
  new Promise((r) => {
    const id = ++seq;
    pending.set(id, r);
    ws.send(JSON.stringify({ id, method, params }));
  });

await send("Runtime.enable");
await send("Page.enable");
await send("Page.reload", { ignoreCache: true });
await new Promise((r) => setTimeout(r, 4000));
const ev = await send("Runtime.evaluate", {
  returnByValue: true,
  expression: `JSON.stringify({
    url: location.href,
    tauri: "__TAURI_INTERNALS__" in window,
    subtitle: document.querySelector("header p, .subtitle, h1 + *")?.textContent?.trim(),
    h1: document.querySelector("h1")?.textContent?.trim(),
  })`,
});
console.log(ev.result.result.value);
console.log(logs.length ? logs.join("\n") : "консоль без ошибок");
if (out) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(shot.result.data, "base64"));
  console.log("screenshot:", out);
}
ws.close();
