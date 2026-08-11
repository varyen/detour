<script setup lang="ts">
/* Редактор состава «Обзора». Порядок меняется кнопками «вверх/вниз», а не
   перетаскиванием: на телефоне перетаскивание внутри прокручиваемого листа
   спорит с самой прокруткой, а кнопка одинаково работает и пальцем, и
   клавиатурой. */
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import { useDashboardStore } from "@/stores/dashboard";

defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const dash = useDashboardStore();
</script>

<template>
  <DrawerSheet :open="open" title="Состав главной страницы" @close="emit('close')">
    <p class="note">
      Выключенные карточки просто не показываются — ничего не отключается на
      роутере. Набор хранится в этом браузере, поэтому на телефоне он может быть
      свой.
    </p>

    <ul class="list">
      <li v-for="(t, i) in dash.tiles" :key="t.id" :class="{ off: !dash.isVisible(t.id) }">
        <SwitchToggle
          :model-value="dash.isVisible(t.id)"
          :label="t.title"
          :hint="t.hint"
          @update:model-value="dash.toggle(t.id)"
        />
        <span class="ord">
          <button
            type="button"
            class="ico"
            :disabled="i === 0"
            :aria-label="`Поднять «${t.title}»`"
            @click="dash.move(t.id, -1)"
          >
            ↑
          </button>
          <button
            type="button"
            class="ico"
            :disabled="i === dash.tiles.length - 1"
            :aria-label="`Опустить «${t.title}»`"
            @click="dash.move(t.id, 1)"
          >
            ↓
          </button>
        </span>
      </li>
    </ul>

    <template #footer>
      <UiButton variant="primary" @click="emit('close')">Готово</UiButton>
      <UiButton :disabled="!dash.customized" @click="dash.reset()">
        Вернуть как было
      </UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.note {
  font-size: 13px;
  color: var(--dim);
  margin-bottom: 10px;
}
.list {
  list-style: none;
  display: flex;
  flex-direction: column;
}
.list li {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
  border-bottom: 1px solid var(--line);
}
.list li > :first-child {
  flex: 1 1 auto;
  min-width: 0;
}
/* Выключенная карточка остаётся читаемой — она ещё пригодится, когда её вернут. */
.list li.off {
  opacity: 0.55;
}
.ord {
  display: flex;
  gap: 6px;
  flex: none;
}
.ico {
  border: 1px solid var(--line-2);
  background: transparent;
  border-radius: var(--radius-sm);
  width: 36px;
  min-height: 36px;
  color: var(--dim);
  line-height: 1;
}
.ico:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--accent);
}
.ico:disabled {
  opacity: 0.35;
}
</style>
