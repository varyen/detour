import { defineStore } from "pinia";
import { computed, ref, shallowRef } from "vue";
import { plugins as api } from "@/api";
import type { InstalledPlugin, PluginArea, PluginSectionSpec } from "@/api";
import { useSessionStore } from "@/stores/session";

/* Установленные плагины и их UI-модули. Список грузится один раз на сессию и
   перечитывается после установки, удаления, включения и сохранения настроек.
   В приложении на устройстве плагинов нет: это пакеты роутера. */

export interface PluginTileDef {
  /** id карточки на «Обзоре»: plugin:<id плагина>. */
  id: string;
  plugin: InstalledPlugin;
  title: string;
  hint: string;
  span: number;
}

export interface PluginSectionDef {
  key: string;
  plugin: InstalledPlugin;
  section: PluginSectionSpec;
}

export interface PluginModule {
  components?: Record<string, unknown>;
}

export const PLUGIN_TILE_PREFIX = "plugin:";

export const usePluginsStore = defineStore("plugins", () => {
  const list = ref<InstalledPlugin[]>([]);
  const platform = ref("");
  const loaded = ref(false);
  const loading = ref(false);
  const error = ref("");
  /* Модули по id плагина: промис, чтобы два компонента не грузили дважды. */
  const modules = shallowRef<Record<string, Promise<PluginModule | null>>>({});

  const active = computed(() => list.value.filter((p) => p.enabled && p.compatible));

  const tiles = computed<PluginTileDef[]>(() =>
    active.value
      .filter((p) => p.manifest.ui?.card)
      .map((p) => {
        const c = p.manifest.ui!.card!;
        return {
          id: PLUGIN_TILE_PREFIX + p.id,
          plugin: p,
          title: c.title || p.manifest.name,
          hint: c.hint || p.manifest.description || "карточка плагина",
          span: [2, 3, 4, 6].includes(Number(c.span)) ? Number(c.span) : 2,
        };
      }),
  );

  function sections(area: PluginArea): PluginSectionDef[] {
    const out: PluginSectionDef[] = [];
    for (const p of active.value) {
      for (const s of p.manifest.ui?.sections ?? []) {
        if ((s.area ?? "services") === area) out.push({ key: `${p.id}:${s.id}`, plugin: p, section: s });
      }
    }
    return out;
  }

  function byId(id: string) {
    return list.value.find((p) => p.id === id);
  }

  async function load(force = false) {
    if (__CLIENT__) {
      loaded.value = true;
      return;
    }
    if (loading.value || (loaded.value && !force)) return;
    /* Админ «только VPN-сервер» плагинов не видит: ROLE_GATE ответил бы 403. */
    if (useSessionStore().role === "vpn") {
      loaded.value = true;
      return;
    }
    loading.value = true;
    try {
      const r = await api.list();
      list.value = r.plugins;
      platform.value = r.platform;
      error.value = "";
      /* Версия плагина могла смениться — модуль грузим заново. */
      modules.value = {};
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
      loaded.value = true;
    }
  }

  function module(p: InstalledPlugin): Promise<PluginModule | null> {
    if (!p.module) return Promise.resolve(null);
    const cached = modules.value[p.id];
    if (cached) return cached;
    const url = new URL(p.module, document.baseURI);
    url.searchParams.set("v", p.manifest.version);
    const promise = import(/* @vite-ignore */ url.href)
      .then(async (m: { default?: (host: unknown) => PluginModule | Promise<PluginModule> }) => {
        if (typeof m.default !== "function") throw new Error("модуль без export default");
        const { makeHost } = await import("@/lib/pluginHost");
        return (await m.default(makeHost(p))) ?? null;
      })
      .catch((e: unknown) => {
        console.error(`[plugin ${p.id}] модуль не загрузился`, e);
        return null;
      });
    modules.value = { ...modules.value, [p.id]: promise };
    return promise;
  }

  return { list, platform, loaded, loading, error, active, tiles, sections, byId, load, module };
});
