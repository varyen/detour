<script setup lang="ts">
/* Моноширинный вывод: лог, дамп правил, вывод установщика. Длинные строки
   прокручиваются внутри самого блока — страница вбок ехать не должна. */
import { nextTick, ref, watch } from "vue";

const props = defineProps<{
  text: string;
  /** Файла ещё нет — это не ошибка, а «пока нечего показывать». */
  missing?: boolean;
  missingText?: string;
  emptyText?: string;
  height?: string;
  /** Держать прокрутку у нижнего края — для живого хвоста. */
  follow?: boolean;
}>();

const box = ref<HTMLElement | null>(null);

watch(
  () => props.text,
  async () => {
    if (!props.follow) return;
    await nextTick();
    const el = box.value;
    if (el) el.scrollTop = el.scrollHeight;
  },
);
</script>

<template>
  <p v-if="missing" class="note">
    {{
      missingText ??
      "Журнала пока нет: файл ещё не создан. Он появится, как только служба что-нибудь запишет."
    }}
  </p>
  <p v-else-if="!text.trim()" class="note">{{ emptyText ?? "Пока пусто." }}</p>
  <pre
    v-else
    ref="box"
    class="dump"
    :style="{ maxHeight: height ?? '380px' }"
    tabindex="0"
  >{{ text }}</pre>
</template>

<style scoped>
.note {
  font-size: 13.5px;
  color: var(--dim);
  padding: 6px 2px;
}
.dump {
  margin: 0;
  max-width: 100%;
  overflow: auto;
  -webkit-overflow-scrolling: touch;
  overscroll-behavior: contain;
  white-space: pre;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.45;
  tab-size: 4;
  color: var(--ink);
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
}
</style>
