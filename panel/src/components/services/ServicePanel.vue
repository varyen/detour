<script setup lang="ts">
/* Область раздела «Сервисы и доступ»: свёрнутая показывает только суть, в
   развёрнутую попадает управление. Экран получается длинным списком коротких
   строк — на телефоне это читается лучше, чем семь одновременно раскрытых
   карточек. */
defineProps<{
  title: string;
  /** Короткая фраза о текущем состоянии — видна и в свёрнутом виде. */
  summary?: string;
  /** Метка справа: «включено», «нет сертификата», … */
  chip?: string;
  tone?: "ok" | "warn" | "bad";
  open: boolean;
}>();

defineEmits<{ "update:open": [boolean] }>();
</script>

<template>
  <section class="area" :class="{ opened: open }">
    <button
      class="head"
      type="button"
      :aria-expanded="open"
      @click="$emit('update:open', !open)"
    >
      <span class="txt">
        <span class="ttl">{{ title }}</span>
        <small v-if="summary">{{ summary }}</small>
      </span>
      <span v-if="chip" class="chip" :class="tone">{{ chip }}</span>
      <svg
        class="chev"
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <div v-if="open" class="body">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.area {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  backdrop-filter: blur(10px);
  min-width: 0;
  overflow: hidden;
}
.area.opened {
  border-color: var(--line-2);
}
.head {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 12px;
  text-align: left;
  border: 0;
  background: transparent;
  padding: 13px 14px;
  /* Цель нажатия на всю строку — палец попадает без прицеливания. */
  min-height: 56px;
}
.head:hover {
  background: var(--panel-2);
}
.txt {
  min-width: 0;
  flex: 1 1 auto;
}
.ttl {
  display: block;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.txt small {
  display: block;
  font-size: 12.5px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.chip {
  flex: none;
  font-size: 11.5px;
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
.chev {
  flex: none;
  color: var(--faint);
  transition: transform 0.18s ease;
}
.area.opened .chev {
  transform: rotate(180deg);
}
.body {
  border-top: 1px solid var(--line);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}

@media (max-width: 420px) {
  .head {
    gap: 8px;
    padding: 12px;
  }
  .chip {
    /* Заголовок сжимается (min-width:0), метка остаётся целой. */
    padding: 2px 7px;
    font-size: 11px;
  }
}
</style>
