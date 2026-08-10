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
import { fmtAgo } from "@/lib/format";

const props = defineProps<{
  rows: ProfileRow[];
  selected: string[];
  switching: string;
  /** Идентификатор профиля, по которому сейчас идёт проверка. */
  probing: string;
}>();

const emit = defineEmits<{
  "update:selected": [string[]];
  open: [ProfileRow];
  connect: [ProfileRow];
}>();

type SortKey = "name" | "type" | "group" | "ping" | "state";

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
    return `${r.name} ${r.type} ${r.group ?? ""} ${r.id}`.toLowerCase().includes(q);
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
    sortAsc.value = true;
  }
}

function pingText(r: ProfileRow): string {
  if (props.probing === r.id) return "проверяю…";
  if (!r.ping || r.ping.ok === undefined) return "—";
  if (!r.ping.ok) return "нет ответа";
  /* rtt = -1 приходит, когда узел ответил, но замерить время не вышло. */
  return (r.ping.rtt ?? -1) > 0 ? `${Math.round(r.ping.rtt as number)} мс` : "отвечает";
}

function healthText(r: ProfileRow): string {
  const h = r.health;
  if (!h || h.ok === undefined) return "—";
  const when = fmtAgo(h.ts);
  return `${h.ok ? "проходит" : "не проходит"}${when ? ` · ${when}` : ""}`;
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
      <button type="button" :aria-pressed="sortKey === 'type'" @click="sortBy('type')">
        Протокол
      </button>
      <button type="button" :aria-pressed="sortKey === 'group'" @click="sortBy('group')">
        Группа
      </button>
      <button type="button" :aria-pressed="sortKey === 'ping'" @click="sortBy('ping')">
        Пинг
      </button>
      <button type="button" :aria-pressed="sortKey === 'state'" @click="sortBy('state')">
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
              <template v-if="r.autoswitch === false"> · без авто-переключения</template>
              <template v-if="r.speedcheck === false"> · без проверки скорости</template>
            </small>
          </span>
          <span v-if="r.isActive" class="badge">активен</span>
        </button>

        <span class="cell type">{{ r.type }}</span>
        <span class="cell grp">{{ r.group || "—" }}</span>
        <span class="cell num">{{ pingText(r) }}</span>
        <span class="cell chk-t">{{ healthText(r) }}</span>

        <span class="act">
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
  grid-template-columns: 38px minmax(0, 2.4fr) 100px minmax(0, 1fr) 92px 150px auto;
  align-items: center;
  gap: 6px;
}
.head {
  padding: 0 6px 6px;
  border-bottom: 1px solid var(--line);
}
.head button {
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
  .cell.chk-t {
    display: none;
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
