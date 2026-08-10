import { requestJson, requestJsonTolerant } from "./client";
import type { HostsStatus, RoutingMode, RulistStatus, SingboxMode } from "./types";

/* Сохранение списков перестраивает конфиг и перезапускает sing-box с полным
   сбросом conntrack — это десятки секунд на роутере с большим whitelist. */
const LIST_SAVE_TIMEOUT = 180_000;

export const rules = {
  /* --- домены через VPN --- */
  domainsGet: () => requestJson<{ domains: string }>("domains", { timeoutMs: 45_000 }),
  domainsSave: (text: string) =>
    requestJson<{ ok: boolean }>("domains", { body: text, timeoutMs: 60_000 }),
  domainsSaveRestart: (text: string) =>
    requestJson<{ ok: boolean }>("domains_save_restart", {
      body: text,
      timeoutMs: LIST_SAVE_TIMEOUT,
    }),

  /* --- whitelist (режим «всё кроме») --- */
  whitelistGet: () => requestJson<{ whitelist: string }>("whitelist", { timeoutMs: 45_000 }),
  whitelistSave: (text: string) =>
    requestJson<{ ok: boolean }>("whitelist", { body: text, timeoutMs: 60_000 }),
  whitelistSaveRestart: (text: string) =>
    requestJson<{ ok: boolean }>("whitelist_save_restart", {
      body: text,
      timeoutMs: LIST_SAVE_TIMEOUT,
    }),

  /* --- домены DPI-обхода (общие для zapret и zapret2) --- */
  zapretDomainsGet: () => requestJson<{ domains: string }>("zapret_domains"),
  zapretDomainsSave: (text: string) =>
    requestJson<{ ok: boolean }>("zapret_domains", { body: text }),
  zapretDomainsSaveRestart: (text: string) =>
    requestJson<{ ok: boolean }>("zapret_domains_save_restart", {
      body: text,
      timeoutMs: LIST_SAVE_TIMEOUT,
    }),

  /* --- маршруты «домен → профиль/цепочка» --- */
  routeMapGet: () => requestJson<{ routemap: string }>("route_map"),
  routeMapSave: (text: string) =>
    requestJson<{ ok: boolean }>("route_map", {
      body: text,
      timeoutMs: LIST_SAVE_TIMEOUT,
    }),

  /* --- перехват трафика самого роутера через цель маршрута --- */
  selfInterceptGet: () =>
    requestJson<{
      targets: string[];
      full_targets: string[];
      eligible: string[];
      mode: SingboxMode;
    }>("self_intercept"),
  selfInterceptSet: (csv: string) =>
    requestJson<{ ok: boolean; targets: string[]; full_targets: string[] }>(
      "self_intercept",
      { body: csv, timeoutMs: LIST_SAVE_TIMEOUT },
    ),

  /* --- блок-лист исходящих IP --- */
  egressBlocklistGet: () => requestJson<{ list: string }>("egress_blocklist"),
  egressBlocklistSet: (text: string) =>
    requestJson<{ ok: boolean }>("egress_blocklist", {
      body: text,
      timeoutMs: 60_000,
    }),

  /* --- режимы маршрутизации --- */
  settingsGet: () =>
    requestJson<{ routing_mode?: RoutingMode; singbox_mode?: SingboxMode }>(
      "settings",
    ),
  settingsSet: (s: { routing_mode?: RoutingMode; singbox_mode?: SingboxMode }) =>
    requestJson<{ ok: boolean }>("settings", {
      body: s,
      timeoutMs: LIST_SAVE_TIMEOUT,
    }),

  /* --- российские подсети (управляемый список) --- */
  rulistStatus: () => requestJsonTolerant<RulistStatus>("rulist_status"),
  rulistUpdate: (source?: "maxmind" | "rir") =>
    requestJson<RulistStatus>("rulist_update", {
      body: source ? { source } : "",
      timeoutMs: 300_000,
    }),
  rulistSet: (s: { source?: "maxmind" | "rir"; auto?: boolean; enabled?: boolean }) =>
    requestJson<RulistStatus>("rulist_set", { body: s, timeoutMs: 120_000 }),
  rulistExcludeGet: () => requestJson<{ exclude: string }>("rulist_exclude"),
  rulistExcludeSet: (text: string) =>
    requestJson<{ ok: boolean }>("rulist_exclude", {
      body: text,
      timeoutMs: 120_000,
    }),

  /* --- приоритетный hosts + шифрованный DNS --- */
  hostsStatus: () => requestJsonTolerant<HostsStatus>("hosts_status"),
  hostsSet: (s: { url?: string; enabled?: boolean }) =>
    requestJson<HostsStatus>("hosts_set", { body: s, timeoutMs: 60_000 }),
  hostsRefresh: () =>
    requestJson<HostsStatus>("hosts_refresh", {
      method: "POST",
      timeoutMs: 300_000,
    }),
  hostsExclude: (enabled: boolean) =>
    requestJson<HostsStatus>("hosts_exclude", { body: { enabled } }),
  hostsGet: () => requestJson<{ hosts: string }>("hosts_get", { timeoutMs: 60_000 }),
  hostsUpload: (text: string) =>
    requestJson<HostsStatus>("hosts_upload", { body: text, timeoutMs: 120_000 }),
  hostsCustomGet: () => requestJson<{ custom: string }>("hosts_custom_get"),
  hostsCustomSave: (text: string) =>
    requestJson<HostsStatus>("hosts_custom_save", {
      body: text,
      timeoutMs: 60_000,
    }),
  hostsCustomToggle: (enabled: boolean) =>
    requestJson<HostsStatus>("hosts_custom_toggle", { body: { enabled } }),

  /** Только GL.iNet: режим встроенного шифрованного DNS. */
  secureDnsSet: (mode: string, list?: string) =>
    requestJson<{ ok: boolean }>("secure_dns_set", {
      body: { mode, list },
      timeoutMs: 60_000,
    }),
};
