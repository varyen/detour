<script setup lang="ts">
/* Компонент из UI-модуля плагина. Модуль грузится лениво (один раз на версию
   плагина); не загрузился или нет такого экспорта — честная строка вместо
   пустого места. */
import { onMounted, ref, shallowRef } from "vue";
import type { Component } from "vue";
import type { InstalledPlugin } from "@/api";
import { usePluginsStore } from "@/stores/plugins";

const props = defineProps<{ plugin: InstalledPlugin; name: string }>();

const store = usePluginsStore();
const comp = shallowRef<Component | null>(null);
const failed = ref("");

onMounted(async () => {
  const m = await store.module(props.plugin);
  const c = m?.components?.[props.name];
  if (c) comp.value = c as Component;
  else failed.value = m ? `в модуле нет компонента «${props.name}»` : "UI-модуль плагина не загрузился";
});
</script>

<template>
  <component :is="comp" v-if="comp" />
  <p v-else-if="failed" class="hint">{{ failed }}</p>
  <p v-else class="hint">загружаю…</p>
</template>
