<script setup lang="ts">
/* Редактор состава «Обзора». Порядок меняется перетаскиванием, но тянуть можно
   только за ручку слева: если бы тянулась вся строка, на телефоне
   перетаскивание спорило бы с прокруткой листа. Ручка — обычная кнопка, и с
   клавиатуры порядок меняется теми же стрелками ↑/↓, что и раньше.

   Тянем на pointer-событиях (а не на HTML5 drag&drop): последний на тач-экранах
   просто не работает. Пока палец в движении, ничего не перестраивается —
   двигаются только transform'ы, а порядок в сторе меняется один раз, когда
   отпустили. */
import { onBeforeUnmount, ref } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SectionIcon from "@/components/SectionIcon.vue";
import { useDashboardStore } from "@/stores/dashboard";

defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const dash = useDashboardStore();

const listEl = ref<HTMLUListElement | null>(null);
const dragId = ref<string | null>(null);
const dragFrom = ref(-1);
const dragTo = ref(-1);
const dragDy = ref(0);
/* Кадр после «отпустили»: строка уже встала на новое место в списке, и её
   собственный transform обязан исчезнуть без анимации — иначе она проедет
   лишнюю высоту от старого сдвига. */
const settling = ref(false);

/* Геометрия строк снимается один раз, в начале перетаскивания: transform на
   offsetTop/offsetHeight не влияет, так что она остаётся верной до конца. */
let rows: { top: number; h: number }[] = [];
let grabY = 0; // точка захвата в координатах списка
let lastY = 0; // последняя позиция пальца в координатах экрана
let pointerId = -1;
let scroller: HTMLElement | null = null;
let raf = 0;

/** Зона у края прокрутки, в которой лист подкручивается сам. */
const EDGE = 44;

function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const o = getComputedStyle(p).overflowY;
    if ((o === "auto" || o === "scroll") && p.scrollHeight > p.clientHeight) return p;
  }
  return null;
}

function onGrab(e: PointerEvent, id: string, i: number) {
  const list = listEl.value;
  /* Второй палец не перехватывает перенос у первого. */
  if (!list || e.button > 0 || pointerId !== -1) return;
  rows = (Array.from(list.children) as HTMLElement[]).map((el) => ({
    top: el.offsetTop,
    h: el.offsetHeight,
  }));
  dragId.value = id;
  dragFrom.value = i;
  dragTo.value = i;
  dragDy.value = 0;
  grabY = e.clientY - list.getBoundingClientRect().top;
  lastY = e.clientY;
  pointerId = e.pointerId;
  scroller = scrollParent(list);
  addEventListener("pointermove", onMove, { passive: false });
  addEventListener("pointerup", onDrop);
  addEventListener("pointercancel", onDrop);
  e.preventDefault();
}

/** Пересчёт сдвига и целевого места. Зовётся и с пальца, и с автопрокрутки. */
function update() {
  const list = listEl.value;
  const from = dragFrom.value;
  if (!list || from < 0) return;
  dragDy.value = lastY - list.getBoundingClientRect().top - grabY;
  const center = rows[from].top + dragDy.value + rows[from].h / 2;
  let to = from;
  while (to > 0 && center < rows[to - 1].top + rows[to - 1].h / 2) to--;
  while (to < rows.length - 1 && center > rows[to + 1].top + rows[to + 1].h / 2) to++;
  dragTo.value = to;
}

function onMove(e: PointerEvent) {
  if (e.pointerId !== pointerId) return;
  lastY = e.clientY;
  update();
  autoScroll();
  e.preventDefault();
}

function autoScroll() {
  if (!scroller || raf) return;
  const tick = () => {
    raf = 0;
    if (!scroller || dragFrom.value < 0) return;
    const r = scroller.getBoundingClientRect();
    const up = lastY - r.top;
    const down = r.bottom - lastY;
    const step =
      up < EDGE ? -Math.ceil((EDGE - up) / 3) : down < EDGE ? Math.ceil((EDGE - down) / 3) : 0;
    if (!step) return; // ушли от края — цикл останавливается сам
    const was = scroller.scrollTop;
    scroller.scrollTop = was + step;
    if (scroller.scrollTop === was) return; // упёрлись в край списка
    update();
    raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);
}

function stop() {
  removeEventListener("pointermove", onMove);
  removeEventListener("pointerup", onDrop);
  removeEventListener("pointercancel", onDrop);
  if (raf) cancelAnimationFrame(raf);
  raf = 0;
  pointerId = -1;
  scroller = null;
  dragId.value = null;
  dragFrom.value = -1;
  dragTo.value = -1;
  dragDy.value = 0;
}

function onDrop(e: PointerEvent) {
  if (e.pointerId !== pointerId) return;
  const from = dragFrom.value;
  const to = dragTo.value;
  stop();
  settling.value = true;
  requestAnimationFrame(() => requestAnimationFrame(() => (settling.value = false)));
  dash.reorder(from, to);
}

onBeforeUnmount(stop);

/** Соседи расступаются ровно на высоту переносимой строки. */
function styleFor(i: number) {
  const from = dragFrom.value;
  if (from < 0) return undefined;
  if (i === from) return { transform: `translateY(${dragDy.value}px)` };
  const to = dragTo.value;
  const h = rows[from].h;
  if (to > from && i > from && i <= to) return { transform: `translateY(${-h}px)` };
  if (to < from && i >= to && i < from) return { transform: `translateY(${h}px)` };
  return undefined;
}

function onKey(e: KeyboardEvent, id: string) {
  const dir = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
  if (!dir) return;
  e.preventDefault();
  dash.move(id, dir);
}
</script>

<template>
  <DrawerSheet :open="open" title="Состав главной страницы" @close="emit('close')">
    <p class="note">
      Выключенные карточки просто не показываются — ничего не отключается на
      роутере. Порядок меняется перетаскиванием за ручку слева (с клавиатуры —
      стрелками ↑ и ↓). Набор хранится в этом браузере, поэтому на телефоне он
      может быть свой.
    </p>

    <ul ref="listEl" class="list" :class="{ dragging: !!dragId, settling }">
      <li
        v-for="(t, i) in dash.tiles"
        :key="t.id"
        :class="{ off: !dash.isVisible(t.id), held: dragId === t.id }"
        :style="styleFor(i)"
      >
        <button
          type="button"
          class="grab"
          :aria-label="`Переместить «${t.title}»: тяните мышью или стрелками вверх и вниз`"
          @pointerdown="onGrab($event, t.id, i)"
          @keydown="onKey($event, t.id)"
        >
          <SectionIcon name="grip" :size="18" />
        </button>
        <SwitchToggle
          :model-value="dash.isVisible(t.id)"
          :label="t.title"
          :hint="t.hint"
          @update:model-value="dash.toggle(t.id)"
        />
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
  transition: transform 0.16s cubic-bezier(0.3, 0.8, 0.3, 1);
}
.list li > :last-child {
  flex: 1 1 auto;
  min-width: 0;
}
/* Выключенная карточка остаётся читаемой — она ещё пригодится, когда её вернут. */
.list li.off {
  opacity: 0.55;
}
/* Пока тянут, текст не должен выделяться под пальцем/курсором. */
.list.dragging {
  user-select: none;
  cursor: grabbing;
}
.list li.held {
  position: relative;
  z-index: 2;
  border-radius: var(--radius-sm);
  border-bottom-color: transparent;
  /* Поднятая строка обязана быть НЕпрозрачной: --panel-2 — это полупрозрачная
     плёнка, сквозь неё просвечивал бы текст строки, над которой её несут. */
  background-color: var(--ground);
  background-image: linear-gradient(var(--panel-2), var(--panel-2));
  box-shadow: var(--shadow);
  transition: none;
  opacity: 1;
}
.grab {
  flex: none;
  border: 0;
  background: transparent;
  color: var(--faint);
  width: 30px;
  min-height: 36px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  cursor: grab;
  /* Жест на самой ручке — только перетаскивание, прокрутка листа остаётся
     жестом по любому другому месту строки. */
  touch-action: none;
}
.grab:hover,
.grab:focus-visible {
  color: var(--accent);
}
.list.dragging .grab {
  cursor: grabbing;
}

.list.settling li {
  transition: none;
}

@media (prefers-reduced-motion: reduce) {
  .list li {
    transition: none;
  }
}
</style>
