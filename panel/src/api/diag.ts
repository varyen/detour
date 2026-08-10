import { requestJson, requestJsonTolerant } from "./client";
import type {
  ApplyLogResponse,
  HealthStatusResponse,
  PingResult,
  PingStatusResponse,
  UpdateChannelState,
  UpdatesOverview,
} from "./types";

export type LogName = "singbox" | "zapret" | "health" | "update" | "apply";

export const diag = {
  /* --- сервис sing-box --- */
  singboxConfigGet: () =>
    requestJson<Record<string, unknown>>("singbox_config", { timeoutMs: 45_000 }),
  singboxConfigSave: (cfg: string) =>
    requestJson<{ ok: boolean }>("singbox_config", {
      body: cfg,
      timeoutMs: 60_000,
    }),
  singboxStart: () => requestJson<{ ok: boolean }>("singbox_start", { method: "POST", timeoutMs: 90_000 }),
  singboxStop: () => requestJson<{ ok: boolean }>("singbox_stop", { method: "POST", timeoutMs: 60_000 }),
  singboxRestart: () => requestJson<{ ok: boolean }>("singbox_restart", { method: "POST", timeoutMs: 180_000 }),
  singboxEnable: () => requestJson<{ ok: boolean }>("singbox_enable", { method: "POST" }),
  singboxDisable: () => requestJson<{ ok: boolean }>("singbox_disable", { method: "POST" }),

  /* --- сервис zapret-tpws --- */
  zapretStart: () => requestJson<{ ok: boolean }>("zapret_start", { method: "POST", timeoutMs: 90_000 }),
  zapretStop: () => requestJson<{ ok: boolean }>("zapret_stop", { method: "POST", timeoutMs: 60_000 }),
  zapretRestart: () => requestJson<{ ok: boolean }>("zapret_restart", { method: "POST", timeoutMs: 120_000 }),
  zapretEnable: () => requestJson<{ ok: boolean }>("zapret_enable", { method: "POST" }),
  zapretDisable: () => requestJson<{ ok: boolean }>("zapret_disable", { method: "POST" }),
  zapretConfigGet: () => requestJson<{ args: string }>("zapret_config"),
  zapretConfigSave: (args: string) =>
    requestJson<{ ok: boolean }>("zapret_config", { body: args }),

  /* --- журналы --- */
  logsView: (name: LogName) =>
    requestJson<{ ok: boolean; name: string; path: string; log: string; missing?: boolean }>(
      "logs_view",
      { params: { name }, timeoutMs: 45_000 },
    ),
  logsClear: (name: LogName) =>
    requestJson<{ ok: boolean }>("logs_clear", { method: "POST", params: { name } }),
  logConfig: (s: { enabled?: boolean; singbox?: boolean }) =>
    requestJson<{ ok: boolean }>("log_config", {
      body: {
        enabled: s.enabled === undefined ? undefined : s.enabled ? 1 : 0,
        singbox: s.singbox === undefined ? undefined : s.singbox ? 1 : 0,
      },
    }),
  /** Живой хвост лога отсоединённой операции обновления. */
  applyLog: () => requestJsonTolerant<ApplyLogResponse>("apply_log"),

  /* --- пинги --- */
  pingStatus: () => requestJson<PingStatusResponse>("ping_status"),
  pingCheck: (id: string) =>
    requestJson<PingResult & { id: string }>("ping_check", {
      params: { id },
      timeoutMs: 30_000,
    }),

  /* --- функциональная проверка --- */
  healthStatus: () => requestJson<HealthStatusResponse>("health_status", { timeoutMs: 45_000 }),
  /**
   * С id — синхронная проверка одного профиля (5–10 с). Внешнее `ok` означает
   * лишь «проверка отработала»; прошёл ли профиль — это `result.ok`.
   */
  healthCheckOne: (id: string) =>
    requestJson<{
      ok: boolean;
      id: string;
      result?: {
        ok: boolean;
        ts: number;
        /** ICMP/handshake, мс; -1 — не измерено. */
        rtt: number;
        /** Скорость скачивания, кбит/с; -1 — не измерялась. */
        dl: number;
        delays: number[];
      };
    }>("health_check", {
      method: "POST",
      params: { id },
      timeoutMs: 90_000,
    }),
  /** Без id — отсоединённый обход всех профилей, результат читать из healthStatus. */
  healthCheckAll: () =>
    requestJson<{ ok: boolean; started: boolean }>("health_check", { method: "POST" }),
  healthUrlsGet: () => requestJson<{ list: string }>("health_urls"),
  healthUrlsSet: (text: string) =>
    requestJson<{ ok: boolean }>("health_urls", { body: text }),
  healthConfig: (s: {
    enabled?: boolean;
    auto_switch?: boolean;
    speed?: boolean;
    speed_bytes?: number;
  }) =>
    requestJson<{ ok: boolean }>("health_config", {
      body: {
        enabled: s.enabled === undefined ? undefined : s.enabled ? 1 : 0,
        auto_switch:
          s.auto_switch === undefined ? undefined : s.auto_switch ? 1 : 0,
        speed: s.speed === undefined ? undefined : s.speed ? 1 : 0,
        speed_bytes: s.speed_bytes,
      },
    }),

  keepaliveStatus: () => requestJsonTolerant<Record<string, unknown>>("keepalive_status"),
  keepaliveCheck: () =>
    requestJson<Record<string, unknown>>("keepalive_check", {
      method: "POST",
      timeoutMs: 120_000,
    }),

  /* --- обновления --- */
  updatesOverview: () => requestJsonTolerant<UpdatesOverview>("updates_overview"),
  panelUpdateStatus: () => requestJsonTolerant<UpdateChannelState>("panel_update_status"),
  panelUpdateCheck: () =>
    requestJson<UpdateChannelState>("panel_update_check", {
      method: "POST",
      timeoutMs: 90_000,
    }),
  /** Установка отсоединена: следить через applyLog. */
  panelUpdateApply: () =>
    requestJson<{ ok: boolean; status: string }>("panel_update_apply", { method: "POST" }),
  panelUpdateSig: (sig: string) =>
    requestJson<{ ok: boolean }>("panel_update_sig", { body: sig }),
  panelUpdateLocal: (ipk: Blob) =>
    requestJson<{ ok: boolean; status: string }>("panel_update_local", {
      body: ipk as unknown as string,
      timeoutMs: 300_000,
    }),
  autocheckStatus: () => requestJson<{ enabled: boolean }>("autocheck_status"),
  autocheckSet: (enabled: boolean) =>
    requestJson<{ ok: boolean }>("autocheck_set", { body: { enabled } }),

  binsStatus: () => requestJsonTolerant<UpdateChannelState>("bins_update_status"),
  binsCheck: () =>
    requestJson<UpdateChannelState>("bins_update_check", { method: "POST", timeoutMs: 120_000 }),
  binsApply: () =>
    requestJson<{ ok: boolean }>("bins_update_apply", { method: "POST" }),
  tpwsStatus: () => requestJsonTolerant<UpdateChannelState>("tpws_update_status"),
  tpwsCheck: () =>
    requestJson<UpdateChannelState>("tpws_update_check", { method: "POST", timeoutMs: 120_000 }),
  tpwsApply: () =>
    requestJson<{ ok: boolean }>("tpws_update_apply", { method: "POST" }),
  nfqws2Status: () => requestJsonTolerant<UpdateChannelState>("nfqws2_update_status"),
  nfqws2Check: () =>
    requestJson<UpdateChannelState>("nfqws2_update_check", { method: "POST", timeoutMs: 120_000 }),
  nfqws2Apply: () =>
    requestJson<{ ok: boolean }>("nfqws2_update_apply", { method: "POST" }),
};
