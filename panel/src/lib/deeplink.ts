import { onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";

/* Ссылка внутрь раздела: `#/journal?focus=updates`.

   Нужна потому, что панель показывает состояние в одном месте, а правится оно в
   другом: режим UDP живёт на «Обзоре», список адресов — в «Правилах»; движок
   обхода DPI на «Обзоре», аргументы tpws — в «Журнале». Без такой ссылки
   read-only карточка оказывается тупиком: человек видит настройку и не знает,
   куда идти. Просто `router.push('/rules')` тоже не помогает — области свёрнуты,
   и нужная тонет среди девяти других.

   Поэтому ссылка не только уводит в раздел, но и раскрывает область и коротко её
   подсвечивает. */

/** Сколько держится подсветка. Достаточно, чтобы глаз поймал, и не настолько,
    чтобы мешать читать. */
const FLASH_MS = 1600;

export function focusFlash(el: HTMLElement) {
  el.classList.remove("focus-flash");
  /* Перезапустить анимацию на том же элементе можно только после того, как
     браузер увидел его без класса. */
  requestAnimationFrame(() => {
    el.classList.add("focus-flash");
    setTimeout(() => el.classList.remove("focus-flash"), FLASH_MS);
  });
}

/**
 * Подписывает раздел на `?focus=<key>`.
 *
 * @param open     раскрыть область по ключу — модель состояния у каждого раздела своя
 * @param anchorId id элемента, к которому прокручивать
 */
export function useFocusTarget(open: (key: string) => void, anchorId: (key: string) => string) {
  const route = useRoute();
  const router = useRouter();

  function apply(raw: unknown) {
    const key = typeof raw === "string" ? raw : "";
    if (!key) return;
    open(key);
    /* Область раскрывается по v-if, поэтому её высота известна только после
       перерисовки — иначе прокрутка уедет к прежнему положению. */
    requestAnimationFrame(() => {
      const el = document.getElementById(anchorId(key));
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "start" });
      focusFlash(el);
    });
    /* Убираем параметр из URL: иначе перезагрузка страницы или возврат назад
       снова дёрнут прокрутку, а свёрнутая вручную область снова раскроется. */
    void router.replace({ path: route.path, query: {} });
  }

  onMounted(() => apply(route.query.focus));
  /* Переход из другого раздела на уже открытый (палитра команд, ссылка в
     карточке) меняет только query — mount второй раз не случится. */
  watch(() => route.query.focus, apply);
}
