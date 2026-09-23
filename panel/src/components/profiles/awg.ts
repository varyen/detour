/* AmneziaWG — WireGuard с обфускацией. sing-box его не умеет, поэтому на
   роутере такой профиль обслуживает сайдкар mihomo (detour-awg), а в профиле
   параметры обфускации лежат в outbound.amnezia с именами ключей mihomo:
   jc, jmin, …, h1–h4, i1–i5, j1–j3, itime, version, header-protection-key…

   В форме и в .conf они же пишутся так, как их пишет клиент Amnezia:
   «Jc = 4», «H1 = 100000-200000», «HeaderProtectionKey = …». Версии:
     1.0 — Jc/Jmin/Jmax, S1/S2, H1–H4 (числа);
     1.5 — плюс S3/S4, I1–I5, J1–J3, Itime;
     2.0 — H1–H4 диапазонами, J1–J3 и Itime убраны;
     3.x — HeaderProtectionKey, ContentPaddingAddition и пр., mihomo нужен version: 3. */

export function isWgType(t: string): boolean {
  return t === "wireguard" || t === "amneziawg";
}

/** Имя в .conf → ключ mihomo. Порядок — порядок вывода. */
const CONF_TO_KEY: [string, string][] = [
  ["Jc", "jc"],
  ["Jmin", "jmin"],
  ["Jmax", "jmax"],
  ["S1", "s1"],
  ["S2", "s2"],
  ["S3", "s3"],
  ["S4", "s4"],
  ["H1", "h1"],
  ["H2", "h2"],
  ["H3", "h3"],
  ["H4", "h4"],
  ["I1", "i1"],
  ["I2", "i2"],
  ["I3", "i3"],
  ["I4", "i4"],
  ["I5", "i5"],
  ["J1", "j1"],
  ["J2", "j2"],
  ["J3", "j3"],
  ["Itime", "itime"],
  ["HeaderProtectionKey", "header-protection-key"],
  ["ContentPaddingAddition", "content-padding-addition"],
  ["RekeyAfterTime", "rekey-after-time"],
  ["RekeyTimeout", "rekey-timeout"],
  ["RejectAfterTime", "reject-after-time"],
  ["KeepaliveTimeout", "keepalive-timeout"],
  ["MaxHandshakeAttempts", "max-handshake-attempts"],
  ["RandomTrailers", "random-trailers"],
  ["DisableCookies", "disable-cookies"],
];
const BY_CONF = new Map(CONF_TO_KEY.map(([c, k]) => [c.toLowerCase(), k]));
const BY_KEY = new Map(CONF_TO_KEY.map(([c, k]) => [k, c]));
const NUMERIC = new Set(["jc", "jmin", "jmax", "s1", "s2", "s3", "s4", "itime"]);
const BOOL = new Set(["random-trailers", "disable-cookies"]);
/** Признак AWG 3: без version: 3 mihomo взял бы старую реализацию. */
const V3_KEYS = [
  "header-protection-key",
  "content-padding-addition",
  "rekey-after-time",
  "rekey-timeout",
  "reject-after-time",
  "keepalive-timeout",
  "max-handshake-attempts",
  "random-trailers",
  "disable-cookies",
];

/** Ключ .conf (в любом регистре, а также имя mihomo) → ключ mihomo. */
export function awgKey(name: string): string | undefined {
  const n = name.trim();
  if (BY_KEY.has(n.toLowerCase())) return n.toLowerCase();
  return BY_CONF.get(n.toLowerCase()) ?? (n.toLowerCase() === "version" ? "version" : undefined);
}

function awgValue(key: string, raw: string): string | number | boolean | undefined {
  const v = raw.trim();
  if (!v) return undefined;
  if (NUMERIC.has(key) || key === "version") {
    const n = Number(v);
    return Number.isFinite(n) ? n : undefined;
  }
  if (BOOL.has(key)) return /^(1|true|yes|on)$/i.test(v);
  return v;
}

/** Строки «Ключ = значение» → outbound.amnezia. Прочие строки игнорируются. */
export function awgFromText(text: string): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const line of text.split(/\r?\n/)) {
    const m = line.match(/^\s*([A-Za-z0-9_-]+)\s*[=:]\s*(.*?)\s*$/);
    if (!m) continue;
    const key = awgKey(m[1]);
    if (!key) continue;
    const v = awgValue(key, m[2]);
    if (v !== undefined) out[key] = v;
  }
  if (out.version === undefined && V3_KEYS.some((k) => k in out)) out.version = 3;
  return out;
}

/** outbound.amnezia → строки «Ключ = значение» для формы и .conf. */
export function awgToText(a: Record<string, unknown>): string {
  const lines: string[] = [];
  for (const [conf, key] of CONF_TO_KEY) {
    const v = a[key];
    if (v === undefined || v === null || v === "" || v === false) continue;
    lines.push(`${conf} = ${v === true ? "true" : String(v)}`);
  }
  return lines.join("\n");
}

export interface ConfPeer {
  privateKey: string;
  addresses: string[];
  mtu: string;
  publicKey: string;
  presharedKey: string;
  allowedIps: string[];
  host: string;
  port: string;
  keepalive: string;
  awg: string;
}

function splitList(v: string): string[] {
  return v
    .split(/[\s,]+/)
    .map((x) => x.trim())
    .filter(Boolean);
}

/** Разбор wg-quick/Amnezia .conf. null — если это не конфиг WireGuard. */
export function parseConf(text: string): ConfPeer | null {
  if (!/\[Interface\]/i.test(text) || !/\[Peer\]/i.test(text)) return null;
  const r: ConfPeer = {
    privateKey: "",
    addresses: [],
    mtu: "",
    publicKey: "",
    presharedKey: "",
    allowedIps: [],
    host: "",
    port: "",
    keepalive: "",
    awg: "",
  };
  const awg: string[] = [];
  let section = "";
  let peers = 0;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/\s+#.*$/, "").trim();
    if (!line || line.startsWith("#") || line.startsWith(";")) continue;
    const sec = line.match(/^\[(\w+)\]$/);
    if (sec) {
      section = sec[1].toLowerCase();
      if (section === "peer") peers += 1;
      continue;
    }
    /* второй и дальше пиры не поддерживаем: у профиля один сервер */
    if (section === "peer" && peers > 1) continue;
    const eq = line.indexOf("=");
    if (eq < 0) continue;
    const k = line.slice(0, eq).trim().toLowerCase();
    const v = line.slice(eq + 1).trim();
    if (section === "interface") {
      if (k === "privatekey") r.privateKey = v;
      else if (k === "address") r.addresses.push(...splitList(v));
      else if (k === "mtu") r.mtu = v;
      else if (awgKey(k)) awg.push(`${line.slice(0, eq).trim()} = ${v}`);
    } else if (section === "peer") {
      if (k === "publickey") r.publicKey = v;
      else if (k === "presharedkey") r.presharedKey = v;
      else if (k === "allowedips") r.allowedIps.push(...splitList(v));
      else if (k === "persistentkeepalive") r.keepalive = v;
      else if (k === "endpoint") {
        const ep = v.match(/^\[([^\]]+)\]:(\d+)$/) || v.match(/^([^:]+):(\d+)$/);
        if (ep) {
          r.host = ep[1];
          r.port = ep[2];
        } else r.host = v;
      }
    }
  }
  if (!r.privateKey || !r.publicKey) return null;
  r.awg = awgToText(awgFromText(awg.join("\n")));
  return r;
}

function b64urlBytes(s: string): Uint8Array<ArrayBuffer> {
  let t = s.replace(/-/g, "+").replace(/_/g, "/").replace(/\s+/g, "");
  while (t.length % 4) t += "=";
  return Uint8Array.from(atob(t), (c) => c.charCodeAt(0));
}

async function inflate(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const ds = new DecompressionStream("deflate");
  const stream = new Blob([bytes]).stream().pipeThrough(ds);
  return await new Response(stream).text();
}

/** Ищет в разобранном JSON (включая вложенные JSON-строки) текст .conf. */
function findConf(v: unknown, depth = 0): string | null {
  if (depth > 6) return null;
  if (typeof v === "string") {
    const t = v.trim();
    /* сначала — вложенный JSON: в нём .conf тоже «виден», но с экранированными \n */
    if (t.startsWith("{")) {
      try {
        return findConf(JSON.parse(t), depth + 1);
      } catch {
        /* не JSON — проверяем как текст */
      }
    }
    return /\[Interface\]/i.test(v) && /\[Peer\]/i.test(v) ? v : null;
  }
  if (Array.isArray(v)) {
    for (const x of v) {
      const r = findConf(x, depth + 1);
      if (r) return r;
    }
    return null;
  }
  if (v && typeof v === "object") {
    for (const x of Object.values(v as Record<string, unknown>)) {
      const r = findConf(x, depth + 1);
      if (r) return r;
    }
  }
  return null;
}

/**
 * Ключ Amnezia «vpn://…» → текст .conf. Формат ключа: base64url от qCompress
 * (4 байта длины + zlib) JSON-а, внутри которого конфиг AWG лежит строкой,
 * иногда ещё одним слоем JSON. Старые ключи бывают несжатыми — пробуем оба.
 */
export async function confFromAmneziaKey(raw: string): Promise<string | null> {
  const body = raw.trim().replace(/^vpn:\/\//i, "");
  let bytes: Uint8Array<ArrayBuffer>;
  try {
    bytes = b64urlBytes(body);
  } catch {
    return null;
  }
  const candidates: (() => Promise<string>)[] = [
    () => inflate(bytes.slice(4)),
    () => inflate(bytes),
    async () => new TextDecoder().decode(bytes),
  ];
  for (const get of candidates) {
    try {
      const text = await get();
      const conf = findConf(JSON.parse(text));
      if (conf) return conf;
    } catch {
      /* следующий вариант */
    }
  }
  return null;
}
