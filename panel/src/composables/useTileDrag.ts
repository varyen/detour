import { onBeforeUnmount, ref, type Ref } from "vue";

/* Перетаскивание карточек по сетке «Обзора».
 *
 * Тянем на pointer-событиях, а не на HTML5 drag&drop: последний на тач-экранах
 * просто не работает, а редактировать главную с телефона надо.
 *
 * Пока карточку несут, сетка НЕ перестраивается: живая перестановка в
 * многоколоночной сетке заставляет прыгать сразу весь ряд, и целиться в такое
 * невозможно. Вместо этого поднятая карточка едет за курсором, а место
 * приземления подсвечивается на соседе. Порядок в сторе меняется один раз —
 * когда отпустили.
 */
export function useTileDrag(
  container: Ref<HTMLElement | null>,
  onDrop: (id: string, toIndex: number) => void,
) {
  const dragId = ref<string | null>(null);
  const dropIndex = ref(-1);
  const dx = ref(0);
  const dy = ref(0);

  let from = -1;
  let boxes: { id: string; top: number; left: number; cx: number; cy: number }[] = [];
  let startX = 0;
  let startY = 0;
  let pointerId = -1;

  /* Порядок снимка — ВИЗУАЛЬНЫЙ (сверху вниз, слева направо), а не в разметке:
     место карточки в сетке задаёт CSS `order`, поэтому querySelectorAll отдаёт
     их в порядке исходника. Индекс приземления уедет на чужую карточку, если
     это перепутать. Внутри ряда `top` у карточек совпадает точно — сетка
     выравнивает строки, — так что сортировка по (top, left) честная. */
  function snapshot() {
    const el = container.value;
    if (!el) return;
    boxes = (Array.from(el.querySelectorAll("[data-tile]")) as HTMLElement[])
      .map((n) => {
        const r = n.getBoundingClientRect();
        return {
          id: n.dataset.tile!,
          top: r.top,
          left: r.left,
          cx: r.left + r.width / 2,
          cy: r.top + r.height / 2,
        };
      })
      .sort((a, b) => a.top - b.top || a.left - b.left);
  }

  function grab(e: PointerEvent, id: string) {
    /* Только основная кнопка и только один палец: второй не перехватывает
       перенос у первого. */
    if (e.button > 0 || pointerId !== -1) return;
    snapshot();
    from = boxes.findIndex((b) => b.id === id);
    if (from < 0) return;
    dragId.value = id;
    dropIndex.value = from;
    dx.value = 0;
    dy.value = 0;
    startX = e.clientX;
    startY = e.clientY;
    pointerId = e.pointerId;
    addEventListener("pointermove", move, { passive: false });
    addEventListener("pointerup", drop);
    addEventListener("pointercancel", drop);
    e.preventDefault();
  }

  function move(e: PointerEvent) {
    if (e.pointerId !== pointerId) return;
    dx.value = e.clientX - startX;
    dy.value = e.clientY - startY;
    /* Ближайший центр — по обеим осям сразу: сетка двумерная, и «мимо ряда»
       здесь такая же ошибка, как «мимо колонки». Вертикаль весит больше, иначе
       карточка, поднесённая к краю широкого соседа снизу, цепляется за свой
       же ряд. */
    const px = e.clientX;
    const py = e.clientY;
    let best = from;
    let bestD = Infinity;
    boxes.forEach((b, i) => {
      const d = (b.cx - px) ** 2 + ((b.cy - py) * 1.6) ** 2;
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    });
    dropIndex.value = best;
    e.preventDefault();
  }

  function stop() {
    removeEventListener("pointermove", move);
    removeEventListener("pointerup", drop);
    removeEventListener("pointercancel", drop);
    pointerId = -1;
    dragId.value = null;
    dropIndex.value = -1;
    dx.value = 0;
    dy.value = 0;
    from = -1;
  }

  function drop(e: PointerEvent) {
    if (e.pointerId !== pointerId) return;
    const id = dragId.value;
    const to = dropIndex.value;
    stop();
    if (id && to >= 0) onDrop(id, to);
  }

  onBeforeUnmount(stop);

  return { dragId, dropIndex, dx, dy, grab };
}
