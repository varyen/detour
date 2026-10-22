import * as Vue from "vue";
import { reactive } from "vue";
import { plugins as api } from "@/api";
import type { InstalledPlugin } from "@/api";
import { useToastStore } from "@/stores/toast";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import TileCard from "@/components/TileCard.vue";
import FormField from "@/components/services/FormField.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import AreaIcon from "@/components/AreaIcon.vue";

/* Объект, который получает default-экспорт UI-модуля плагина. Модуль не
   собирается вместе с панелью, поэтому всё нужное — Vue того же экземпляра,
   вызовы роутера, тосты, базовые компоненты — приходит отсюда, а не импортом.
   Контракт версионируется полем apiVersion: поля только добавляются. */

export const PLUGIN_HOST_API = 1;

export function makeHost(p: InstalledPlugin) {
  const toast = useToastStore();
  const settings = reactive({ ...p.settings });
  const secrets = reactive({ ...p.secrets });
  const passwords = new Set(
    (p.manifest.ui?.sections ?? [])
      .flatMap((s) => s.fields ?? [])
      .filter((f) => f.type === "password")
      .map((f) => f.key),
  );
  return {
    apiVersion: PLUGIN_HOST_API,
    vue: Vue,
    plugin: {
      id: p.id,
      name: p.manifest.name,
      version: p.manifest.version,
      manifest: p.manifest,
    },
    /** Текущие настройки (пароли — пустые строки; задан ли — в secrets). */
    settings,
    secrets,
    /** Действие обработчика api плагина; с телом — POST. */
    call: <T = unknown>(op: string, body?: unknown) => api.call<T>(p.id, op, body),
    async saveSettings(patch: Record<string, unknown>) {
      const r = await api.saveSettings(p.id, patch);
      for (const [k, v] of Object.entries(patch)) {
        if (!passwords.has(k)) settings[k] = v;
        else if (v) secrets[k] = true;
      }
      return r;
    },
    toast: {
      ok: (t: string) => toast.ok(t),
      info: (t: string) => toast.info(t),
      error: (t: string) => toast.error(t),
    },
    components: { UiButton, SwitchToggle, TileCard, FormField, SegmentedControl, AreaIcon },
  };
}

export type PluginHost = ReturnType<typeof makeHost>;
