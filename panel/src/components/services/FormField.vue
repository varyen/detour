<script setup lang="ts">
/* Подпись + поле ввода. Стиль полей задан здесь один раз через :deep(), чтобы
   не повторять его в каждой форме и не разъехаться: 16px — иначе iOS зумит
   страницу при фокусе, 44px — чтобы палец попадал в поле. */
defineProps<{ label: string; hint?: string }>();
</script>

<template>
  <label class="field">
    <span class="lbl">{{ label }}</span>
    <slot />
    <small v-if="hint">{{ hint }}</small>
  </label>
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}
.lbl {
  font-size: 12.5px;
  color: var(--dim);
}
.field small {
  font-size: 11.5px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.field :deep(input),
.field :deep(select),
.field :deep(textarea) {
  width: 100%;
  min-width: 0;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 9px 11px;
  font-size: 16px;
  min-height: 44px;
  outline: none;
}
.field :deep(textarea) {
  resize: vertical;
  font-family: var(--mono);
  font-size: 13px;
  line-height: 1.45;
}
.field :deep(input:focus),
.field :deep(select:focus),
.field :deep(textarea:focus) {
  border-color: var(--accent);
}
.field :deep(input:disabled),
.field :deep(select:disabled) {
  opacity: 0.55;
}
</style>
