import { requestJson, requestJsonTolerant } from "./client";
import type {
  CertStatus,
  OffloadStatus,
  PortmapEntry,
  PortmapStatus,
  PushConfig,
  SwapStatus,
  WanpinMode,
  WanpinStatus,
} from "./types";

export const services = {
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

  /* --- сертификат Let's Encrypt --- */
  certDetect: () => requestJsonTolerant<Record<string, unknown>>("cert_detect"),
  certStatus: () => requestJsonTolerant<CertStatus>("cert_status"),
  /** Выпуск уходит в фон: следить через certStatus. */
  certIssue: (domain: string, email: string) =>
    requestJson<{ ok: boolean; started: boolean }>("cert_issue", {
      body: { domain, email },
    }),

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
  importConfig: (envelope: unknown) =>
    requestJson<{ ok: boolean }>("panel_import_config", {
      body: envelope,
      timeoutMs: 120_000,
    }),
};
