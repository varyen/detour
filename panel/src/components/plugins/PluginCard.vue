<script setup lang="ts">
/* Карточка плагина на «Обзоре». Содержимое — ответ действия card.op (данные,
   которые рисует PluginView) либо компонент из UI-модуля плагина. Опрос идёт,
   только пока вкладка видна: карточка, которую никто не смотрит, не должна
   дёргать роутер раз в полминуты. */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import TileCard from "@/components/TileCard.vue";
import PluginView from "./PluginView.vue";
import PluginComponent from "./PluginComponent.vue";
import { plugins } from "@/api";
import type { InstalledPlugin, PluginViewData } from "@/api";

const props = defineProps<{ plugin: InstalledPlugin }>();

const card = computed(() => props.plugin.manifest.ui?.card);
const data = ref<PluginViewData | null>(null);
const error = ref("");
let timer: ReturnType<typeof setInterval> | undefined;

async function load() {
  const op = card.value?.op;
  if (!op || document.visibilityState === "hidden") return;
  try {
    data.value = await plugins.call(props.plugin.id, op);
    error.value = "";
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

onMounted(() => {
  void load();
  const every = Number(card.value?.poll ?? 30);
  if (card.value?.op && every > 0) timer = setInterval(load, Math.max(5, every) * 1000);
});
onBeforeUnmount(() => clearInterval(timer));
</script>

<template>
  <TileCard :icon="card?.icon || plugin.manifest.icon || 'plug'" :title="card?.title || plugin.manifest.name">
    <template v-if="data?.status?.text" #badge>
      <span class="chip" :class="data.status.tone">{{ data.status.text }}</span>
    </template>
    <PluginComponent v-if="card?.component" :plugin="plugin" :name="card.component" />
    <template v-else>
      <p v-if="error" class="hint">{{ error }}</p>
      <p v-else-if="!data && card?.op" class="hint">читаю состояние…</p>
      <PluginView :plugin-id="plugin.id" :data="data" :actions="card?.actions" @done="load" />
    </template>
  </TileCard>
</template>

<style scoped>
.chip {
  margin-left: auto;
  font-size: 11.5px;
  font-weight: 500;
  border: 1px solid var(--line-2);
  border-radius: 999px;
  padding: 2px 9px;
  color: var(--dim);
  white-space: nowrap;
}
.chip.ok {
  color: var(--ok);
  border-color: color-mix(in srgb, var(--ok) 45%, transparent);
}
.chip.warn {
  color: var(--warn);
  border-color: color-mix(in srgb, var(--warn) 45%, transparent);
}
.chip.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
</style>
