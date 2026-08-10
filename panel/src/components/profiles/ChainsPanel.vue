<script setup lang="ts">
/* Цепочки — сохранённый порядок профилей: клиент → первый хоп → … → выход.
   Активной считается та, чей набор хопов совпадает с текущим active_chain:
   роутер хранит именно хопы, а не идентификатор цепочки. */
import { computed, ref } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import SectionIcon from "@/components/SectionIcon.vue";
import PField from "@/components/profiles/PField.vue";
import { chains } from "@/api";
import type { Chain } from "@/api";
import { useProfilesStore } from "@/stores/profiles";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { slugify } from "@/components/profiles/uri";

const store = useProfilesStore();
const status = useStatusStore();
const toast = useToastStore();

const busy = ref("");
const sheetOpen = ref(false);
const editing = ref<Chain>({ id: "", name: "", hops: [] });
const isNew = ref(true);
const hopQuery = ref("");

const activeCsv = computed(() => store.activeChain.join(","));

function nameOf(id: string): string {
  return store.rows.find((r) => r.id === id)?.name ?? id;
}

const candidates = computed(() => {
  const q = hopQuery.value.trim().toLowerCase();
  return store.rows
    .filter((r) => !editing.value.hops.includes(r.id))
    .filter((r) => !q || `${r.name} ${r.type} ${r.group ?? ""}`.toLowerCase().includes(q))
    .slice(0, 60);
});

function openNew() {
  editing.value = { id: "", name: "", hops: [] };
  isNew.value = true;
  hopQuery.value = "";
  sheetOpen.value = true;
}

function openEdit(c: Chain) {
  editing.value = { id: c.id, name: c.name, hops: [...c.hops] };
  isNew.value = false;
  hopQuery.value = "";
  sheetOpen.value = true;
}

function addHop(id: string) {
  editing.value.hops = [...editing.value.hops, id];
}

function dropHop(i: number) {
  editing.value.hops = editing.value.hops.filter((_, idx) => idx !== i);
}

function moveHop(i: number, delta: number) {
  const next = [...editing.value.hops];
  const j = i + delta;
  if (j < 0 || j >= next.length) return;
  [next[i], next[j]] = [next[j], next[i]];
  editing.value.hops = next;
}

async function save() {
  const name = editing.value.name.trim();
  if (!name || !editing.value.hops.length) return;
  busy.value = "save";
  try {
    await chains.save({
      id: editing.value.id || slugify(name),
      name,
      hops: editing.value.hops,
    });
    toast.ok("Цепочка сохранена");
    sheetOpen.value = false;
    await store.loadChains();
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить цепочку");
  } finally {
    busy.value = "";
  }
}

async function activate(c: Chain) {
  busy.value = `on:${c.id}`;
  try {
    await store.activateChain(c.hops);
    toast.ok(`Включена цепочка «${c.name}»`);
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось включить цепочку");
  } finally {
    busy.value = "";
  }
}

async function remove(c: Chain) {
  if (!window.confirm(`Удалить цепочку «${c.name}»?`)) return;
  busy.value = `del:${c.id}`;
  try {
    await chains.remove(c.id);
    toast.ok("Цепочка удалена");
    await store.loadChains();
  } catch (e) {
    /* Роутер отказывает, если цепочка сейчас в работе — показываем причину,
       а не общее «не получилось». */
    toast.fromError(e, "Не удалось удалить цепочку");
  } finally {
    busy.value = "";
  }
}

defineExpose({ openNew });
</script>

<template>
  <TileCard title="Цепочки">
    <p class="lead">
      Цепочка ведёт трафик через несколько серверов подряд: клиент → первый
      профиль → следующий → выход. Порядок задаёте вы; последний хоп и есть тот
      адрес, который видят сайты.
    </p>

    <p v-if="!store.chainList.length" class="empty">
      Пока ни одной цепочки. Обычный профиль этим не отменяется — цепочка нужна,
      когда одного сервера мало.
    </p>

    <ul v-else class="chains">
      <li v-for="c in store.chainList" :key="c.id" :class="{ on: c.hops.join(',') === activeCsv }">
        <div class="ct">
          <p class="cname">
            {{ c.name }}
            <span v-if="c.hops.join(',') === activeCsv" class="badge">активна</span>
          </p>
          <p class="hops">{{ c.hops.map(nameOf).join(" → ") }}</p>
        </div>
        <div class="cact">
          <UiButton
            :busy="busy === `on:${c.id}`"
            :disabled="!!busy || c.hops.join(',') === activeCsv"
            @click="activate(c)"
          >
            Включить
          </UiButton>
          <UiButton :disabled="!!busy" @click="openEdit(c)">Править</UiButton>
          <UiButton variant="danger" :busy="busy === `del:${c.id}`" :disabled="!!busy" @click="remove(c)">
            Удалить
          </UiButton>
        </div>
      </li>
    </ul>

    <template #actions>
      <UiButton variant="primary" @click="openNew">Новая цепочка</UiButton>
    </template>
  </TileCard>

  <DrawerSheet
    :open="sheetOpen"
    :title="isNew ? 'Новая цепочка' : `Цепочка: ${editing.name}`"
    wide
    @close="sheetOpen = false"
  >
    <PField label="Название цепочки">
      <input v-model="editing.name" type="text" placeholder="Например, Двойной прыжок" />
    </PField>

    <p class="eyebrow sec">Хопы по порядку</p>
    <p v-if="!editing.hops.length" class="empty">
      Добавьте хотя бы один профиль из списка ниже.
    </p>
    <ol v-else class="hoplist">
      <li v-for="(h, i) in editing.hops" :key="`${h}-${i}`">
        <span class="idx num">{{ i + 1 }}</span>
        <span class="hname">{{ nameOf(h) }}</span>
        <span class="hbtns">
          <button type="button" aria-label="Выше" :disabled="i === 0" @click="moveHop(i, -1)">↑</button>
          <button
            type="button"
            aria-label="Ниже"
            :disabled="i === editing.hops.length - 1"
            @click="moveHop(i, 1)"
          >
            ↓
          </button>
          <button type="button" aria-label="Убрать" @click="dropHop(i)">
            <SectionIcon name="close" :size="15" />
          </button>
        </span>
      </li>
    </ol>

    <p class="eyebrow sec">Добавить хоп</p>
    <label class="search">
      <SectionIcon name="search" :size="14" />
      <input v-model="hopQuery" type="search" placeholder="Имя, протокол, группа" aria-label="Поиск профиля" />
    </label>
    <div class="cand">
      <button v-for="r in candidates" :key="r.id" type="button" @click="addHop(r.id)">
        <i class="hdot" :class="`h-${r.state}`"></i>
        <span>{{ r.name }}<small>{{ r.type }}</small></span>
      </button>
      <p v-if="!candidates.length" class="empty">Свободных профилей не осталось</p>
    </div>

    <template #footer>
      <UiButton
        variant="primary"
        :busy="busy === 'save'"
        :disabled="!editing.name.trim() || !editing.hops.length"
        @click="save"
      >
        Сохранить
      </UiButton>
      <UiButton @click="sheetOpen = false">Отмена</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.lead,
.empty {
  font-size: 13px;
  color: var(--dim);
}
.empty {
  padding: 8px 0;
}
.chains {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.chains li {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.chains li.on {
  border-color: var(--accent);
  background: var(--accent-wash);
}
.ct {
  min-width: 0;
  flex: 1 1 220px;
}
.cname {
  font-size: 14.5px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.badge {
  font-size: 10.5px;
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 999px;
  padding: 1px 7px;
}
.hops {
  font-size: 12px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.cact {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.sec {
  margin-top: 16px;
  margin-bottom: 6px;
}
.hoplist {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.hoplist li {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 6px 10px;
  min-height: 48px;
}
.idx {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--panel-2);
  border: 1px solid var(--line);
  display: grid;
  place-items: center;
  font-size: 11.5px;
  color: var(--dim);
  flex: none;
}
.hname {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 14px;
}
.hbtns {
  margin-left: auto;
  display: flex;
  gap: 4px;
}
.hbtns button {
  border: 1px solid var(--line-2);
  background: transparent;
  border-radius: var(--radius-sm);
  min-width: 44px;
  min-height: 44px;
  color: var(--dim);
  display: grid;
  place-items: center;
}
.hbtns button:disabled {
  opacity: 0.35;
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
  min-height: 44px;
}
.search input {
  border: 0;
  background: transparent;
  outline: none;
  width: 100%;
  min-width: 0;
  font-size: 16px;
  color: var(--ink);
}
.cand {
  margin-top: 8px;
  max-height: 300px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.cand button {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 1px solid transparent;
  background: transparent;
  border-radius: var(--radius-sm);
  padding: 8px 10px;
  text-align: left;
  min-height: 44px;
  color: var(--ink);
}
.cand button:hover {
  background: var(--panel-2);
  border-color: var(--line);
}
.cand span {
  min-width: 0;
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cand small {
  display: block;
  font-size: 11.5px;
  color: var(--faint);
}
</style>
