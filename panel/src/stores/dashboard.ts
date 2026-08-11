import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";

/* Состав и порядок карточек «Обзора». Хранится в браузере, а не на роутере:
   набор нужного отличается у телефона и у большого экрана, и это личная
   настройка того, кто смотрит, а не состояние роутера. Плата за это честная —
   на другом устройстве набор свой. */

export interface DashTile {
  id: string;
  title: string;
  hint: string;
  /** Занимает всю ширину сетки. */
  wide?: boolean;
}

/** Каталог в порядке по умолчанию. Порядок здесь = порядок на чистой панели. */
export const DASH_TILES: DashTile[] = [
  { id: "flow", title: "Поток трафика", hint: "схема: куда уходит трафик и в каких долях", wide: true },
  { id: "traffic", title: "Трафик по лентам", hint: "график за сутки и за месяц: сколько ушло напрямую, через VPN и в обход DPI", wide: true },
  { id: "connection", title: "Активное подключение", hint: "какой VPN включён, автозапуск, старт и стоп", wide: true },
  { id: "scope", title: "Область действия", hint: "«Все через VPN» и UDP через VPN" },
  { id: "bypass", title: "Обход DPI", hint: "движок, стратегия, автозапуск" },
  { id: "health", title: "Здоровье профилей", hint: "сколько профилей проходят проверку" },
  { id: "uplinks", title: "Каналы в интернет", hint: "провайдеры, скорость, канал для входящих" },
  { id: "routing", title: "Маршрутизация", hint: "режим туннеля, маршруты по сайтам, списки правил" },
  { id: "services", title: "Сервисы и доступ", hint: "сертификат, проброс портов, уведомления" },
  { id: "system", title: "Система", hint: "процессор, память, диск, устройства в сети" },
  { id: "versions", title: "Версии", hint: "версии панели и бинарников, обновления" },
];

const KEY = "detour:dashboard";
const IDS = DASH_TILES.map((t) => t.id);

interface Saved {
  order?: string[];
  hidden?: string[];
}

export const useDashboardStore = defineStore("dashboard", () => {
  const order = ref<string[]>([...IDS]);
  const hidden = ref<string[]>([]);

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
    } catch {
      /* Испорченная запись — не повод остаться без главной страницы. */
    }
  }

  read();

  watch(
    [order, hidden],
    () => {
      try {
        localStorage.setItem(
          KEY,
          JSON.stringify({ order: order.value, hidden: hidden.value } satisfies Saved),
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

  const visibleCount = computed(() => tiles.value.length - hidden.value.length);
  const customized = computed(
    () => hidden.value.length > 0 || order.value.join() !== IDS.join(),
  );

  function isVisible(id: string) {
    return !hidden.value.includes(id);
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

  function move(id: string, dir: -1 | 1) {
    const list = [...order.value];
    const i = list.indexOf(id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    order.value = list;
  }

  /** Перенос карточки на произвольное место — это перетаскивание в редакторе. */
  function reorder(from: number, to: number) {
    const list = [...order.value];
    if (from < 0 || from >= list.length || to < 0 || to >= list.length || from === to) return;
    const [id] = list.splice(from, 1);
    list.splice(to, 0, id);
    order.value = list;
  }

  function reset() {
    order.value = [...IDS];
    hidden.value = [];
  }

  return {
    order,
    hidden,
    tiles,
    visibleCount,
    customized,
    isVisible,
    orderOf,
    toggle,
    move,
    reorder,
    reset,
  };
});
