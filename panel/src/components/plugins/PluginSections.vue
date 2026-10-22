<script setup lang="ts">
/* Секции плагинов в разделе панели (area: services | rules | profiles). Каждая —
   такая же сворачиваемая область, как у встроенных настроек: сводка из op,
   форма по схеме полей, кнопки-действия и/или компонент из UI-модуля. */
import { onMounted, reactive } from "vue";
import ServicePanel from "@/components/services/ServicePanel.vue";
import PluginView from "./PluginView.vue";
import PluginComponent from "./PluginComponent.vue";
import PluginSettingsForm from "./PluginSettingsForm.vue";
import { plugins } from "@/api";
import type { PluginArea, PluginViewData } from "@/api";
import { usePluginsStore } from "@/stores/plugins";
import type { PluginSectionDef } from "@/stores/plugins";

const props = defineProps<{ area: PluginArea }>();

const store = usePluginsStore();
const open = reactive<Record<string, boolean>>({});
const views = reactive<Record<string, PluginViewData | null>>({});
const errors = reactive<Record<string, string>>({});

async function loadView(d: PluginSectionDef) {
  if (!d.section.op) return;
  try {
    views[d.key] = await plugins.call(d.plugin.id, d.section.op);
    errors[d.key] = "";
  } catch (e) {
    errors[d.key] = e instanceof Error ? e.message : String(e);
  }
}

async function toggle(d: PluginSectionDef, v: boolean) {
  open[d.key] = v;
  if (v) await loadView(d);
}

async function saved(d: PluginSectionDef) {
  await store.load(true);
  await loadView(d);
}

onMounted(async () => {
  await store.load();
  /* Сводка в шапке видна и в свёрнутом виде — грузим её сразу. */
  await Promise.all(store.sections(props.area).map(loadView));
});
</script>

<template>
  <ServicePanel
    v-for="d in store.sections(area)"
    :id="`plugin-${d.plugin.id}-${d.section.id}`"
    :key="d.key"
    :open="!!open[d.key]"
    :icon="d.section.icon || d.plugin.manifest.icon || 'plug'"
    :title="d.section.title"
    :summary="views[d.key]?.lead || d.section.summary || `Плагин «${d.plugin.manifest.name}»`"
    :chip="views[d.key]?.status?.text"
    :tone="views[d.key]?.status?.tone"
    @update:open="toggle(d, $event)"
  >
    <div class="psec">
      <p v-if="errors[d.key]" class="hint">{{ errors[d.key] }}</p>
      <PluginView
        v-if="d.section.op || d.section.actions?.length"
        :plugin-id="d.plugin.id"
        :data="views[d.key] ? { ...views[d.key], lead: undefined } : null"
        :actions="d.section.actions"
        @done="loadView(d)"
      />
      <PluginSettingsForm
        v-if="d.section.fields?.length"
        :plugin="d.plugin"
        :fields="d.section.fields"
        @saved="saved(d)"
      />
      <PluginComponent v-if="d.section.component" :plugin="d.plugin" :name="d.section.component" />
    </div>
  </ServicePanel>
</template>

<style scoped>
.psec {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}
</style>
