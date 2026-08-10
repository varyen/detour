/* Профиль на роутере — это файл вида
   {id, name, type, group, uri, routing_mode, outbound:{…sing-box outbound…}}.
   Здесь всё, что превращает его в поля формы и обратно, плюс разбор ссылок
   (vless://, trojan://, vmess://, ss://, hysteria2://, tuic://, socks5://) —
   самый частый способ завести профиль руками.

   Отдельно про uTLS: sing-box на outbound-ах hysteria2/tuic падает на КАЖДОМ
   соединении, если в tls есть utls («unsupported usage for uTLS»). Бэкенд
   вырезает его при рендере конфига, но записывать заведомо ядовитое поле в
   профиль всё равно незачем — форма для этих типов отпечаток не спрашивает. */

export type ProtoType =
  | "trojan"
  | "vless"
  | "vmess"
  | "shadowsocks"
  | "hysteria2"
  | "tuic"
  | "wireguard"
  | "socks"
  | "http";

export const PROTO_TYPES: { value: ProtoType; label: string }[] = [
  { value: "trojan", label: "Trojan" },
  { value: "vless", label: "VLESS" },
  { value: "vmess", label: "VMess" },
  { value: "shadowsocks", label: "Shadowsocks" },
  { value: "hysteria2", label: "Hysteria2" },
  { value: "tuic", label: "TUIC" },
  { value: "wireguard", label: "WireGuard" },
  { value: "socks", label: "SOCKS" },
  { value: "http", label: "HTTP-прокси" },
];

/** Типы, которым нельзя отдавать tls.utls — см. комментарий сверху. */
export const NO_UTLS: ProtoType[] = ["hysteria2", "tuic"];

export const SS_METHODS = [
  "2022-blake3-aes-128-gcm",
  "2022-blake3-aes-256-gcm",
  "2022-blake3-chacha20-poly1305",
  "aes-128-gcm",
  "aes-256-gcm",
  "chacha20-ietf-poly1305",
  "xchacha20-ietf-poly1305",
  "none",
];

export interface ProfileDraft {
  id: string;
  name: string;
  group: string;
  type: ProtoType;
  server: string;
  port: string;
  /** vless / vmess / tuic */
  uuid: string;
  /** trojan / shadowsocks / tuic / socks / http */
  password: string;
  username: string;
  method: string;
  flow: string;
  alterId: string;
  tls: boolean;
  sni: string;
  alpn: string;
  fingerprint: string;
  insecure: boolean;
  realityKey: string;
  realityShortId: string;
  transport: "" | "ws" | "grpc" | "http";
  path: string;
  host: string;
  serviceName: string;
  /** hysteria2 */
  obfsPassword: string;
  upMbps: string;
  downMbps: string;
  /** tuic */
  congestion: string;
  /** wireguard */
  privateKey: string;
  peerPublicKey: string;
  presharedKey: string;
  localAddress: string;
  allowedIps: string;
  mtu: string;
  reserved: string;
  routingMode: "" | "proxy-list" | "all-except";
  /** Исходная ссылка, если профиль заводили из неё. */
  uri: string;
}

export function emptyDraft(): ProfileDraft {
  return {
    id: "",
    name: "",
    group: "",
    type: "vless",
    server: "",
    port: "443",
    uuid: "",
    password: "",
    username: "",
    method: "aes-256-gcm",
    flow: "",
    alterId: "",
    tls: true,
    sni: "",
    alpn: "",
    fingerprint: "",
    insecure: false,
    realityKey: "",
    realityShortId: "",
    transport: "",
    path: "",
    host: "",
    serviceName: "",
    obfsPassword: "",
    upMbps: "",
    downMbps: "",
    congestion: "",
    privateKey: "",
    peerPublicKey: "",
    presharedKey: "",
    localAddress: "",
    allowedIps: "0.0.0.0/0",
    mtu: "",
    reserved: "",
    routingMode: "",
    uri: "",
  };
}

const TRANSLIT: Record<string, string> = {
  а: "a", б: "b", в: "v", г: "g", д: "d", е: "e", ё: "e", ж: "zh", з: "z",
  и: "i", й: "y", к: "k", л: "l", м: "m", н: "n", о: "o", п: "p", р: "r",
  с: "s", т: "t", у: "u", ф: "f", х: "h", ц: "c", ч: "ch", ш: "sh", щ: "sch",
  ъ: "", ы: "y", ь: "", э: "e", ю: "yu", я: "ya",
};

/** Ровно то, что делает с именем роутер: выкидывает всё лишнее, режет до 64. */
export function sanitizeId(id: string): string {
  return id.replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 64);
}

/** Имя файла профиля на роутере — только [a-z0-9_-], поэтому имя транслитерим. */
export function slugify(name: string): string {
  const lat = name
    .toLowerCase()
    .split("")
    .map((ch) => (ch in TRANSLIT ? TRANSLIT[ch] : ch))
    .join("");
  const id = lat
    .replace(/[^a-z0-9_-]/g, "_")
    .replace(/_+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 64);
  return id || `profile_${Date.now()}`;
}

function num(v: unknown): number | undefined {
  const n = Number(String(v ?? "").trim());
  return Number.isFinite(n) && n > 0 ? n : undefined;
}

function str(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return "";
}

function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === "object" && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : {};
}

function list(v: unknown): string[] {
  if (Array.isArray(v)) return v.map((x) => str(x)).filter(Boolean);
  const s = str(v).trim();
  return s ? s.split(/[\s,]+/).filter(Boolean) : [];
}

/** Тип профиля так же, как его выводит бэкенд: http+tls → https-proxy и т. д. */
export function inferType(outbound: Record<string, unknown>): string {
  const t = str(outbound.type);
  if (t === "http") return obj(outbound.tls).enabled ? "https-proxy" : "http-proxy";
  if (t === "socks") return `socks${str(outbound.version) || "5"}`;
  return t || "unknown";
}

/* ======================= форма → профиль ======================= */

export function outboundFromDraft(d: ProfileDraft): Record<string, unknown> {
  const o: Record<string, unknown> = { type: d.type, tag: "proxy" };
  if (d.type !== "wireguard") {
    o.server = d.server.trim();
    const p = num(d.port);
    if (p) o.server_port = p;
  }

  switch (d.type) {
    case "vless":
      o.uuid = d.uuid.trim();
      if (d.flow.trim()) o.flow = d.flow.trim();
      break;
    case "vmess":
      o.uuid = d.uuid.trim();
      o.security = "auto";
      o.alter_id = Number(d.alterId) || 0;
      break;
    case "trojan":
      o.password = d.password;
      break;
    case "shadowsocks":
      o.method = d.method;
      o.password = d.password;
      break;
    case "hysteria2":
      o.password = d.password;
      if (d.obfsPassword.trim()) {
        o.obfs = { type: "salamander", password: d.obfsPassword.trim() };
      }
      if (num(d.upMbps)) o.up_mbps = num(d.upMbps);
      if (num(d.downMbps)) o.down_mbps = num(d.downMbps);
      break;
    case "tuic":
      o.uuid = d.uuid.trim();
      o.password = d.password;
      if (d.congestion.trim()) o.congestion_control = d.congestion.trim();
      break;
    case "socks":
      o.version = "5";
      if (d.username.trim()) o.username = d.username.trim();
      if (d.password) o.password = d.password;
      break;
    case "http":
      if (d.username.trim()) o.username = d.username.trim();
      if (d.password) o.password = d.password;
      if (d.path.trim()) o.path = d.path.trim();
      break;
    case "wireguard": {
      o.server = d.server.trim();
      const p = num(d.port);
      if (p) o.server_port = p;
      o.private_key = d.privateKey.trim();
      o.peer_public_key = d.peerPublicKey.trim();
      if (d.presharedKey.trim()) o.pre_shared_key = d.presharedKey.trim();
      const addrs = list(d.localAddress);
      if (addrs.length) o.local_address = addrs;
      const allowed = list(d.allowedIps);
      if (allowed.length) o.allowed_ips = allowed;
      if (num(d.mtu)) o.mtu = num(d.mtu);
      const res = list(d.reserved)
        .map((x) => Number(x))
        .filter((x) => Number.isFinite(x));
      if (res.length === 3) o.reserved = res;
      break;
    }
  }

  /* TLS. hysteria2/tuic всегда шифрованы, но отпечаток utls им противопоказан. */
  const wantsTls = d.type === "hysteria2" || d.type === "tuic" ? true : d.tls;
  if (wantsTls && d.type !== "wireguard" && d.type !== "socks") {
    const tls: Record<string, unknown> = { enabled: true };
    if (d.sni.trim()) tls.server_name = d.sni.trim();
    const alpn = list(d.alpn);
    if (alpn.length) tls.alpn = alpn;
    if (d.insecure) tls.insecure = true;
    if (d.realityKey.trim()) {
      tls.reality = {
        enabled: true,
        public_key: d.realityKey.trim(),
        short_id: d.realityShortId.trim(),
      };
    }
    if (d.fingerprint.trim() && !NO_UTLS.includes(d.type)) {
      tls.utls = { enabled: true, fingerprint: d.fingerprint.trim() };
    }
    o.tls = tls;
  }

  /* Транспорт есть только у TCP-протоколов семейства v2ray. */
  if (
    d.transport &&
    (d.type === "vless" || d.type === "vmess" || d.type === "trojan")
  ) {
    const tr: Record<string, unknown> = { type: d.transport };
    if (d.transport === "ws") {
      if (d.path.trim()) tr.path = d.path.trim();
      if (d.host.trim()) tr.headers = { Host: d.host.trim() };
    } else if (d.transport === "grpc") {
      if (d.serviceName.trim()) tr.service_name = d.serviceName.trim();
    } else if (d.transport === "http") {
      if (d.path.trim()) tr.path = d.path.trim();
      if (d.host.trim()) tr.host = [d.host.trim()];
    }
    o.transport = tr;
  }

  for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k];
  return o;
}

export function profileFromDraft(d: ProfileDraft): Record<string, unknown> {
  const outbound = outboundFromDraft(d);
  const name = d.name.trim() || d.server.trim() || "Профиль";
  return {
    /* Идентификатор — имя файла на роутере. Заданный руками только чистим тем
       же правилом, что и роутер (регистр он сохраняет, поэтому и мы сохраняем:
       понизить его — значит создать второй файл вместо правки старого). */
    id: d.id.trim() ? sanitizeId(d.id) : slugify(name),
    name,
    type: inferType(outbound),
    group: d.group.trim(),
    uri: d.uri,
    routing_mode: d.routingMode,
    outbound,
  };
}

/* ======================= профиль → форма ======================= */

export function draftFromProfile(p: Record<string, unknown>): ProfileDraft {
  const d = emptyDraft();
  const o = obj(p.outbound);
  d.id = str(p.id);
  d.name = str(p.name);
  d.group = str(p.group);
  d.uri = str(p.uri);
  const rm = str(p.routing_mode);
  d.routingMode = rm === "proxy-list" || rm === "all-except" ? rm : "";

  const t = str(o.type);
  const known = PROTO_TYPES.some((x) => x.value === t);
  d.type = known ? (t as ProtoType) : "vless";

  d.server = str(o.server);
  d.port = str(o.server_port);
  d.uuid = str(o.uuid);
  d.password = str(o.password);
  d.username = str(o.username);
  if (str(o.method)) d.method = str(o.method);
  d.flow = str(o.flow);
  d.alterId = str(o.alter_id);
  d.congestion = str(o.congestion_control);
  if (num(o.up_mbps)) d.upMbps = str(o.up_mbps);
  if (num(o.down_mbps)) d.downMbps = str(o.down_mbps);
  d.obfsPassword = str(obj(o.obfs).password);

  const tls = obj(o.tls);
  d.tls = tls.enabled === true;
  d.sni = str(tls.server_name);
  d.alpn = list(tls.alpn).join(", ");
  d.insecure = tls.insecure === true;
  d.fingerprint = str(obj(tls.utls).fingerprint);
  const reality = obj(tls.reality);
  d.realityKey = str(reality.public_key);
  d.realityShortId = str(reality.short_id);

  const tr = obj(o.transport);
  const trType = str(tr.type);
  if (trType === "ws" || trType === "grpc" || trType === "http") {
    d.transport = trType;
    d.path = str(tr.path);
    d.serviceName = str(tr.service_name);
    d.host = str(obj(tr.headers).Host) || list(tr.host)[0] || "";
  } else if (d.type === "http") {
    d.path = str(o.path);
  }

  if (d.type === "wireguard") {
    d.privateKey = str(o.private_key);
    d.peerPublicKey = str(o.peer_public_key);
    d.presharedKey = str(o.pre_shared_key);
    d.localAddress = list(o.local_address).join("\n");
    d.allowedIps = list(o.allowed_ips).join("\n");
    d.mtu = str(o.mtu);
    d.reserved = list(o.reserved).join(",");
  }
  return d;
}

/* ======================= разбор ссылок ======================= */

function b64decode(raw: string): string {
  let t = raw.replace(/-/g, "+").replace(/_/g, "/").trim();
  while (t.length % 4) t += "=";
  const bin = atob(t);
  const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

interface UriParts {
  userinfo: string;
  host: string;
  port: string;
  params: Record<string, string>;
  label: string;
}

function splitUri(rest: string): UriParts {
  let body = rest;
  let label = "";
  const hash = body.lastIndexOf("#");
  if (hash >= 0) {
    label = safeDecode(body.slice(hash + 1));
    body = body.slice(0, hash);
  }
  const params: Record<string, string> = {};
  const q = body.indexOf("?");
  if (q >= 0) {
    for (const pair of body.slice(q + 1).split("&")) {
      if (!pair) continue;
      const eq = pair.indexOf("=");
      const k = safeDecode(eq < 0 ? pair : pair.slice(0, eq));
      params[k] = eq < 0 ? "" : safeDecode(pair.slice(eq + 1));
    }
    body = body.slice(0, q);
  }
  const at = body.lastIndexOf("@");
  const userinfo = at >= 0 ? body.slice(0, at) : "";
  let hostPort = at >= 0 ? body.slice(at + 1) : body;
  let host = hostPort;
  let port = "";
  if (hostPort.startsWith("[")) {
    const close = hostPort.indexOf("]");
    host = hostPort.slice(0, close + 1);
    hostPort = hostPort.slice(close + 1);
    if (hostPort.startsWith(":")) port = hostPort.slice(1);
  } else {
    const colon = hostPort.lastIndexOf(":");
    if (colon >= 0) {
      host = hostPort.slice(0, colon);
      port = hostPort.slice(colon + 1);
    }
  }
  return { userinfo, host, port: port.replace(/\/.*$/, ""), params, label };
}

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

function applyTlsParams(d: ProfileDraft, p: Record<string, string>) {
  const security = (p.security || "").toLowerCase();
  d.tls = security === "tls" || security === "reality" || security === "xtls";
  if (p.sni || p.peer) d.sni = p.sni || p.peer;
  if (p.alpn) d.alpn = p.alpn.split(",").join(", ");
  if (p.fp && !NO_UTLS.includes(d.type)) d.fingerprint = p.fp;
  if (p.allowInsecure === "1" || p.insecure === "1") d.insecure = true;
  if (security === "reality") {
    d.tls = true;
    d.realityKey = p.pbk || "";
    d.realityShortId = p.sid || "";
  }
}

function applyTransportParams(d: ProfileDraft, p: Record<string, string>) {
  const tr = (p.type || p.network || "tcp").toLowerCase();
  if (tr === "ws") {
    d.transport = "ws";
    d.path = p.path || "";
    d.host = p.host || "";
  } else if (tr === "grpc") {
    d.transport = "grpc";
    d.serviceName = p.serviceName || p.servicename || "";
  } else if (tr === "h2" || tr === "http") {
    d.transport = "http";
    d.path = p.path || "";
    d.host = p.host || "";
  }
}

/**
 * Разбирает одну ссылку в черновик профиля. Возвращает null, если схема не
 * распознана — вызывающий покажет это человеку, а не молча создаст пустышку.
 */
export function parseShareLink(raw: string): ProfileDraft | null {
  const line = raw.trim();
  const scheme = line.slice(0, line.indexOf("://")).toLowerCase();
  if (!scheme) return null;
  const rest = line.slice(scheme.length + 3);
  const d = emptyDraft();
  d.uri = line;

  if (scheme === "vmess") {
    let j: Record<string, unknown>;
    try {
      j = JSON.parse(b64decode(rest.split("#")[0])) as Record<string, unknown>;
    } catch {
      return null;
    }
    d.type = "vmess";
    d.name = str(j.ps) || str(j.remarks);
    d.server = str(j.add) || str(j.address);
    d.port = str(j.port) || "443";
    d.uuid = str(j.id);
    d.alterId = str(j.aid) || "0";
    d.tls = String(j.tls || "").toLowerCase() === "tls";
    d.sni = str(j.sni) || str(j.host);
    if (str(j.alpn)) d.alpn = str(j.alpn).split(",").join(", ");
    if (str(j.fp)) d.fingerprint = str(j.fp);
    const net = (str(j.net) || str(j.network) || "tcp").toLowerCase();
    applyTransportParams(d, {
      type: net,
      path: str(j.path),
      host: str(j.host),
      serviceName: str(j.path),
    });
    d.id = slugify(d.name || d.server);
    return d;
  }

  if (scheme === "ss") {
    /* Две живущие в природе формы: base64(method:password)@host:port и
       base64(method:password@host:port). */
    let body = rest;
    let label = "";
    const hash = body.lastIndexOf("#");
    if (hash >= 0) {
      label = safeDecode(body.slice(hash + 1));
      body = body.slice(0, hash);
    }
    const qi = body.indexOf("?");
    if (qi >= 0) body = body.slice(0, qi);
    if (!body.includes("@")) {
      try {
        body = b64decode(body);
      } catch {
        return null;
      }
    }
    const at = body.lastIndexOf("@");
    if (at < 0) return null;
    let userinfo = body.slice(0, at);
    if (!userinfo.includes(":")) {
      try {
        userinfo = b64decode(userinfo);
      } catch {
        /* оставляем как есть — ниже упадёт на проверке метода */
      }
    }
    const hostPort = body.slice(at + 1);
    const colon = hostPort.lastIndexOf(":");
    d.type = "shadowsocks";
    d.tls = false;
    d.server = colon >= 0 ? hostPort.slice(0, colon) : hostPort;
    d.port = colon >= 0 ? hostPort.slice(colon + 1) : "";
    const sep = userinfo.indexOf(":");
    d.method = sep >= 0 ? userinfo.slice(0, sep) : "aes-256-gcm";
    d.password = sep >= 0 ? userinfo.slice(sep + 1) : "";
    d.name = label;
    d.id = slugify(d.name || d.server);
    return d;
  }

  const parts = splitUri(rest);
  const p = parts.params;
  d.name = parts.label;
  d.server = parts.host;
  d.port = parts.port;

  switch (scheme) {
    case "vless":
      d.type = "vless";
      d.uuid = parts.userinfo;
      d.flow = p.flow || "";
      applyTlsParams(d, p);
      applyTransportParams(d, p);
      break;
    case "trojan":
      d.type = "trojan";
      d.password = safeDecode(parts.userinfo);
      d.tls = true;
      applyTlsParams(d, p);
      d.tls = true;
      applyTransportParams(d, p);
      break;
    case "hysteria2":
    case "hy2":
      d.type = "hysteria2";
      d.password = safeDecode(parts.userinfo);
      d.tls = true;
      d.sni = p.sni || p.peer || "";
      d.insecure = p.insecure === "1";
      d.obfsPassword = p["obfs-password"] || "";
      break;
    case "tuic":
      d.type = "tuic";
      {
        const colon = parts.userinfo.indexOf(":");
        d.uuid = colon >= 0 ? parts.userinfo.slice(0, colon) : parts.userinfo;
        d.password = colon >= 0 ? safeDecode(parts.userinfo.slice(colon + 1)) : "";
      }
      d.tls = true;
      d.sni = p.sni || "";
      d.alpn = (p.alpn || "").split(",").filter(Boolean).join(", ");
      d.congestion = p.congestion_control || "";
      d.insecure = p.allow_insecure === "1";
      break;
    case "socks":
    case "socks5":
      d.type = "socks";
      d.tls = false;
      {
        const colon = parts.userinfo.indexOf(":");
        d.username = colon >= 0 ? safeDecode(parts.userinfo.slice(0, colon)) : "";
        d.password = colon >= 0 ? safeDecode(parts.userinfo.slice(colon + 1)) : "";
      }
      break;
    case "http":
    case "https":
      d.type = "http";
      d.tls = scheme === "https";
      {
        const colon = parts.userinfo.indexOf(":");
        d.username = colon >= 0 ? safeDecode(parts.userinfo.slice(0, colon)) : "";
        d.password = colon >= 0 ? safeDecode(parts.userinfo.slice(colon + 1)) : "";
      }
      break;
    default:
      return null;
  }

  if (!d.port) d.port = "443";
  d.id = slugify(d.name || d.server);
  return d;
}
