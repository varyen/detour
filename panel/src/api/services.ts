import { requestJson, requestJsonTolerant } from "./client";
import type {
  CertDnsStatus,
  DdnsProvider,
  DdnsStatus,
  PubProto,
  RtunStatus,
  CertStatus,
  EngineConfig,
  EngineMode,
  OffloadStatus,
  PowerCheck,
  PortmapEntry,
  PortmapStatus,
  PushConfig,
  ServerClientMode,
  ServerSession,
  ServerSettingsPatch,
  ServerStatus,
  SwapStatus,
  WanpinMode,
  WanpinStatus,
} from "./types";

export const services = {
  /* --- полная остановка и удаление панели (фоном, ход — в apply_log) --- */
  /** Что из Detour сейчас живо на роутере — процессы, правила, расписание. */
  powerCheck: () => requestJson<PowerCheck>("power_check", { timeoutMs: 30_000 }),
  powerOff: () => requestJson<{ ok: boolean }>("power_off", { method: "POST" }),
  powerOn: () => requestJson<{ ok: boolean }>("power_on", { method: "POST" }),
  selfUninstall: (keepEngines: boolean) =>
    requestJson<{ ok: boolean }>("self_uninstall", {
      body: { confirm: "uninstall", keep_engines: keepEngines },
    }),

  /* --- проброс сервисов наружу --- */
  portmapStatus: () => requestJsonTolerant<PortmapStatus>("portmap_status"),
  portmapSave: (e: PortmapEntry) =>
    requestJson<{ ok: boolean }>("portmap_save", { body: e }),
  portmapAuth: (id: string, user: string, password: string) =>
    requestJson<{ ok: boolean }>("portmap_auth", { body: { id, user, password } }),
  portmapToggle: (id: string, enabled: boolean) =>
    requestJson<{ ok: boolean }>("portmap_toggle", { body: { id, enabled } }),
  portmapDelete: (id: string) =>
    requestJson<{ ok: boolean }>("portmap_delete", { body: { id } }),
  portmapApply: () =>
    requestJson<{ ok: boolean }>("portmap_apply", { method: "POST", timeoutMs: 90_000 }),
  portmapCheck: (id: string) =>
    requestJson<{ ok: boolean }>("portmap_check", { params: { id }, timeoutMs: 45_000 }),
  /** Единственное место, где панель ходит к стороннему сервису (WAN-IP и порт). */
  portmapCheckExternal: (id: string) =>
    requestJson<{ ok: boolean; reachable?: boolean; detail?: string }>(
      "portmap_check_external",
      { body: { id }, timeoutMs: 60_000 },
    ),

  /* --- свой VPN-сервер (AmneziaWG) --- */
  serverStatus: () => requestJsonTolerant<ServerStatus>("server_status"),
  /** Ставит kmod-amneziawg из фида прошивки; отсоединено, следить через applyLog. */
  serverInstall: () => requestJson<{ ok: boolean }>("server_install", { method: "POST" }),
  serverSet: (patch: ServerSettingsPatch) =>
    requestJson<{ ok: boolean }>("server_set", { body: patch, timeoutMs: 120_000 }),
  /** owner — логин админа, за которым закрепить клиента (только главный админ). */
  serverClientAdd: (name: string, mode: ServerClientMode, owner?: string) =>
    requestJson<{ ok: boolean; id: string }>("server_client_add", {
      body: { name, mode, ...(owner ? { owner } : {}) },
      timeoutMs: 120_000,
    }),
  serverClientSet: (
    id: string,
    patch: { name?: string; mode?: ServerClientMode; enabled?: boolean; route?: string; owner?: string },
  ) => requestJson<{ ok: boolean }>("server_client_set", { body: { id, ...patch }, timeoutMs: 120_000 }),
  serverClientDel: (id: string) =>
    requestJson<{ ok: boolean }>("server_client_del", { body: { id } }),
  /** Конфиг несёт приватный ключ клиента — только POST. */
  serverClientConf: (id: string) =>
    requestJson<{ ok: boolean; conf: string }>("server_client_conf", { body: { id } }),
  serverClientLink: (id: string) =>
    requestJson<{ ok: boolean; link: string; tls_link?: string; ws_link?: string; xhttp_link?: string; xhttp_rot_link?: string }>("server_client_link", { body: { id } }),
  /** URL подписки — в нём секрет клиента, поэтому тоже POST. */
  serverClientSub: (id: string) =>
    requestJson<{ ok: boolean; url: string }>("server_client_sub", { body: { id } }),
  serverClientSubReset: (id: string) =>
    requestJson<{ ok: boolean }>("server_client_sub_reset", { body: { id } }),
  serverPubAdd: (id: string, proto: PubProto, port: number, note: string) =>
    requestJson<{ ok: boolean; id: string }>("server_pub_add", {
      body: { id, proto, port, note },
      timeoutMs: 60_000,
    }),
  serverPubSet: (pid: string, patch: { enabled?: boolean; note?: string }) =>
    requestJson<{ ok: boolean }>("server_pub_set", { body: { pid, ...patch }, timeoutMs: 60_000 }),
  serverPubDel: (pid: string) =>
    requestJson<{ ok: boolean }>("server_pub_del", { body: { pid }, timeoutMs: 60_000 }),

  /* --- встречный канал (роутер за CGNAT ↔ свой VPN-сервер) --- */
  rtunStatus: () => requestJsonTolerant<RtunStatus>("rtun_status"),
  /** Ставит только AmneziaWG; ход — через diag.applyLog. */
  rtunInstall: () => requestJson<{ ok: boolean }>("rtun_install", { method: "POST" }),
  rtunImport: (conf: string) =>
    requestJson<{ ok: boolean }>("rtun_import", { body: { conf }, timeoutMs: 60_000 }),
  rtunSet: (enabled: boolean) =>
    requestJson<{ ok: boolean }>("rtun_set", { body: { enabled }, timeoutMs: 60_000 }),
  rtunMapAdd: (m: { proto: PubProto; port: number; ip: string; dport: number; note: string }) =>
    requestJson<{ ok: boolean; id: string }>("rtun_map_add", { body: m, timeoutMs: 60_000 }),
  rtunMapSet: (id: string, patch: { enabled?: boolean; note?: string }) =>
    requestJson<{ ok: boolean }>("rtun_map_set", { body: { id, ...patch }, timeoutMs: 60_000 }),
  rtunMapDel: (id: string) =>
    requestJson<{ ok: boolean }>("rtun_map_del", { body: { id }, timeoutMs: 60_000 }),
  rtunForget: () => requestJson<{ ok: boolean }>("rtun_forget", { method: "POST", timeoutMs: 60_000 }),

  serverRegen: () =>
    requestJson<{ ok: boolean }>("server_regen", { method: "POST", timeoutMs: 120_000 }),
  serverSessions: (id?: string) =>
    requestJson<{ ok: boolean; sessions: ServerSession[] }>("server_sessions", {
      params: id ? { id } : undefined,
    }),

  /* --- сертификат Let's Encrypt --- */
  certDetect: () => requestJsonTolerant<Record<string, unknown>>("cert_detect"),
  certStatus: () => requestJsonTolerant<CertStatus>("cert_status"),
  /** Выпуск уходит в фон: следить через certStatus. */
  certIssue: (domain: string, email: string) =>
    requestJson<{ ok: boolean; started: boolean }>("cert_issue", {
      body: { domain, email },
    }),

  /* --- DNS-01: подтверждение владения через API DNS-провайдера ---
     Даёт wildcard (Let's Encrypt выдаёт его только так) и не требует, чтобы
     снаружи был доступен 80-й порт. */
  certDnsStatus: () => requestJsonTolerant<CertDnsStatus>("cert_dns_status"),
  certDnsSet: (provider: string, values: Record<string, string>) =>
    requestJson<{ ok: boolean; ready?: boolean }>("cert_dns_set", {
      body: { provider, values },
    }),
  /** Проверка ключа чтением списка зон — выпуск не тратится. */
  certDnsCheck: () =>
    requestJson<{ ok: boolean; http?: number; zones?: number }>("cert_dns_check", {
      method: "POST",
      timeoutMs: 30_000,
    }),
  certIssueDns: (domain: string, email: string, alts: string[]) =>
    requestJson<{ ok: boolean; started: boolean }>("cert_issue_dns", {
      body: { domain, email, alts },
    }),

  /* --- бесплатное динамическое имя (DuckDNS / deSEC) --- */
  ddnsStatus: () => requestJsonTolerant<DdnsStatus>("ddns_status"),
  /** Сохранение сразу отправляет адрес — ответ несёт результат этой отправки. */
  ddnsSet: (body: {
    provider: DdnsProvider;
    domain: string;
    token?: string;
    lan: boolean;
    wildcard: boolean;
    enabled: boolean;
  }) =>
    requestJson<{ ok: boolean; ip?: string }>("ddns_set", { body, timeoutMs: 60_000 }),
  ddnsUpdate: () =>
    requestJson<{ ok: boolean; ip?: string }>("ddns_update", { method: "POST", timeoutMs: 60_000 }),
  ddnsOff: () => requestJson<{ ok: boolean }>("ddns_off", { method: "POST" }),
  ddnsDelete: () => requestJson<{ ok: boolean }>("ddns_delete", { method: "POST" }),
  /** Выпуск сертификата на имя уходит в фон: следить через certStatus. */
  ddnsCert: (email: string) =>
    requestJson<{ ok: boolean; started: boolean }>("ddns_cert", { body: { email } }),

  /* --- уведомления в браузер --- */
  pushConfig: () => requestJsonTolerant<PushConfig>("push_config"),
  pushSubscribe: (sub: PushSubscriptionJSON) =>
    requestJson<{ ok: boolean }>("push_subscribe", { body: sub }),
  pushUnsubscribe: (endpoint: string) =>
    requestJson<{ ok: boolean }>("push_unsubscribe", { body: { endpoint } }),
  pushTest: () =>
    requestJson<{ ok: boolean; sent: number }>("push_test", {
      method: "POST",
      timeoutMs: 45_000,
    }),

  /* --- движок: sing-box / гибрид / mihomo --- */
  engineConfig: () => requestJsonTolerant<EngineConfig>("engine_config"),
  engineSet: (mode: EngineMode, torrentSingbox: boolean) =>
    requestJson<{ ok: boolean }>("engine_config", {
      body: { mode, torrent_singbox: torrentSingbox },
      timeoutMs: 120_000,
    }),

  /* --- аппаратный офлоад (возможность определяется не платформой) --- */
  offloadStatus: () => requestJsonTolerant<OffloadStatus>("offload_status"),
  offloadSet: (mode: "auto" | "notify" | "off") =>
    requestJson<OffloadStatus>("offload_set", { body: { mode } }),
  offloadKick: () =>
    requestJson<OffloadStatus>("offload_kick", { method: "POST", timeoutMs: 90_000 }),

  /* --- ответ тем же каналом, которым пришёл запрос --- */
  wanpinStatus: () => requestJsonTolerant<WanpinStatus>("wanpin_status"),
  wanpinSetMode: (mode: WanpinMode) =>
    requestJson<WanpinStatus>("wanpin_set", { body: { mode } }),
  wanpinSetPublic: (id: string) =>
    requestJson<WanpinStatus>("wanpin_set", { body: { public: id } }),

  /* --- файл подкачки (только Keenetic) --- */
  swapStatus: () => requestJsonTolerant<SwapStatus>("swap_status"),
  swapCreate: (sizeMb: number) =>
    requestJson<{ ok: boolean; started: boolean; size_mb: number }>("swap_create", {
      body: { size_mb: sizeMb },
    }),
  swapRemove: () =>
    requestJson<{ ok: boolean }>("swap_remove", { method: "POST", timeoutMs: 60_000 }),

  /* --- резервная копия настроек панели --- */
  exportConfig: () => requestJson<Record<string, unknown>>("panel_export_config"),
  /** Полная копия: профили, подписки, цепочки, все списки, роутерные файлы. */
  exportFull: () => requestJson<Record<string, unknown>>("backup_export", { timeoutMs: 60_000 }),
  /** Принимает и настройки (v1), и полную копию (v2), и копию из приложения. */
  importConfig: (envelope: unknown) =>
    requestJson<{ ok: boolean; warning?: string; skipped?: string[] }>("panel_import_config", {
      body: envelope,
      timeoutMs: 120_000,
    }),
};
