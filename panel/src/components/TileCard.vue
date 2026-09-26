<script setup lang="ts">
defineProps<{ title: string; span?: boolean }>();
</script>

<template>
  <section class="tile" :class="{ span }">
    <h2 class="eyebrow">
      {{ title }}
      <slot name="badge" />
    </h2>
    <slot />
    <div v-if="$slots.actions" class="actions">
      <slot name="actions" />
    </div>
  </section>
</template>

<style scoped>
.tile {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  backdrop-filter: blur(10px);
  padding: 15px 16px 14px;
  display: flex;
  flex-direction: column;
  gap: 9px;
  min-width: 0;
}
.tile.span {
  grid-column: span 2;
}
/* Глобальный .eyebrow — 10px бледной капителью: для подписей колонок годится,
   а заголовок карточки в нём теряется. */
.eyebrow {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  font-family: var(--sans);
  font-size: 15px;
  font-weight: 650;
  letter-spacing: 0;
  text-transform: none;
  color: var(--ink);
}
.actions {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  margin-top: auto;
  padding-top: 4px;
}
@media (max-width: 700px) {
  .tile.span {
    grid-column: span 1;
  }
}
</style>
