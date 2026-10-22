import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import { usePluginsStore } from "@/stores/plugins";

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
  /** Только на роутере: в приложении на устройстве этих подсистем нет. */
  router?: boolean;
}

/** Каталог в порядке по умолчанию. Порядок здесь = порядок на чистой панели. */
export const DASH_TILES: DashTile[] = [
  { id: "power", title: "Подключиться", hint: "большая кнопка: подключить выбранный профиль или отключиться", span: 6 },
  { id: "flow", title: "Поток трафика", hint: "схема: куда уходит трафик и в каких долях", span: 3 },
  { id: "traffic", title: "Трафик", hint: "скорость приёма и передачи сейчас и графиком; доли по направлениям за сутки и месяц", span: 3 },
  { id: "connection", title: "Активное подключение", hint: "какой VPN включён, автозапуск, старт и стоп", span: 4 },
  { id: "scope", title: "Область действия", hint: "«Все через VPN» и UDP через VPN", span: 2 },
  { id: "bypass", title: "Обход DPI", hint: "движок, стратегия, автозапуск", span: 2 },
  { id: "health", title: "Здоровье профилей", hint: "сколько профилей проходят проверку", span: 2 },
  { id: "uplinks", title: "Каналы в интернет", hint: "провайдеры, скорость, канал для входящих", span: 2, router: true },
  { id: "routing", title: "Маршрутизация", hint: "режим туннеля, маршруты по сайтам, списки правил", span: 2 },
  { id: "services", title: "Сервисы и доступ", hint: "сертификат, проброс портов, уведомления", span: 2, router: true },
  { id: "system", title: "Система", hint: "процессор, память, диск, устройства в сети", span: 2 },
  { id: "versions", title: "Версии", hint: "версии панели и бинарников, обновления", span: 2 },
];

const KEY = "detour:dashboard";
const CATALOG = __CLIENT__ ? DASH_TILES.filter((t) => !t.router) : DASH_TILES;
const ALLOWED: number[] = SPANS.map((s) => s.span);

interface Saved {
  order?: string[];
  hidden?: string[];
  spans?: Record<string, number>;
}

export const useDashboardStore = defineStore("dashboard", () => {
  const plugins = usePluginsStore();

  /* Каталог живой: к встроенным карточкам добавляются карточки установленных
     плагинов (id plugin:<id>), и появляются они не сразу, а когда придёт список
     плагинов. Поэтому сохранённое состояние хранится «сырым», с чужими id, и
     фильтруется по каталогу при чтении — иначе место и ширина карточки плагина
     терялись бы на каждой загрузке страницы, до прихода списка. */
  const catalog = computed<DashTile[]>(() => [
    ...CATALOG,
    ...plugins.tiles.map((t) => ({ id: t.id, title: t.title, hint: t.hint, span: t.span })),
  ]);
  const ids = computed(() => catalog.value.map((t) => t.id));
  const defaultSpans = computed<Record<string, number>>(() =>
    Object.fromEntries(catalog.value.map((t) => [t.id, t.span])),
  );

  const savedOrder = ref<string[]>([]);
  const savedHidden = ref<string[]>([]);
  const savedSpans = ref<Record<string, number>>({});

  function read() {
    try {
      const raw = localStorage.getItem(KEY);
      if (!raw) return;
      const saved = JSON.parse(raw) as Saved;
      savedOrder.value = Array.isArray(saved.order) ? saved.order.filter((x) => typeof x === "string") : [];
      savedHidden.value = Array.isArray(saved.hidden) ? saved.hidden.filter((x) => typeof x === "string") : [];
      /* Размеры не из набора отбрасываем: запись правится руками из консоли не
         реже, чем самой панелью. */
      const sp: Record<string, number> = {};
      for (const [id, n] of Object.entries(saved.spans ?? {})) {
        if (ALLOWED.includes(Number(n))) sp[id] = Number(n);
      }
      savedSpans.value = sp;
    } catch {
      /* Испорченная запись — не повод остаться без главной страницы. */
    }
  }

  read();

  watch(
    [savedOrder, savedHidden, savedSpans],
    () => {
      try {
        localStorage.setItem(
          KEY,
          JSON.stringify({
            order: savedOrder.value,
            hidden: savedHidden.value,
            spans: savedSpans.value,
          } satisfies Saved),
        );
      } catch {
        /* Приватный режим/переполненное хранилище: настройка просто не переживёт
           перезагрузку — ломать из-за этого экран нечем. */
      }
    },
    { deep: true },
  );

  /* Сохранённый порядок по каталогу, а недостающие карточки — на их место по
     умолчанию: иначе плитка, появившаяся в новой версии панели (или новый
     плагин), не показалась бы никому, кто хоть раз менял состав. */
  const order = computed<string[]>(() => {
    const known = ids.value;
    const next = savedOrder.value.filter((id) => known.includes(id));
    known.forEach((id, i) => {
      if (!next.includes(id)) next.splice(Math.min(i, next.length), 0, id);
    });
    return next;
  });
  const hidden = computed(() => savedHidden.value.filter((id) => ids.value.includes(id)));

  /** Карточки в выбранном порядке — для редактора состава. */
  const tiles = computed<DashTile[]>(() =>
    order.value
      .map((id) => catalog.value.find((t) => t.id === id))
      .filter((t): t is DashTile => !!t),
  );

  /** Видимые карточки в выбранном порядке — то, что реально лежит на поле. */
  const visible = computed<DashTile[]>(() => tiles.value.filter((t) => isVisible(t.id)));
  const visibleCount = computed(() => visible.value.length);
  const customized = computed(
    () =>
      hidden.value.length > 0 ||
      order.value.join() !== ids.value.join() ||
      ids.value.some((id) => spanOf(id) !== defaultSpans.value[id]),
  );

  function isVisible(id: string) {
    return ids.value.includes(id) && !savedHidden.value.includes(id);
  }

  function titleOf(id: string) {
    return catalog.value.find((t) => t.id === id)?.title ?? id;
  }

  function spanOf(id: string) {
    return savedSpans.value[id] ?? defaultSpans.value[id] ?? 2;
  }

  function setSpan(id: string, span: number) {
    if (!ALLOWED.includes(span)) return;
    savedSpans.value = { ...savedSpans.value, [id]: span };
  }

  /** CSS-порядок в гриде: сама сетка остаётся сеткой, меняется только место. */
  function orderOf(id: string) {
    const i = order.value.indexOf(id);
    return i < 0 ? 99 : i;
  }

  function toggle(id: string) {
    savedHidden.value = isVisible(id)
      ? [...savedHidden.value, id]
      : savedHidden.value.filter((x) => x !== id);
  }

  /* Записываем порядок известных карточек, а id, которых сейчас в каталоге нет
     (плагин выключен), дописываем в хвост — они вернутся на своё место. */
  function writeOrder(list: string[]) {
    savedOrder.value = [...list, ...savedOrder.value.filter((id) => !list.includes(id))];
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
    writeOrder(list);
  }

  function reset() {
    savedOrder.value = [];
    savedHidden.value = [];
    savedSpans.value = {};
  }

  return {
    order,
    hidden,
    tiles,
    visible,
    visibleCount,
    customized,
    isVisible,
    titleOf,
    orderOf,
    spanOf,
    setSpan,
    toggle,
    move,
    place,
    reset,
  };
});
