<script setup lang="ts">
/* Иконка области в цветной плашке. Длинные списки раскрывающихся пунктов
   («Правила», «Журнал», «Сервисы») без неё сливались в одинаковые строки —
   форма и цвет дают зацепку глазу раньше, чем прочитан заголовок. Цвет
   закреплён за смыслом, а не за позицией: «напрямую» везде зелёное,
   «запрещено» — красное. */
import { computed } from "vue";
import SectionIcon from "@/components/SectionIcon.vue";

const props = defineProps<{ name: string; size?: number; small?: boolean }>();

const HUE: Record<string, string> = {
  tunnel: "blue",
  shield: "blue",
  route: "violet",
  chain: "violet",
  direct: "green",
  flag: "green",
  dpi: "amber",
  bolt: "amber",
  devices: "teal",
  apps: "teal",
  intercept: "slate",
  waves: "teal",
  block: "red",
  torrent: "red",
  dns: "violet",
  logs: "slate",
  code: "violet",
  sliders: "blue",
  firewall: "red",
  download: "green",
  terminal: "slate",
  pulse: "pink",
  heart: "pink",
  plug: "teal",
  lock: "green",
  bell: "amber",
  cpu: "blue",
  memory: "slate",
  archive: "amber",
  key: "violet",
  power: "red",
  server: "teal",
  chart: "blue",
  link: "green",
  target: "violet",
  tag: "slate",
  uplinks: "teal",
  rss: "amber",
  cloud: "amber",
  list: "blue",
};
const hue = computed(() => HUE[props.name] ?? "blue");
</script>

<template>
  <span class="ai" :class="[hue, { small }]" aria-hidden="true">
    <SectionIcon :name="name" :size="size ?? (small ? 15 : 18)" />
  </span>
</template>

<style scoped>
.ai {
  flex: none;
  display: inline-grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border-radius: 10px;
  color: var(--c);
  background: color-mix(in srgb, var(--c) 14%, transparent);
  border: 1px solid color-mix(in srgb, var(--c) 28%, transparent);
}
.ai.small {
  width: 26px;
  height: 26px;
  border-radius: 8px;
}
.blue { --c: #3b82f6; }
.green { --c: #1f9d55; }
.amber { --c: #c98a0b; }
.red { --c: #e5534b; }
.violet { --c: #8b5cf6; }
.teal { --c: #0f9f95; }
.pink { --c: #d6417f; }
.slate { --c: #6b7a90; }
/* Серый на тёмном фоне почти пропадает — там он светлее. */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) .slate { --c: #9aa8bb; }
}
:root[data-theme="dark"] .slate { --c: #9aa8bb; }
</style>
