<script setup lang="ts">
/* Одна область правил: строка «что сейчас настроено» и раскрытие. Семь вкладок
   старой панели превратились в вертикальный список именно так — чтобы с
   телефона было видно всё состояние сразу, без переключения. */
defineProps<{
  title: string;
  summary: string;
  open: boolean;
  /* Короткая пометка справа от заголовка: «выключено», «недоступно». */
  tag?: string;
  tone?: "ok" | "warn" | "bad" | "off";
}>();

const emit = defineEmits<{ toggle: [] }>();
</script>

<template>
  <section class="area" :class="{ expanded: open }">
    <button class="head" type="button" :aria-expanded="open" @click="emit('toggle')">
      <span class="txt">
        <span class="t">
          {{ title }}
          <em v-if="tag" class="tag" :class="tone ?? 'off'">{{ tag }}</em>
        </span>
        <small class="s">{{ summary }}</small>
      </span>
      <svg
        class="chev"
        :class="{ up: open }"
        width="18"
        height="18"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <div v-if="open" class="body"><slot /></div>
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
.area.expanded {
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
  padding: 12px 14px;
  /* Палец должен попадать по строке целиком. */
  min-height: 56px;
}
.head:hover {
  background: var(--panel-2);
}
.txt {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.t {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.tag {
  font-style: normal;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  border-radius: 999px;
  padding: 2px 8px;
  border: 1px solid var(--line-2);
  color: var(--dim);
}
.tag.ok {
  color: var(--ok);
  border-color: color-mix(in srgb, var(--ok) 45%, transparent);
}
.tag.warn {
  color: var(--warn);
  border-color: color-mix(in srgb, var(--warn) 45%, transparent);
}
.tag.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
.s {
  font-size: 13px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.chev {
  margin-left: auto;
  flex: none;
  color: var(--faint);
  transition: transform 0.18s ease;
}
.chev.up {
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
@media (max-width: 700px) {
  .head {
    padding: 12px;
  }
  .body {
    padding: 12px;
  }
}
</style>
