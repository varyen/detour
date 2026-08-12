<script setup lang="ts">
/* Слот карточки на «Обзоре»: место в сетке, а в режиме редактирования — ещё и
   ручка переноса, выбор ширины и «скрыть».
 *
 * Почему обёртка, а не правки в каждой карточке: карточки ничего не знают ни о
 * своём месте, ни о своей ширине — это настройка того, кто смотрит, и живёт она
 * в одном месте (stores/dashboard).
 */
import { computed, inject } from "vue";
import SectionIcon from "@/components/SectionIcon.vue";
import { DASH_TILES, SPANS, useDashboardStore } from "@/stores/dashboard";
import { EDIT_KEY, type EditContext } from "./dash-edit";

const props = defineProps<{ id: string }>();

const dash = useDashboardStore();
const edit = inject<EditContext | null>(EDIT_KEY, null);

const title = computed(() => DASH_TILES.find((t) => t.id === props.id)?.title ?? props.id);
const editing = computed(() => !!edit?.editing.value);
const index = computed(() => dash.visible.findIndex((t) => t.id === props.id));
const held = computed(() => edit?.dragId.value === props.id);
/* Подсветка места приземления — на карточке, ЧЬЁ место займут. Своё же место
   подсвечивать нечего: это «оставить как есть». */
const isDrop = computed(
  () =>
    !!edit &&
    !!edit.dragId.value &&
    edit.dragId.value !== props.id &&
    edit.dropIndex.value === index.value,
);

const style = computed(() => {
  const s: Record<string, string> = {
    order: String(dash.orderOf(props.id)),
    "--span": String(dash.spanOf(props.id)),
  };
  if (held.value && edit) {
    s.transform = `translate(${edit.dx.value}px, ${edit.dy.value}px)`;
  }
  return s;
});

function onKey(e: KeyboardEvent) {
  const dir =
    e.key === "ArrowLeft" || e.key === "ArrowUp"
      ? -1
      : e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : 0;
  if (!dir) return;
  e.preventDefault();
  dash.move(props.id, dir as -1 | 1);
}
</script>

<template>
  <div
    v-if="dash.isVisible(id)"
    class="slot"
    :class="{ editing, held, drop: isDrop, wide: dash.spanOf(id) >= 4 }"
    :data-tile="id"
    :style="style"
  >
    <!-- Пока правят состав, содержимое карточки не кликается: иначе «перенести»
         норовит обернуться «перезапустить». -->
    <div class="body" :class="{ frozen: editing }">
      <slot />
    </div>

    <div v-if="editing" class="chrome">
      <button
        type="button"
        class="grab"
        :aria-label="`Переместить «${title}»: тяните мышью или стрелками`"
        @pointerdown="edit?.grab($event, id)"
        @keydown="onKey"
      >
        <SectionIcon name="grip" :size="16" />
      </button>
      <span class="name">{{ title }}</span>
      <div class="sizes" role="group" :aria-label="`Ширина «${title}»`">
        <button
          v-for="s in SPANS"
          :key="s.span"
          type="button"
          :class="{ on: dash.spanOf(id) === s.span }"
          :aria-pressed="dash.spanOf(id) === s.span"
          :title="s.title"
          @click="dash.setSpan(id, s.span)"
        >
          {{ s.label }}
        </button>
      </div>
      <button type="button" class="hide" :title="`Скрыть «${title}»`" @click="dash.toggle(id)">
        <SectionIcon name="close" :size="16" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.slot {
  display: flex;
  min-width: 0;
  position: relative;
}
.slot > .body {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
}
.slot > .body > :deep(*) {
  flex: 1 1 auto;
  min-width: 0;
}
.body.frozen {
  pointer-events: none;
  user-select: none;
}

/* ---- режим правки ---- */
.slot.editing {
  outline: 1px dashed var(--line-2);
  outline-offset: 3px;
  border-radius: var(--radius);
}
.slot.drop {
  outline: 2px dashed var(--accent);
  outline-offset: 3px;
}
.slot.held {
  z-index: 5;
  cursor: grabbing;
  box-shadow: var(--shadow);
  border-radius: var(--radius);
  /* Поднятая карточка не должна ловить события — под курсором нужен тот, над
     кем её несут. */
  pointer-events: none;
  outline-color: var(--accent);
}

.chrome {
  position: absolute;
  top: -14px;
  left: 10px;
  right: 10px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 6px;
  border-radius: 999px;
  border: 1px solid var(--line-2);
  background: var(--panel);
  backdrop-filter: blur(10px);
  font-size: 12px;
  color: var(--dim);
}
.chrome .name {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.grab {
  flex: none;
  border: 0;
  background: transparent;
  color: var(--faint);
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  cursor: grab;
  /* Жест на ручке — только перенос: прокрутка страницы остаётся жестом по
     любому другому месту. */
  touch-action: none;
}
.grab:hover,
.grab:focus-visible {
  color: var(--accent);
}
.sizes {
  display: flex;
  gap: 2px;
  flex: none;
}
.sizes button {
  border: 1px solid transparent;
  background: transparent;
  color: var(--faint);
  min-width: 26px;
  height: 22px;
  border-radius: 999px;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.sizes button:hover {
  color: var(--ink);
  border-color: var(--line-2);
}
.sizes button.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-on);
  font-weight: 600;
}
.hide {
  flex: none;
  border: 0;
  background: transparent;
  color: var(--faint);
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  cursor: pointer;
}
.hide:hover {
  color: var(--bad);
}

@media (max-width: 700px) {
  .chrome {
    left: 6px;
    right: 6px;
  }
  /* На телефоне колонка одна: выбор ширины там ничего не меняет и только
     занимает место — остаются перенос и «скрыть». */
  .sizes {
    display: none;
  }
}
</style>
