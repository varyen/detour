<script setup lang="ts">
/* Редактор большого списка. Живёт в шторке (на телефоне — нижний лист), потому
   что на экране правил важнее видеть, ЧТО настроено, а не редактировать всё
   сразу. Две кнопки сохранения — осознанный выбор: «Сохранить» только пишет
   файл, «Сохранить и применить» ещё и перестраивает конфиг с перезапуском
   sing-box, а это десятки секунд. */
import { computed } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import { countEntries, entriesLabel } from "./entries";

const props = defineProps<{
  open: boolean;
  title: string;
  modelValue: string;
  /* Человеческое объяснение, что вообще делает этот список. */
  hint?: string;
  placeholder?: string;
  loading?: boolean;
  /* "" | "save" | "apply" — какая кнопка сейчас работает. */
  busy?: string;
  readonly?: boolean;
  /* Нет отдельного «применить»: список применяется сразу при сохранении. */
  singleSave?: boolean;
  saveLabel?: string;
  applyLabel?: string;
  applyNote?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [string];
  close: [];
  save: [boolean];
}>();

const count = computed(() => countEntries(props.modelValue));
const anyBusy = computed(() => props.busy === "save" || props.busy === "apply");
</script>

<template>
  <DrawerSheet :open="open" :title="title" wide @close="emit('close')">
    <template #sticky>
      <p class="hint">
        <span v-if="hint">{{ hint }}</span>
        <b class="num">{{ loading ? "загружаю…" : entriesLabel(count) }}</b>
      </p>
    </template>

    <textarea
      class="ta mono"
      :value="modelValue"
      :placeholder="placeholder"
      :readonly="readonly || loading"
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
      autocorrect="off"
      :aria-label="title"
      @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
    ></textarea>

    <p v-if="applyNote && !readonly" class="note">{{ applyNote }}</p>

    <template #footer>
      <template v-if="readonly">
        <UiButton @click="emit('close')">Закрыть</UiButton>
      </template>
      <template v-else-if="singleSave">
        <UiButton
          variant="primary"
          :busy="busy === 'apply'"
          :disabled="loading || anyBusy"
          @click="emit('save', true)"
        >
          {{ applyLabel ?? "Сохранить и применить" }}
        </UiButton>
        <UiButton :disabled="anyBusy" @click="emit('close')">Отмена</UiButton>
      </template>
      <template v-else>
        <UiButton
          variant="primary"
          :busy="busy === 'apply'"
          :disabled="loading || anyBusy"
          @click="emit('save', true)"
        >
          {{ applyLabel ?? "Сохранить и применить" }}
        </UiButton>
        <UiButton
          :busy="busy === 'save'"
          :disabled="loading || anyBusy"
          @click="emit('save', false)"
        >
          {{ saveLabel ?? "Только сохранить" }}
        </UiButton>
        <UiButton :disabled="anyBusy" @click="emit('close')">Отмена</UiButton>
      </template>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.hint {
  font-size: 12.5px;
  color: var(--dim);
  display: flex;
  gap: 10px;
  align-items: baseline;
  flex-wrap: wrap;
}
.hint b {
  margin-left: auto;
  color: var(--ink);
  font-weight: 600;
  white-space: nowrap;
}
.ta {
  width: 100%;
  min-height: 46dvh;
  resize: vertical;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 10px 12px;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  line-height: 1.45;
  outline: none;
}
.ta:focus {
  border-color: var(--accent);
}
.ta[readonly] {
  color: var(--dim);
}
.note {
  margin-top: 10px;
  font-size: 12px;
  color: var(--faint);
}
@media (max-width: 860px) {
  .ta {
    min-height: 38dvh;
  }
}
@media (max-width: 700px) {
  /* Кнопки сохранения — главные цели на этом экране, пальцем по ним промахиваться нельзя. */
  :deep(.btn) {
    min-height: 44px;
  }
}
</style>
