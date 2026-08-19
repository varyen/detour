import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";

/* Состав и порядок карточек «Обзора». Хранится в браузере, а не на роутере:
   набор нужного отличается у телефона и у большого экрана, и это личная
   настройка того, кто смотрит, а не состояние роутера. Плата за это честная —
   на другом устройстве набор свой. */

/* Ширина карточки — в колонках шестиколоночной сетки. Шесть, а не три: только
   так выражаются и трети (2), и половины (3), и две трети (4). Пятая и первая
   доли не предлагаются — карточка уже 240 px не выживает по содержимому. */
export const SPANS = [
  { span: 2, label: "⅓", title: "Треть ряда" },
  { span: 3, label: "½", title: "Половина ряда" },
  { span: 4, label: "⅔", title: "Две трети ряда" },
  { span: 6, label: "1", title: "Весь ряд" },
] as const;

export const COLUMNS = 6;

export interface DashTile {
  id: string;
  title: string;
  hint: string;
  /** Ширина по умолчанию, в колонках из шести. */
  span: number;
}

/** Каталог в порядке по умолчанию. Порядок здесь = порядок на чистой панели. */
export const DASH_TILES: DashTile[] = [
  { id: "flow", title: "Поток трафика", hint: "схема: куда уходит трафик и в каких долях", span: 3 },
  { id: "traffic", title: "Трафик", hint: "скорость приёма и передачи сейчас и графиком; доли по направлениям за сутки и месяц", span: 3 },
  { id: "connection", title: "Активное подключение", hint: "какой VPN включён, автозапуск, старт и стоп", span: 4 },
  { id: "scope", title: "Область действия", hint: "«Все через VPN» и UDP через VPN", span: 2 },
  { id: "bypass", title: "Обход DPI", hint: "движок, стратегия, автозапуск", span: 2 },
  { id: "health", title: "Здоровье профилей", hint: "сколько профилей проходят проверку", span: 2 },
  { id: "uplinks", title: "Каналы в интернет", hint: "провайдеры, скорость, канал для входящих", span: 2 },
  { id: "routing", title: "Маршрутизация", hint: "режим туннеля, маршруты по сайтам, списки правил", span: 2 },
  { id: "services", title: "Сервисы и доступ", hint: "сертификат, проброс портов, уведомления", span: 2 },
  { id: "system", title: "Система", hint: "процессор, память, диск, устройства в сети", span: 2 },
  { id: "versions", title: "Версии", hint: "версии панели и бинарников, обновления", span: 2 },
];

const KEY = "detour:dashboard";
const IDS = DASH_TILES.map((t) => t.id);

const DEFAULT_SPANS: Record<string, number> = Object.fromEntries(
  DASH_TILES.map((t) => [t.id, t.span]),
);
const ALLOWED: number[] = SPANS.map((s) => s.span);

interface Saved {
  order?: string[];
  hidden?: string[];
  spans?: Record<string, number>;
}

export const useDashboardStore = defineStore("dashboard", () => {
  const order = ref<string[]>([...IDS]);
  const hidden = ref<string[]>([]);
  const spans = ref<Record<string, number>>({ ...DEFAULT_SPANS });

  function read() {
    try {
      const raw = localStorage.getItem(KEY);
      if (!raw) return;
      const saved = JSON.parse(raw) as Saved;
      /* Сохранённый порядок фильтруем по каталогу, а недостающие карточки
         дописываем на их место по умолчанию: иначе плитка, появившаяся в новой
         версии панели, не показалась бы никому, кто хоть раз менял состав. */
      const known = (saved.order ?? []).filter((id) => IDS.includes(id));
      const rest = IDS.filter((id) => !known.includes(id));
      order.value = [...known, ...rest];
      hidden.value = (saved.hidden ?? []).filter((id) => IDS.includes(id));
      /* Ширины пришли позже порядка: у того, кто настраивал главную до этой
         версии, их в записи нет — берём каталожные, а не нули. Чужие id и
         размеры не из набора отбрасываем: запись правится руками из консоли
         не реже, чем самой панелью. */
      const savedSpans = saved.spans ?? {};
      spans.value = { ...DEFAULT_SPANS };
      for (const id of IDS) {
        const n = Number(savedSpans[id]);
        if (ALLOWED.includes(n)) spans.value[id] = n;
      }
    } catch {
      /* Испорченная запись — не повод остаться без главной страницы. */
    }
  }

  read();

  watch(
    [order, hidden, spans],
    () => {
      try {
        localStorage.setItem(
          KEY,
          JSON.stringify({
            order: order.value,
            hidden: hidden.value,
            spans: spans.value,
          } satisfies Saved),
        );
      } catch {
        /* Приватный режим/переполненное хранилище: настройка просто не переживёт
           перезагрузку — ломать из-за этого экран нечем. */
      }
    },
    { deep: true },
  );

  /** Карточки в выбранном порядке — для редактора состава. */
  const tiles = computed<DashTile[]>(() =>
    order.value
      .map((id) => DASH_TILES.find((t) => t.id === id))
      .filter((t): t is DashTile => !!t),
  );

  /** Видимые карточки в выбранном порядке — то, что реально лежит на поле. */
  const visible = computed<DashTile[]>(() => tiles.value.filter((t) => isVisible(t.id)));
  const visibleCount = computed(() => visible.value.length);
  const customized = computed(
    () =>
      hidden.value.length > 0 ||
      order.value.join() !== IDS.join() ||
      IDS.some((id) => spans.value[id] !== DEFAULT_SPANS[id]),
  );

  function isVisible(id: string) {
    return !hidden.value.includes(id);
  }

  function spanOf(id: string) {
    return spans.value[id] ?? DEFAULT_SPANS[id] ?? 2;
  }

  function setSpan(id: string, span: number) {
    if (!ALLOWED.includes(span)) return;
    spans.value = { ...spans.value, [id]: span };
  }

  /** CSS-порядок в гриде: сама сетка остаётся сеткой, меняется только место. */
  function orderOf(id: string) {
    const i = order.value.indexOf(id);
    return i < 0 ? 99 : i;
  }

  function toggle(id: string) {
    hidden.value = isVisible(id)
      ? [...hidden.value, id]
      : hidden.value.filter((x) => x !== id);
  }

  /* Перемещения считаются по ВИДИМЫМ соседям: скрытая карточка не должна
     съедать нажатие «сдвинуть вправо», иначе на поле ничего не происходит.
     Скрытые при этом сохраняют своё место в общем списке — вернув карточку,
     её находят там, где оставили. */
  function move(id: string, dir: -1 | 1) {
    const vis = visible.value.map((t) => t.id);
    const i = vis.indexOf(id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= vis.length) return;
    place(id, j);
  }

  /** Перенос карточки на произвольное место среди видимых — это перетаскивание. */
  function place(id: string, toVisibleIndex: number) {
    const vis = visible.value.map((t) => t.id);
    const from = vis.indexOf(id);
    const to = Math.min(vis.length - 1, Math.max(0, toVisibleIndex));
    if (from < 0 || from === to) return;
    const rest = vis.filter((x) => x !== id);
    /* Место назначения задаётся соседом, а не индексом в общем списке: между
       видимыми могут лежать скрытые, и «встать пятым» значит «встать перед тем,
       кто сейчас пятый среди видимых». */
    const anchor = rest[to] ?? null;
    const list = order.value.filter((x) => x !== id);
    const at = anchor ? list.indexOf(anchor) : list.length;
    list.splice(at < 0 ? list.length : at, 0, id);
    order.value = list;
  }

  function reset() {
    order.value = [...IDS];
    hidden.value = [];
    spans.value = { ...DEFAULT_SPANS };
  }

  return {
    order,
    hidden,
    spans,
    tiles,
    visible,
    visibleCount,
    customized,
    isVisible,
    orderOf,
    spanOf,
    setSpan,
    toggle,
    move,
    place,
    reset,
  };
});
