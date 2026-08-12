import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";

/** Ширина элемента в CSS-пикселях, живая (ResizeObserver).
 *
 * Нужна там, где SVG обязан РАСТЯГИВАТЬСЯ, но не МАСШТАБИРОВАТЬСЯ. Обычный
 * приём «viewBox фиксированной ширины + width:100%» даёт одно из двух зол: с
 * `preserveAspectRatio="none"` картинка тянется по X и текст с обводками
 * выходит сплющенным, а без него — вся схема пропорционально раздувается и
 * подписи на широком экране становятся вдвое крупнее остального интерфейса.
 * Меряем ширину и рисуем в пикселях: единица viewBox = единица экрана, шрифты
 * и обводки остаются такими, какими заданы.
 *
 * @param fallback ширина до первого замера (SSR/первый кадр).
 */
export function useElementWidth(el: Ref<Element | null>, fallback = 720) {
  const width = ref(fallback);
  let ro: ResizeObserver | null = null;
  let onResize: (() => void) | null = null;

  onMounted(() => {
    if (!el.value) return;
    const measure = () => {
      const w = el.value?.getBoundingClientRect().width ?? 0;
      /* Ноль приходит, когда элемент скрыт (display:none на телефоне) —
         оставляем последнюю осмысленную ширину, иначе геометрия схлопнется. */
      if (w > 1) width.value = Math.round(w);
    };
    measure();
    if (typeof ResizeObserver === "function") {
      ro = new ResizeObserver(measure);
      ro.observe(el.value);
    } else {
      onResize = measure;
      addEventListener("resize", measure);
    }
  });

  onBeforeUnmount(() => {
    ro?.disconnect();
    if (onResize) removeEventListener("resize", onResize);
  });

  return width;
}
