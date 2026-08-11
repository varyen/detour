<script setup lang="ts">
/* Список профилей. Их бывает под сотню, поэтому здесь всё, что помогает
   быстро найти нужный: поиск, фильтр по группе, сортировка по колонкам и
   массовое выделение с диапазоном по Shift.

   Разметка одна на все экраны: на широком это таблица (колонки заданы
   grid-template-columns), на телефоне те же строки превращаются в карточки —
   таблица с горизонтальной прокруткой на 360px нечитаема.

   Телефонная карточка сделана «метриками-плитками»: под именем ряд плиток
   (скорость акцентом, пинг, проверка), а справа одна круглая кнопка ▶/■. Раньше
   там лежал второй ряд с «Подключить» и «…» — два контрола по 44px удваивали
   высоту каждой из сотни карточек. Остальные действия никуда не делись: тап по
   строке открывает шторку, где они подписаны словами. */
import { computed, ref, watch } from "vue";
import SectionIcon from "@/components/SectionIcon.vue";
import type { ProfileRow } from "@/stores/profiles";
import { ccFromName, countryName, flagOf, fmtAgo, fmtSpeedKbps } from "@/lib/format";

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
  /** Идёт скан стран (detour-geo): кнопка глобуса заблокирована. */
  geoBusy?: boolean;
}>();

const emit = defineEmits<{
  "update:selected": [string[]];
  open: [ProfileRow];
  connect: [ProfileRow];
  stop: [ProfileRow];
  ping: [ProfileRow];
  health: [ProfileRow];
  flag: [{ row: ProfileRow; kind: "autoswitch" | "speedcheck"; value: boolean }];
  geoscan: [];
}>();

type SortKey = "name" | "type" | "group" | "ping" | "speed" | "state";

const query = ref("");
const group = ref("");
const country = ref("");
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

/**
 * Страна профиля для фильтра и поиска. Сначала флаг из имени — это страна
 * ВЫХОДА, ради которой профиль и покупали; и только если провайдер имя не
 * подписал (прямые SOCKS/HTTP-прокси), берём страну узла из detour-geo — у
 * такого прокси эндпоинт и есть выход, так что там она верна.
 */
function profileCc(r: ProfileRow): string {
  return ccFromName(r.name) || (r.cc || "").toUpperCase();
}

/* Страны, которые реально встречаются в списке: пустой фильтр из 250 стран мира
   бесполезен. Сортируем по названию, а не по коду — «Нидерланды» ищут глазами,
   а не как NL. */
const countries = computed(() => {
  const set = new Set<string>();
  for (const r of props.rows) {
    const cc = profileCc(r);
    if (cc) set.add(cc);
  }
  return [...set]
    .map((cc) => ({ cc, name: countryName(cc) || cc }))
    .sort((a, b) => a.name.localeCompare(b.name, "ru"));
});

const visible = computed(() => {
  const q = query.value.trim().toLowerCase();
  const g = group.value;
  const c = country.value;
  const out = props.rows.filter((r) => {
    if (g && (r.group || "Без группы") !== g) return false;
    if (c && profileCc(r) !== c) return false;
    if (!q) return true;
    /* Адрес узла тоже ищем: у подписок имена профилей похожи как две капли, и
       найти нужный проще по хосту. Страна ищется и словом, и кодом: «нидерл» и
       «nl» должны находить одно и то же.
       Страна УЗЛА в поиск намеренно не входит, хотя данные есть: иначе запрос
       «германия» выдавал бы и Буэнос-Айрес с Софией (их узлы в DE) — поиск начал
       бы противоречить фильтру над ним. Узел показан в шторке профиля. */
    const cc = profileCc(r);
    return `${r.name} ${r.type} ${r.group ?? ""} ${r.id} ${r.ping?.server ?? ""} ${cc} ${countryName(
      cc,
    )}`
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

/* Направление сортировки должно быть видно, а не угадываться: колонок семь, а
   у скорости и пинга «по возрастанию» и «по убыванию» отвечают на прямо
   противоположные вопросы. */
function arrow(key: SortKey): string {
  if (sortKey.value !== key) return "";
  return sortAsc.value ? "↑" : "↓";
}

const SORT_HINT: Record<SortKey, [string, string]> = {
  name: ["А→Я", "Я→А"],
  type: ["А→Я", "Я→А"],
  group: ["А→Я", "Я→А"],
  ping: ["сначала быстрые", "сначала медленные"],
  speed: ["сначала медленные", "сначала быстрые"],
  state: ["сначала рабочие", "сначала нерабочие"],
};

function sortTitle(key: SortKey, label: string): string {
  const [asc, desc] = SORT_HINT[key];
  if (sortKey.value !== key) return `Сортировать по «${label}»: ${asc}`;
  return `${label}: ${sortAsc.value ? asc : desc} — нажмите, чтобы перевернуть`;
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

/* Сортировка на телефоне — видимыми чипами, а не выпадашкой: в списке на сотню
   профилей важно с одного взгляда понимать, по чему он сейчас отсортирован.
   Порядок — по частоте: скорость и есть тот вопрос, ради которого сюда заходят. */
const SORT_CHIPS: [SortKey, string][] = [
  ["speed", "Скорость"],
  ["ping", "Пинг"],
  ["state", "Статус"],
  ["name", "Имя"],
];

/**
 * Плитка скорости: акцентная, потому что её сравнивают между строками. Значка
 * молнии нет намеренно — цвет и так выделяет плитку, а на экране 360px эти
 * лишние 14px переносили ряд плиток на вторую строку.
 */
function speedTile(r: ProfileRow): string {
  return fmtSpeedKbps(speedKbps(r)) || "";
}

function stateTile(r: ProfileRow): string {
  if (props.probing === r.id) return "проверяю…";
  switch (r.state) {
    case "ok":
      return "✓ проверка";
    case "slow":
      return "медленно";
    case "dead":
      return "✕ не отвечает";
    default:
      return "не проверялся";
  }
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
      <!-- Список стран пуст, пока detour-geo не отработал: показывать выпадашку
           с одним пунктом «Все страны» незачем — вместо неё кнопка запуска. -->
      <select v-if="countries.length" v-model="country" aria-label="Страна">
        <option value="">Все страны</option>
        <option v-for="c in countries" :key="c.cc" :value="c.cc">
          {{ flagOf(c.cc) }} {{ c.name }}
        </option>
      </select>
      <button
        class="geo"
        type="button"
        :disabled="geoBusy"
        :title="
          countries.length
            ? 'Обновить страны эндпоинтов'
            : 'Определить страны эндпоинтов'
        "
        aria-label="Определить страны эндпоинтов"
        @click="emit('geoscan')"
      >
        <SectionIcon name="globe" :size="15" />
      </button>
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

    <!-- Тот же выбор, что и в `.sortsel`, но кнопками: на телефоне выпадашка
         прячет и сам факт сортировки, и её направление. Повторное нажатие по
         активному чипу переворачивает порядок — как по колонке в таблице. -->
    <div class="chips" role="group" aria-label="Сортировка">
      <button
        v-for="[k, label] in SORT_CHIPS"
        :key="k"
        type="button"
        class="chip"
        :class="{ on: sortKey === k }"
        :aria-pressed="sortKey === k"
        :title="sortTitle(k, label)"
        @click="sortBy(k)"
      >
        {{ label }}<i v-if="sortKey === k" class="arw" aria-hidden="true">{{ arrow(k) }}</i>
      </button>
    </div>

    <p class="count">
      {{ visible.length }} из {{ rows.length }}<template v-if="selected.length">
        · выбрано {{ selected.length }}</template>
      <!-- Требование лицензии CC-BY базы DB-IP: там, где показаны её результаты,
           обязана быть ссылка на источник. Убирать нельзя. -->
      <template v-if="countries.length">
        ·
        <a class="attrib" href="https://db-ip.com" target="_blank" rel="noopener noreferrer"
          >IP Geolocation by DB-IP</a
        >
      </template>
    </p>

    <div class="list">
      <!-- Шапка живёт ВНУТРИ прокручиваемого списка, и это не украшательство:
           снаружи у неё своя ширина (без полосы прокрутки) и свой расчёт колонки
           `auto`, поэтому подписи разъезжались со столбцами тем сильнее, чем
           шире окно. Внутри — та же ширина, та же сетка, плюс шапка остаётся
           видимой при прокрутке сотни строк. -->
      <div class="head" role="row">
        <label class="chk">
          <input
            type="checkbox"
            :checked="allChecked"
            aria-label="Выделить всё видимое"
            @change="toggleAll"
          />
        </label>
        <button
          type="button"
          :aria-pressed="sortKey === 'name'"
          :title="sortTitle('name', 'Имя')"
          @click="sortBy('name')"
        >
          Имя<i class="arw" aria-hidden="true">{{ arrow("name") }}</i>
        </button>
        <button
          class="h-type"
          type="button"
          :aria-pressed="sortKey === 'type'"
          :title="sortTitle('type', 'Протокол')"
          @click="sortBy('type')"
        >
          Протокол<i class="arw" aria-hidden="true">{{ arrow("type") }}</i>
        </button>
        <button
          class="h-grp"
          type="button"
          :aria-pressed="sortKey === 'group'"
          :title="sortTitle('group', 'Группа')"
          @click="sortBy('group')"
        >
          Группа<i class="arw" aria-hidden="true">{{ arrow("group") }}</i>
        </button>
        <!-- Не кнопка: сортировать по адресу узла незачем, а колонка живёт только
             на широком экране — её данные приходят из кэша пингов. -->
        <span class="h-srv">Сервер</span>
        <button
          type="button"
          :aria-pressed="sortKey === 'ping'"
          :title="sortTitle('ping', 'Пинг')"
          @click="sortBy('ping')"
        >
          Пинг<i class="arw" aria-hidden="true">{{ arrow("ping") }}</i>
        </button>
        <button
          type="button"
          :aria-pressed="sortKey === 'speed'"
          :title="sortTitle('speed', 'Скорость')"
          @click="sortBy('speed')"
        >
          Скорость<i class="arw" aria-hidden="true">{{ arrow("speed") }}</i>
        </button>
        <button
          class="h-chk"
          type="button"
          :aria-pressed="sortKey === 'state'"
          :title="sortTitle('state', 'Проверка')"
          @click="sortBy('state')"
        >
          Проверка<i class="arw" aria-hidden="true">{{ arrow("state") }}</i>
        </button>
        <span class="h-act"></span>
      </div>

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
            <span class="nm-n">
              <!-- Флаг рисуем ТОЛЬКО если провайдер не поставил свой в имени:
                   почти все подписки начинают имя с флага, и второй — вычисленный
                   по адресу узла — встал бы рядом и противоречил ему (проверено:
                   у 78 из 83 профилей узел в другой стране, чем обещает имя).
                   Здесь же, а не отдельной плиткой: на 360px ряд плиток занят
                   скоростью/пингом/проверкой, четвёртая ушла бы на вторую строку. -->
              <span
                v-if="!ccFromName(r.name) && flagOf(profileCc(r))"
                class="cflag"
                :title="countryName(profileCc(r))"
                >{{ flagOf(profileCc(r)) }}</span
              >{{ r.name }}</span
            >
            <small>
              {{ r.type }}<template v-if="r.group"> · {{ r.group }}</template>
              <template v-if="r.autoswitch === false"> · без авто-переключения</template>
              <template v-if="r.speedcheck === false"> · без проверки скорости</template>
            </small>
            <!-- Плитки только на телефоне: на широком экране те же три числа
                 стоят своими колонками, и дублировать их под именем незачем. -->
            <span class="mrow">
              <!-- Плитки скорости у неизмеренного профиля нет вовсе: «скорость —»
                   занимала треть строки и переносила остальные на вторую. -->
              <span v-if="speedKbps(r) > 0" class="m spd measured">{{ speedTile(r) }}</span>
              <span class="m" :class="{ bad: r.ping && !r.ping.ok }">{{ pingText(r) }}</span>
              <span class="m" :class="`s-${r.state}`">{{ stateTile(r) }}</span>
            </span>
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
          <!-- Телефонная замена паре «Подключить»/«…»: одно прямое действие в
               строке. У активного профиля это «стоп» — тот же самый, что и на
               «Обзоре», другого способа выключить туннель отсюда нет. -->
          <button
            class="go"
            type="button"
            :class="{ stop: r.isActive, busy: switching === r.id }"
            :disabled="!!switching"
            :title="r.isActive ? 'Остановить sing-box' : 'Подключить'"
            :aria-label="r.isActive ? `Остановить ${r.name}` : `Подключить ${r.name}`"
            @click="r.isActive ? emit('stop', r) : emit('connect', r)"
          >
            <SectionIcon :name="r.isActive ? 'stop' : 'play'" :size="15" />
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
  /* Ширина колонки действий — ЧИСЛОМ, а не `auto`. Шапка и строки это разные
     grid-контейнеры, поэтому `auto` каждый считал по своему содержимому: у строк
     там шесть кнопок (~290px), у шапки — пустой span (0). Разницу забирали
     резиновые колонки, и подписи уезжали от своих столбцов тем сильнее, чем шире
     окно. Здесь: 4 иконки по 32 + «Подключить» + «…» на 36 + пять зазоров. */
  --act-w: 296px;
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
/* Кнопка скана стран — той же высоты, что и соседние контролы, чтобы не ломать
   ряд, и квадратная: подпись ей не нужна, смысл несут title и глобус. */
.geo {
  display: grid;
  place-items: center;
  width: 44px;
  min-height: 44px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--dim);
  cursor: pointer;
}
.geo:hover:not(:disabled) {
  color: var(--accent);
  border-color: var(--accent);
}
.geo:disabled {
  opacity: 0.5;
  cursor: default;
}
.count {
  font-size: 12px;
  color: var(--faint);
}
.attrib {
  color: inherit;
  text-decoration: underline;
  text-underline-offset: 2px;
}
/* Флаг не должен растягивать строку имени: эмодзи выше строчных букв, поэтому
   держим его на той же высоте и отделяем узким зазором. */
.cflag {
  margin-right: 6px;
  font-size: 13px;
  line-height: 1;
}
.head,
.row {
  display: grid;
  grid-template-columns:
    38px minmax(0, 2.4fr) 92px minmax(0, 1fr) minmax(0, 1.25fr)
    78px 104px 132px var(--act-w);
  align-items: center;
  gap: 6px;
}
.head {
  position: sticky;
  top: 0;
  z-index: 1;
  padding: 2px 6px 6px;
  border-bottom: 1px solid var(--line);
  /* Плитка полупрозрачная, поэтому «фон как у плитки» — это её тон, положенный
     поверх непрозрачного фона страницы. Просто var(--panel) не годится: сквозь
     липкую шапку просвечивали бы уезжающие под неё строки. */
  background: linear-gradient(var(--panel), var(--panel)) var(--ground);
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
.head button {
  display: flex;
  align-items: baseline;
  gap: 3px;
  min-width: 0;
}
.head button[aria-pressed="true"] {
  color: var(--accent);
}
.arw {
  font-style: normal;
  font-size: 11px;
  line-height: 1;
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
/* Чипы сортировки и плитки метрик живут только на телефоне (медиазапрос ниже). */
.chips,
.mrow,
.go {
  display: none;
}
.act {
  display: flex;
  gap: 6px;
  align-items: center;
  justify-content: flex-end;
  min-width: 0;
}
.mini {
  /* Колонка фиксированной ширины: если подпись кнопки окажется длиннее
     расчётной (другой шрифт, другой язык), сжимается она, а не столбец. */
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
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
    grid-template-columns: 38px minmax(0, 2.4fr) 78px 104px 132px var(--act-w);
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
    grid-template-columns: 38px minmax(0, 2.4fr) 78px 104px var(--act-w);
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
  /* Обе выпадашки и глобус — в одну строку. По умолчанию select занимает ширину
     самого длинного пункта («Все страны» + название страны), из-за чего каждый
     контрол уезжал на свою строку и список начинался у середины экрана. */
  .tools select {
    flex: 1 1 130px;
    min-width: 0;
    /* Ширину задаёт flex, а не самый длинный пункт списка. */
    width: 0;
  }
  .row {
    /* Колонка отметки уже: место нужнее плиткам метрик, а сам квадратик 18px. */
    grid-template-columns: 30px minmax(0, 1fr) auto;
    padding: 6px 4px;
    gap: 4px;
  }
  .chk {
    min-width: 30px;
  }
  /* Имя — в одну строку с многоточием: у профилей из подписок оно длинное, и
     перенос добавлял каждой карточке лишнюю строку. */
  .nm-n {
    display: block;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nm {
    gap: 8px;
    min-height: 0;
    align-items: flex-start;
    padding-top: 8px;
  }
  /* Точка здоровья убрана: то же самое словами говорит плитка проверки, а
     её место нужно самим плиткам, чтобы они умещались в одну строку. */
  .hdot {
    display: none;
  }
  /* Активная карточка — не полоса во всю ширину, а выделенная плитка. */
  .row.on {
    border-radius: 12px;
    border-bottom-color: transparent;
  }
  .cell.type,
  .cell.grp,
  .cell.chk-t,
  .cell.speed,
  /* Пинг и проверка переехали в плитки под именем. */
  .cell.num {
    display: none;
  }
  .chips {
    display: flex;
    gap: 7px;
    overflow-x: auto;
    scrollbar-width: none;
    padding-bottom: 2px;
  }
  .chips::-webkit-scrollbar {
    display: none;
  }
  .chip {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--line-2);
    background: var(--panel-2);
    color: var(--dim);
    border-radius: 999px;
    padding: 7px 13px;
    font-size: 12.5px;
    min-height: 36px;
    white-space: nowrap;
  }
  .chip.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-on);
    font-weight: 600;
  }
  /* Стрелка направления внутри залитого чипа — цветом чипа, иначе не видна. */
  .chip .arw {
    color: inherit;
  }
  /* Выпадашка сортировки дублировала бы чипы. Группы остаются списком: их
     бывает десяток, чипами это уже полоса на два экрана. */
  .sortsel {
    display: none;
  }
  .mrow {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 6px;
  }
  .m {
    font-size: 11px;
    line-height: 1.45;
    border-radius: 7px;
    padding: 2px 7px;
    white-space: nowrap;
    background: var(--panel-2);
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }
  .m.spd.measured {
    background: var(--accent-wash);
    color: var(--accent);
    font-weight: 700;
  }
  .m.bad,
  .m.s-dead {
    color: var(--bad);
  }
  .m.s-ok {
    color: var(--ok);
  }
  .act {
    grid-column: 3;
    flex-wrap: nowrap;
  }
  /* В строке остаётся одно действие — круглая ▶/■. Остальные (пинг, замер,
     оба флага, правка, удаление) подписаны словами в шторке, которая
     открывается нажатием по строке. */
  .ico,
  .mini,
  .dots {
    display: none;
  }
  .go {
    display: grid;
    place-items: center;
    flex: none;
    width: 40px;
    height: 40px;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-on);
    box-shadow: 0 2px 8px var(--accent-wash);
  }
  .go.stop {
    background: transparent;
    color: var(--bad);
    border: 1.5px solid var(--bad);
    box-shadow: none;
  }
  .go:disabled {
    opacity: 0.35;
  }
  .go.busy {
    animation: blink 1s ease-in-out infinite;
    opacity: 1;
  }
  .list {
    /* На телефоне крутим страницу целиком: вложенная прокрутка под пальцем
       раздражает, а панель массовых действий и так липнет к низу экрана. */
    max-height: none;
  }
}
</style>
