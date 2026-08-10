<script setup lang="ts">
/* Список профилей. Их бывает под сотню, поэтому здесь всё, что помогает
   быстро найти нужный: поиск, фильтр по группе, сортировка по колонкам и
   массовое выделение с диапазоном по Shift.

   Разметка одна на все экраны: на широком это таблица (колонки заданы
   grid-template-columns), на телефоне те же строки превращаются в карточки —
   таблица с горизонтальной прокруткой на 360px нечитаема. */
import { computed, ref, watch } from "vue";
import SectionIcon from "@/components/SectionIcon.vue";
import type { ProfileRow } from "@/stores/profiles";
import { fmtAgo, fmtSpeedKbps } from "@/lib/format";

const props = defineProps<{
  rows: ProfileRow[];
  selected: string[];
  switching: string;
  /** Идентификатор профиля, по которому сейчас идёт проверка. */
  probing: string;
  /**
   * Цели функциональной проверки в том же порядке, в каком роутер складывает
   * `health.delays` — только по ним и можно сказать, ЧТО именно не открылось.
   */
  targets?: { label: string; url: string }[];
  /** Какой флаг сейчас сохраняется: `<id>:autoswitch` / `<id>:speedcheck`. */
  flagBusy?: string;
}>();

const emit = defineEmits<{
  "update:selected": [string[]];
  open: [ProfileRow];
  connect: [ProfileRow];
  ping: [ProfileRow];
  health: [ProfileRow];
  flag: [{ row: ProfileRow; kind: "autoswitch" | "speedcheck"; value: boolean }];
}>();

type SortKey = "name" | "type" | "group" | "ping" | "speed" | "state";

const query = ref("");
const group = ref("");
const sortKey = ref<SortKey>("name");
const sortAsc = ref(true);
let lastIndex = -1;

const STATE_TEXT: Record<string, string> = {
  ok: "проверка проходит",
  slow: "отвечает медленно",
  dead: "не отвечает",
  unknown: "не проверялся",
};

const STATE_ORDER: Record<string, number> = { ok: 0, slow: 1, unknown: 2, dead: 3 };

const groups = computed(() => {
  const set = new Set<string>();
  for (const r of props.rows) set.add(r.group || "Без группы");
  return [...set].sort((a, b) => a.localeCompare(b, "ru"));
});

const visible = computed(() => {
  const q = query.value.trim().toLowerCase();
  const g = group.value;
  const out = props.rows.filter((r) => {
    if (g && (r.group || "Без группы") !== g) return false;
    if (!q) return true;
    /* Адрес узла тоже ищем: у подписок имена профилей похожи как две капли, и
       найти нужный проще по хосту. */
    return `${r.name} ${r.type} ${r.group ?? ""} ${r.id} ${r.ping?.server ?? ""}`
      .toLowerCase()
      .includes(q);
  });
  const dir = sortAsc.value ? 1 : -1;
  return [...out].sort((a, b) => {
    switch (sortKey.value) {
      case "type":
        return dir * (a.type ?? "").localeCompare(b.type ?? "", "ru");
      case "group":
        return (
          dir * (a.group || "").localeCompare(b.group || "", "ru") ||
          a.name.localeCompare(b.name, "ru", { numeric: true })
        );
      case "ping": {
        /* Профили без замера всегда внизу: сортировка по пингу нужна, чтобы
           наверху был самый быстрый, а не самый неизвестный. */
        const av = a.ping?.ok ? (a.ping.rtt ?? 1e8) : 1e9;
        const bv = b.ping?.ok ? (b.ping.rtt ?? 1e8) : 1e9;
        return dir * (av - bv);
      }
      case "speed": {
        /* Неизмеренные тонут вниз в любом направлении: смысл сортировки по
           скорости — увидеть самый быстрый канал, а не самый неизвестный. */
        const av = speedKbps(a);
        const bv = speedKbps(b);
        if (av <= 0 || bv <= 0) {
          if (av <= 0 && bv <= 0) return a.name.localeCompare(b.name, "ru", { numeric: true });
          return av <= 0 ? 1 : -1;
        }
        return dir * (av - bv) || a.name.localeCompare(b.name, "ru", { numeric: true });
      }
      case "state":
        return dir * ((STATE_ORDER[a.state] ?? 9) - (STATE_ORDER[b.state] ?? 9));
      default:
        return dir * a.name.localeCompare(b.name, "ru", { numeric: true });
    }
  });
});

/* Фильтр поменялся — прежний якорь диапазона указывает не туда. */
watch([query, group, sortKey, sortAsc], () => {
  lastIndex = -1;
});

const allChecked = computed(
  () => visible.value.length > 0 && visible.value.every((r) => props.selected.includes(r.id)),
);

function toggleAll() {
  const ids = visible.value.map((r) => r.id);
  if (allChecked.value) {
    emit(
      "update:selected",
      props.selected.filter((id) => !ids.includes(id)),
    );
  } else {
    emit("update:selected", [...new Set([...props.selected, ...ids])]);
  }
  lastIndex = -1;
}

function toggle(index: number, e: MouseEvent) {
  const row = visible.value[index];
  if (!row) return;
  const set = new Set(props.selected);
  const on = !set.has(row.id);
  if (e.shiftKey && lastIndex >= 0) {
    const [from, to] = index < lastIndex ? [index, lastIndex] : [lastIndex, index];
    for (let i = from; i <= to; i++) {
      const id = visible.value[i]?.id;
      if (!id) continue;
      if (on) set.add(id);
      else set.delete(id);
    }
  } else if (on) {
    set.add(row.id);
  } else {
    set.delete(row.id);
  }
  lastIndex = index;
  emit("update:selected", [...set]);
}

function sortBy(key: SortKey) {
  if (sortKey.value === key) sortAsc.value = !sortAsc.value;
  else {
    sortKey.value = key;
    /* У скорости «по возрастанию» бесполезно: от колонки ждут, что сверху
       окажется самый быстрый профиль. */
    sortAsc.value = key !== "speed";
  }
}

function pingText(r: ProfileRow): string {
  if (props.probing === r.id) return "проверяю…";
  if (!r.ping || r.ping.ok === undefined) return "—";
  if (!r.ping.ok) return "нет ответа";
  /* rtt = -1 приходит, когда узел ответил, но замерить время не вышло. */
  return (r.ping.rtt ?? -1) > 0 ? `${Math.round(r.ping.rtt as number)} мс` : "отвечает";
}

/**
 * Адрес узла профиля. Берётся из кэша пингов: `profiles_list` адреса не отдаёт
 * (это сокращённая запись), а вычитывать сотню профилей поштучно ради одной
 * колонки — сотня запросов на открытие экрана. Порт роутер в этом кэше не
 * хранит, поэтому показываем только хост, и до первого пинга — прочерк.
 */
function serverText(r: ProfileRow): string {
  return r.ping?.server || "—";
}

function healthText(r: ProfileRow): string {
  const h = r.health;
  if (!h || h.ok === undefined) return "—";
  const when = fmtAgo(h.ts);
  return `${h.ok ? "проходит" : "не проходит"}${when ? ` · ${when}` : ""}`;
}

/** Скорость в кбит/с или -1: роутер отдаёт -1/0, когда замера не было. */
function speedKbps(r: ProfileRow): number {
  const v = r.health?.dl;
  return typeof v === "number" && Number.isFinite(v) ? v : -1;
}

function speedText(r: ProfileRow): string {
  const s = fmtSpeedKbps(speedKbps(r));
  return s ? `↓ ${s}` : "—";
}

function speedTitle(r: ProfileRow): string {
  const s = fmtSpeedKbps(speedKbps(r));
  if (!s) {
    return r.speedcheck === false
      ? "скорость ещё не измерена · профиль исключён из фоновых замеров"
      : "скорость ещё не измерена";
  }
  const when = fmtAgo(r.health?.ts);
  return `скачивание через профиль: ${s}${when ? ` · замер ${when}` : ""}`;
}

/**
 * Расшифровка проверки по каждой цели: «YouTube 120 мс · Google не отвечает».
 * Без неё «не проходит» ничего не объясняет — а delays роутер отдаёт всегда.
 */
function healthTitle(r: ProfileRow): string {
  const h = r.health;
  if (!h || h.ok === undefined) return "проверка работоспособности ещё не выполнялась";
  const delays = h.delays ?? [];
  const parts = delays.map((d, i) => {
    const label = props.targets?.[i]?.label || `цель ${i + 1}`;
    return typeof d === "number" && d >= 0 ? `${label} ${Math.round(d)} мс` : `${label} не отвечает`;
  });
  const speed = fmtSpeedKbps(speedKbps(r));
  const when = fmtAgo(h.ts);
  return [parts.join(" · ") || "нет данных", speed ? `↓ ${speed}` : "", when].filter(Boolean).join(" · ");
}

function flagBusyFor(r: ProfileRow, kind: "autoswitch" | "speedcheck"): boolean {
  return props.flagBusy === `${r.id}:${kind}`;
}
</script>

<template>
  <div class="wrap">
    <div class="tools">
      <label class="search">
        <SectionIcon name="search" :size="14" />
        <input
          v-model="query"
          type="search"
          placeholder="Имя, протокол, группа"
          autocomplete="off"
          aria-label="Поиск профиля"
        />
      </label>
      <select v-model="group" aria-label="Группа">
        <option value="">Все группы</option>
        <option v-for="g in groups" :key="g" :value="g">{{ g }}</option>
      </select>
      <select
        :value="sortKey"
        aria-label="Сортировка"
        class="sortsel"
        @change="sortBy(($event.target as HTMLSelectElement).value as SortKey)"
      >
        <option value="name">По имени</option>
        <option value="type">По протоколу</option>
        <option value="group">По группе</option>
        <option value="ping">По пингу</option>
        <option value="speed">По скорости</option>
        <option value="state">По состоянию</option>
      </select>
    </div>

    <p class="count">
      {{ visible.length }} из {{ rows.length }}<template v-if="selected.length">
        · выбрано {{ selected.length }}</template>
    </p>

    <div class="head" role="row">
      <label class="chk">
        <input
          type="checkbox"
          :checked="allChecked"
          aria-label="Выделить всё видимое"
          @change="toggleAll"
        />
      </label>
      <button type="button" :aria-pressed="sortKey === 'name'" @click="sortBy('name')">
        Имя
      </button>
      <button
        class="h-type"
        type="button"
        :aria-pressed="sortKey === 'type'"
        @click="sortBy('type')"
      >
        Протокол
      </button>
      <button
        class="h-grp"
        type="button"
        :aria-pressed="sortKey === 'group'"
        @click="sortBy('group')"
      >
        Группа
      </button>
      <!-- Не кнопка: сортировать по адресу узла незачем, а колонка живёт только
           на широком экране — её данные приходят из кэша пингов. -->
      <span class="h-srv">Сервер</span>
      <button type="button" :aria-pressed="sortKey === 'ping'" @click="sortBy('ping')">
        Пинг
      </button>
      <button type="button" :aria-pressed="sortKey === 'speed'" @click="sortBy('speed')">
        Скорость
      </button>
      <button
        class="h-chk"
        type="button"
        :aria-pressed="sortKey === 'state'"
        @click="sortBy('state')"
      >
        Проверка
      </button>
      <span></span>
    </div>

    <div class="list">
      <p v-if="!visible.length" class="empty">Ничего не нашлось</p>

      <div
        v-for="(r, i) in visible"
        :key="r.id"
        class="row"
        :class="{ on: r.isActive, sel: selected.includes(r.id) }"
      >
        <label class="chk">
          <input
            type="checkbox"
            :checked="selected.includes(r.id)"
            :aria-label="`Выбрать ${r.name}`"
            @click.stop="toggle(i, $event as MouseEvent)"
          />
        </label>

        <button class="nm" type="button" @click="emit('open', r)">
          <i class="hdot" :class="`h-${r.state}`" :title="STATE_TEXT[r.state]"></i>
          <span class="nm-t">
            {{ r.name }}
            <small>
              {{ r.type }}<template v-if="r.group"> · {{ r.group }}</template>
              <!-- На телефоне колонки скрыты, а скорость — то, ради чего в этот
                   список и заходят: показываем её прямо в подписи. -->
              <span v-if="speedKbps(r) > 0" class="only-mob"> · {{ speedText(r) }}</span>
              <template v-if="r.autoswitch === false"> · без авто-переключения</template>
              <template v-if="r.speedcheck === false"> · без проверки скорости</template>
            </small>
          </span>
          <span v-if="r.isActive" class="badge">активен</span>
        </button>

        <span class="cell type">{{ r.type }}</span>
        <span class="cell grp">{{ r.group || "—" }}</span>
        <span class="cell srv mono" :title="serverText(r)">{{ serverText(r) }}</span>
        <span class="cell num">{{ pingText(r) }}</span>
        <span
          class="cell num speed"
          :class="{ measured: speedKbps(r) > 0 }"
          :title="speedTitle(r)"
        >
          {{ speedText(r) }}
        </span>
        <span class="cell chk-t" :title="healthTitle(r)">{{ healthText(r) }}</span>

        <span class="act">
          <button
            class="ico"
            type="button"
            role="switch"
            :aria-checked="r.autoswitch !== false"
            :class="{ on: r.autoswitch !== false }"
            :disabled="flagBusyFor(r, 'autoswitch')"
            :title="
              r.autoswitch !== false
                ? 'Участвует в авто-переключении — нажмите, чтобы исключить'
                : 'Исключён из авто-переключения — нажмите, чтобы вернуть'
            "
            @click="emit('flag', { row: r, kind: 'autoswitch', value: r.autoswitch === false })"
          >
            <SectionIcon name="route" :size="15" />
          </button>
          <button
            class="ico"
            type="button"
            role="switch"
            :aria-checked="r.speedcheck !== false"
            :class="{ on: r.speedcheck !== false }"
            :disabled="flagBusyFor(r, 'speedcheck')"
            :title="
              r.speedcheck !== false
                ? 'Участвует в фоновых замерах скорости — нажмите, чтобы исключить'
                : 'Исключён из фоновых замеров скорости — нажмите, чтобы вернуть'
            "
            @click="emit('flag', { row: r, kind: 'speedcheck', value: r.speedcheck === false })"
          >
            <span class="glyph" aria-hidden="true">↓</span>
          </button>
          <button
            class="ico"
            type="button"
            :disabled="!!probing"
            :class="{ busy: probing === r.id }"
            title="Проверить пинг сервера"
            aria-label="Проверить пинг"
            @click="emit('ping', r)"
          >
            <SectionIcon name="refresh" :size="15" />
          </button>
          <button
            class="ico"
            type="button"
            :disabled="!!probing"
            :class="{ busy: probing === r.id }"
            title="Проверить работу и скорость через этот профиль"
            aria-label="Проверить работу и скорость"
            @click="emit('health', r)"
          >
            <SectionIcon name="gauge" :size="15" />
          </button>
          <button
            class="mini"
            type="button"
            :disabled="!!switching || r.isActive"
            :title="r.isActive ? 'Уже подключён' : 'Подключить'"
            @click="emit('connect', r)"
          >
            {{ switching === r.id ? "включаю…" : "Подключить" }}
          </button>
          <button class="dots" type="button" aria-label="Действия" @click="emit('open', r)">
            …
          </button>
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.wrap {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
.tools {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.search {
  display: flex;
  align-items: center;
  gap: 9px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  padding: 6px 12px;
  background: var(--panel-2);
  color: var(--faint);
  flex: 1 1 220px;
  min-width: 0;
  min-height: 44px;
}
.search input {
  border: 0;
  background: transparent;
  outline: none;
  width: 100%;
  min-width: 0;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  color: var(--ink);
}
.tools select {
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 8px 10px;
  font-size: 16px;
  min-height: 44px;
  max-width: 100%;
}
.count {
  font-size: 12px;
  color: var(--faint);
}
.head,
.row {
  display: grid;
  grid-template-columns:
    38px minmax(0, 2.4fr) 92px minmax(0, 1fr) minmax(0, 1.25fr)
    78px 104px 132px auto;
  align-items: center;
  gap: 6px;
}
.head {
  padding: 0 6px 6px;
  border-bottom: 1px solid var(--line);
}
.head button,
.head .h-srv {
  border: 0;
  background: transparent;
  padding: 4px 2px;
  text-align: left;
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--faint);
  font-weight: 600;
}
.head button[aria-pressed="true"] {
  color: var(--accent);
}
.list {
  /* Сотня профилей не должна тянуть страницу — крутим внутри контейнера. */
  max-height: min(62vh, 620px);
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  min-width: 0;
}
.empty {
  color: var(--dim);
  font-size: 14px;
  padding: 14px 6px;
}
.row {
  border-bottom: 1px solid var(--line);
  padding: 3px 6px;
}
.row.sel {
  background: var(--panel-2);
}
.row.on {
  background: var(--accent-wash);
}
.chk {
  display: grid;
  place-items: center;
  min-height: 44px;
  min-width: 38px;
}
.chk input {
  width: 18px;
  height: 18px;
  accent-color: var(--accent);
}
.nm {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 0;
  background: transparent;
  text-align: left;
  min-width: 0;
  min-height: 44px;
  padding: 4px 2px;
}
.nm-t {
  min-width: 0;
  font-size: 14.5px;
  overflow: hidden;
}
.nm-t small {
  display: block;
  font-size: 11.5px;
  color: var(--faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.badge {
  font-size: 10.5px;
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 999px;
  padding: 1px 7px;
  white-space: nowrap;
}
.cell {
  font-size: 12.5px;
  color: var(--dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* Адрес узла длиннее любой другой ячейки, поэтому мельче: он нужен, чтобы
   отличить два одноимённых профиля подписки, а не для чтения вслух. */
.cell.srv {
  font-size: 11.5px;
  color: var(--faint);
}
/* Измеренная скорость — единственное число в строке, которое сравнивают
   глазами между строками, поэтому выделяем её из общего серого. */
.cell.speed.measured {
  color: var(--ink);
  font-variant-numeric: tabular-nums;
}
.only-mob {
  display: none;
}
.act {
  display: flex;
  gap: 6px;
  align-items: center;
  justify-content: flex-end;
}
.mini {
  border: 1px solid var(--line-2);
  background: transparent;
  border-radius: var(--radius-sm);
  padding: 6px 10px;
  font-size: 12.5px;
  color: var(--ink);
  min-height: 36px;
  white-space: nowrap;
}
.mini:hover:not(:disabled) {
  background: var(--accent-wash);
  border-color: var(--accent);
}
.mini:disabled {
  opacity: 0.4;
}
.dots {
  border: 1px solid var(--line-2);
  background: transparent;
  border-radius: var(--radius-sm);
  width: 36px;
  min-height: 36px;
  color: var(--dim);
  line-height: 1;
}
/* Иконочные кнопки строки: два флага-тумблера (участие в авто-переключении и в
   фоновых замерах) и две разовые проверки. Узкие, потому что их четыре в каждой
   из сотни строк; на телефоне ниже вырастают до 44px под палец. */
.ico {
  display: grid;
  place-items: center;
  border: 1px solid var(--line-2);
  background: transparent;
  border-radius: var(--radius-sm);
  width: 32px;
  min-height: 32px;
  padding: 0;
  color: var(--faint);
  flex: none;
}
.ico.on {
  color: var(--accent);
  border-color: var(--accent);
  background: var(--accent-wash);
}
.ico:hover:not(:disabled) {
  border-color: var(--accent);
}
.ico:disabled {
  opacity: 0.4;
}
.ico .glyph {
  font-size: 14px;
  line-height: 1;
}
.ico.busy {
  animation: blink 1s ease-in-out infinite;
}
@keyframes blink {
  50% {
    opacity: 0.35;
  }
}

/* Узкий десктоп и планшет: протокол с группой уже написаны в подписи под
   именем, а адрес узла — справочная колонка, поэтому все три уходят первыми:
   иначе имя сжимается в кашу. */
@media (max-width: 1180px) {
  .head,
  .row {
    grid-template-columns: 38px minmax(0, 2.4fr) 78px 104px 132px auto;
  }
  .head .h-type,
  .head .h-grp,
  .head .h-srv,
  .cell.type,
  .cell.grp,
  .cell.srv {
    display: none;
  }
}

@media (max-width: 1000px) {
  .head,
  .row {
    grid-template-columns: 38px minmax(0, 2.4fr) 78px 104px auto;
  }
  .head .h-chk,
  .cell.chk-t {
    display: none;
  }
}

@media (max-width: 860px) {
  /* Телефон: строка-карточка. Колонки таблицы прячем, всё нужное — под именем,
     действия открываются нажатием по строке. */
  .head {
    display: none;
  }
  .row {
    grid-template-columns: 38px minmax(0, 1fr) auto;
    padding: 6px 4px;
  }
  .cell.type,
  .cell.grp,
  .cell.chk-t,
  .cell.speed {
    display: none;
  }
  .only-mob {
    display: inline;
  }
  .cell.num {
    font-size: 12px;
    text-align: right;
    grid-column: 3;
  }
  .act {
    grid-column: 2 / -1;
    justify-content: flex-start;
    padding-left: 18px;
    flex-wrap: wrap;
  }
  /* Иконочные контролы на телефоне убраны: шесть кнопок переносились во вторую
     строку и удваивали высоту каждой из сотни карточек, а безымянные глифы в
     44px всё равно не читались. Те же четыре действия — подписанными
     тумблерами и кнопками в шторке, она открывается нажатием по строке. */
  .ico {
    display: none;
  }
  .mini,
  .dots {
    min-height: 44px;
  }
  .list {
    /* На телефоне крутим страницу целиком: вложенная прокрутка под пальцем
       раздражает, а панель массовых действий и так липнет к низу экрана. */
    max-height: none;
  }
}
</style>
