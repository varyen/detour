<script setup lang="ts" generic="T extends string">
defineProps<{
  modelValue: T;
  options: { value: T; label: string; disabled?: boolean; hint?: string }[];
  label: string;
  busy?: boolean;
}>();

defineEmits<{ "update:modelValue": [T] }>();
</script>

<template>
  <div class="seg" role="group" :aria-label="label">
    <button
      v-for="o in options"
      :key="o.value"
      type="button"
      :aria-pressed="o.value === modelValue"
      :disabled="o.disabled || busy"
      :title="o.hint"
      @click="$emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>

<style scoped>
.seg {
  display: flex;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  /* Прокрутка, а не обрезка: на 360px набор из четырёх подписей шире экрана, и
     обрезанный последний вариант выглядит как поломка. Страницу вбок при этом
     не тянет — переполнение остаётся внутри самого переключателя. */
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none;
  width: max-content;
  max-width: 100%;
}
.seg::-webkit-scrollbar {
  display: none;
}
.seg button {
  border: 0;
  background: transparent;
  padding: 7px 13px;
  font-size: 12.5px;
  color: var(--dim);
  border-right: 1px solid var(--line);
  white-space: nowrap;
  /* Не сжимать варианты: пусть лучше прокручиваются целиком. */
  flex: 0 0 auto;
  /* Палец должен попадать без прицеливания. */
  min-height: 36px;
}
.seg button:last-child {
  border-right: 0;
}
.seg button[aria-pressed="true"] {
  background: var(--accent-wash);
  color: var(--accent);
  font-weight: 600;
}
.seg button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
