<script setup lang="ts">
/* Моноширинное поле для того, что редактируется целиком текстом: конфиг
   sing-box, аргументы zapret, список целей проверки, подпись .ipk. */
const model = defineModel<string>({ default: "" });

defineProps<{
  label: string;
  rows?: number;
  placeholder?: string;
}>();
</script>

<template>
  <textarea
    v-model="model"
    class="code"
    :rows="rows ?? 14"
    :aria-label="label"
    :placeholder="placeholder"
    spellcheck="false"
    autocapitalize="off"
    autocomplete="off"
    autocorrect="off"
  ></textarea>
</template>

<style scoped>
.code {
  width: 100%;
  min-width: 0;
  resize: vertical;
  font-family: var(--mono);
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  line-height: 1.45;
  tab-size: 2;
  white-space: pre;
  overflow: auto;
  color: var(--ink);
  background: var(--panel-2);
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
}
@media (min-width: 861px) {
  .code {
    font-size: 13px;
  }
}
</style>
