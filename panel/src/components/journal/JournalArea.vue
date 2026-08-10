<script setup lang="ts">
/* Одна область раздела «Журнал»: заголовок со сводкой виден всегда,
   содержимое раскрывается по нажатию. Так страница читается сверху вниз
   одним списком, а тяжёлое (логи, дампы, редакторы) не загружается, пока
   его не попросили. */
const open = defineModel<boolean>("open", { default: false });

defineProps<{ title: string; summary?: string }>();
</script>

<template>
  <section class="area" :class="{ expanded: open }">
    <button class="head" type="button" :aria-expanded="open" @click="open = !open">
      <span class="txt">
        <span class="eyebrow">{{ title }}</span>
        <span v-if="summary" class="sum">{{ summary }}</span>
      </span>
      <slot name="badge" />
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
  gap: 10px;
  text-align: left;
  border: 0;
  background: transparent;
  padding: 12px 14px;
  /* Палец должен попадать по заголовку без прицеливания. */
  min-height: 56px;
}
.head:hover {
  background: var(--panel-2);
}
.txt {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  flex: 1 1 auto;
}
.sum {
  font-size: 13px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.chev {
  flex: none;
  color: var(--faint);
  transition: transform 0.2s ease;
}
.chev.up {
  transform: rotate(180deg);
}
.body {
  display: flex;
  flex-direction: column;
  gap: 13px;
  padding: 2px 14px 15px;
  min-width: 0;
}
</style>
