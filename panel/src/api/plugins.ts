import { requestJson } from "./client";

/* Плагины: отдельные пакеты detour-plugin-<id>. Реестр и вызовы — detour-plugin
   на роутере, установка и удаление — detour-update (фоном, ход в apply_log). */

export type PluginTone = "ok" | "warn" | "bad";
export type PluginArea = "services" | "rules" | "profiles";
export type PluginFieldType = "text" | "password" | "number" | "bool" | "select" | "textarea";

export interface PluginField {
  key: string;
  type?: PluginFieldType;
  label?: string;
  hint?: string;
  placeholder?: string;
  default?: unknown;
  min?: number;
  max?: number;
  maxlength?: number;
  options?: Array<{ value: string | number; label?: string } | string>;
}

export interface PluginAction {
  label: string;
  op: string;
  confirm?: string;
  variant?: "primary" | "ghost" | "danger";
}

export interface PluginCardSpec {
  title: string;
  hint?: string;
  icon?: string;
  span?: number;
  /** Действие, которое отдаёт содержимое карточки (PluginViewData). */
  op?: string;
  /** Период обновления, секунды (по умолчанию 30, 0 — только при открытии). */
  poll?: number;
  actions?: PluginAction[];
  /** Компонент из ESM-модуля плагина вместо декларативного содержимого. */
  component?: string;
}

export interface PluginSectionSpec {
  id: string;
  title: string;
  area?: PluginArea;
  icon?: string;
  summary?: string;
  op?: string;
  fields?: PluginField[];
  actions?: PluginAction[];
  component?: string;
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  icon?: string;
  min_panel?: string;
  platforms?: string[];
  ui?: {
    module?: string;
    card?: PluginCardSpec;
    sections?: PluginSectionSpec[];
  };
}

export interface InstalledPlugin {
  id: string;
  manifest: PluginManifest;
  enabled: boolean;
  compatible: boolean;
  reason: string;
  has_api: boolean;
  /** Путь UI-модуля относительно панели ("" — модуля нет). */
  module: string;
  settings: Record<string, unknown>;
  /** Для полей-паролей: задан ли пароль (сами значения не отдаются). */
  secrets: Record<string, boolean>;
  /** Последняя установка шла из подписанного пакета. */
  signed: boolean;
}

export interface PluginsList {
  ok: boolean;
  platform: string;
  panel: string;
  plugins: InstalledPlugin[];
}

/** Ответ действия, которое рисуется карточкой или секцией. */
export interface PluginViewData {
  ok?: boolean;
  status?: { text: string; tone?: PluginTone };
  lead?: string;
  metrics?: Array<{ label: string; value: string | number; hint?: string; tone?: PluginTone }>;
  actions?: PluginAction[];
  message?: string;
  error?: string;
}

export interface CatalogFile {
  path: string;
  sha256: string;
  size: number;
}

export interface CatalogPlugin {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  icon?: string;
  platforms?: string[];
  min_panel?: string;
  has_card?: boolean;
  files: Partial<Record<"ipk" | "apk" | "keenetic", CatalogFile>>;
}

export interface PluginCatalog {
  generated?: string;
  plugins: CatalogPlugin[];
}

export interface UploadedPlugin {
  ok: boolean;
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  /** true — подписан ключом Detour, false — нет, null — проверить нечем. */
  signed: boolean | null;
  compatible: boolean;
  reason: string;
}

/** lua-cjson отдаёт пустой массив как {} — приводим к массиву. */
function arr<T>(v: unknown): T[] {
  return Array.isArray(v) ? (v as T[]) : [];
}

function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
}

function normalizeManifest(m: PluginManifest): PluginManifest {
  const ui = m.ui && typeof m.ui === "object" ? m.ui : {};
  return {
    ...m,
    platforms: arr<string>(m.platforms),
    ui: {
      ...ui,
      card: ui.card ? { ...ui.card, actions: arr<PluginAction>(ui.card.actions) } : undefined,
      sections: arr<PluginSectionSpec>(ui.sections).map((s) => ({
        ...s,
        fields: arr<PluginField>(s.fields),
        actions: arr<PluginAction>(s.actions),
      })),
    },
  };
}

export const plugins = {
  list: async (): Promise<PluginsList> => {
    const r = await requestJson<PluginsList>("plugins_list");
    return {
      ...r,
      plugins: arr<InstalledPlugin>(r.plugins).map((p) => ({
        ...p,
        manifest: normalizeManifest(p.manifest),
        settings: obj(p.settings),
        secrets: obj(p.secrets) as Record<string, boolean>,
      })),
    };
  },

  /** Действие обработчика плагина. С телом — POST, без — GET. */
  call: <T = PluginViewData>(id: string, op: string, body?: unknown, timeoutMs = 30_000) =>
    requestJson<T>("plugin_call", {
      params: { id, op },
      body,
      method: body === undefined ? "GET" : "POST",
      timeoutMs,
    }),

  saveSettings: (id: string, values: Record<string, unknown>) =>
    requestJson<PluginViewData>("plugin_settings", { params: { id }, body: values }),

  toggle: (id: string, enabled: boolean) =>
    requestJson<{ ok: boolean }>("plugin_toggle", { params: { id }, body: { enabled } }),

  catalog: async (refresh = false): Promise<PluginCatalog> => {
    const r = await requestJson<PluginCatalog>("plugins_catalog", {
      method: refresh ? "POST" : "GET",
      timeoutMs: 90_000,
    });
    return { ...r, plugins: arr<CatalogPlugin>(r.plugins) };
  },

  /* --- установка и удаление: отсоединены, ход — в apply_log --- */
  install: (id: string) => requestJson<{ ok: boolean }>("plugin_install", { body: { id } }),
  remove: (id: string, purge: boolean) =>
    requestJson<{ ok: boolean }>("plugin_remove", { body: { id, purge } }),
  /** Шаг 1 загрузки: подпись (может быть пустой). */
  uploadSig: (sig: string) => requestJson<{ ok: boolean }>("plugin_upload_sig", { body: sig }),
  /** Шаг 2: сам пакет. Роутер разбирает его и отвечает, что это за плагин. */
  upload: (file: Blob) =>
    requestJson<UploadedPlugin>("plugin_upload", { body: file, timeoutMs: 120_000 }),
  /** Шаг 3: поставить загруженное (сторонний пакет — только с allowUnsigned). */
  installUploaded: (allowUnsigned: boolean) =>
    requestJson<{ ok: boolean }>("plugin_install_uploaded", {
      body: { allow_unsigned: allowUnsigned },
    }),
};
