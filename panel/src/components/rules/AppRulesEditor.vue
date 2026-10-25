<script setup lang="ts">
/* Правила приложений (только приложение Detour): весь трафик программы мимо
   VPN или через свой профиль/цепочку. Файл — строки «match|mode|target|name»,
   как правила устройств роутера; match — имя файла, путь/каталог или пакет
   Android. «Отдельные маршруты» и обход DPI главнее. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import type { PickApp } from "@/api";

interface AppRule {
  match: string;
  /* "direct" — мимо VPN, иначе id профиля или цепочки */
  route: string;
  name: string;
}

interface TargetOption {
  id: string;
  name: string;
  isChain: boolean;
}

const props = defineProps<{
  open: boolean;
  text: string;
  targets: TargetOption[];
  by: "package" | "process";
  apps: PickApp[];
  appsSource: "installed" | "running" | "";
  appsLoading?: boolean;
  loading?: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{ close: []; save: [string]; refreshApps: [] }>();

const rules = ref<AppRule[]>([]);

function parse(text: string): AppRule[] {
  const out: AppRule[] = [];
  for (const line of String(text ?? "").replace(/\r/g, "").split("\n")) {
    const [match = "", mode = "", target = "", name = ""] = line.split("|");
    const m = match.trim();
    if (!m || m.startsWith("//")) continue;
    if (mode === "direct") out.push({ match: m, route: "direct", name });
    else if (mode === "vpn" && target) out.push({ match: m, route: target, name });
  }
  return out;
}

function serialize(list: AppRule[]): string {
  const clean = (s: string) => s.replace(/[|\n\r\t]/g, " ").trim();
  return list
    .filter((r) => r.match.trim() && r.route)
    .map((r) =>
      r.route === "direct"
        ? `${clean(r.match)}|direct||${clean(r.name)}`
        : `${clean(r.match)}|vpn|${r.route}|${clean(r.name)}`,
    )
    .join("\n");
}

watch(
  () => [props.open, props.text] as const,
  ([open]) => {
    if (open) {
      rules.value = parse(props.text);
      picking.value = false;
      query.value = "";
    }
  },
  { immediate: true },
);

const profileOptions = computed(() => props.targets.filter((t) => !t.isChain));
const chainOptions = computed(() => props.targets.filter((t) => t.isChain));

const appByMatch = computed(() => {
  const m = new Map<string, PickApp>();
  for (const a of props.apps) m.set(a.match.toLowerCase(), a);
  return m;
});

function iconOf(match: string): string | undefined {
  const icon = appByMatch.value.get(match.toLowerCase())?.icon;
  return icon ? `data:image/png;base64,${icon}` : undefined;
}

function isPath(m: string) {
  return m.includes("/") || m.includes("\\");
}

/* ---------- выбор приложения ---------- */
const picking = ref(false);
const query = ref("");
const showSystem = ref(false);
const manual = ref("");

const taken = computed(() => new Set(rules.value.map((r) => r.match.toLowerCase())));

const choices = computed(() => {
  const q = query.value.trim().toLowerCase();
  return props.apps
    .filter((a) => showSystem.value || !a.system)
    .filter((a) => !q || a.name.toLowerCase().includes(q) || a.match.toLowerCase().includes(q))
    .sort((a, b) => Number(!!a.system) - Number(!!b.system) || a.name.localeCompare(b.name, "ru"))
    .slice(0, 300);
});

const hiddenSystem = computed(() => (showSystem.value ? 0 : props.apps.filter((a) => a.system).length));

function startPick() {
  picking.value = true;
  if (!props.apps.length) emit("refreshApps");
}

function add(match: string, name: string) {
  const m = match.trim();
  if (!m) return;
  if (taken.value.has(m.toLowerCase())) return;
  rules.value = [...rules.value, { match: m, route: "direct", name: name.trim() }];
  picking.value = false;
  query.value = "";
  manual.value = "";
}

function addManual() {
  const m = manual.value.trim();
  const base = m.split(/[\\/]/).filter(Boolean).pop() ?? m;
  add(m, base.replace(/\.(exe|app)$/i, ""));
}

function remove(index: number) {
  rules.value = rules.value.filter((_, i) => i !== index);
}

const manualHint = computed(() =>
  props.by === "package"
    ? "Имя пакета, например org.telegram.messenger"
    : "Имя файла (Telegram.exe), путь или папка программы",
);
</script>

<template>
  <DrawerSheet :open="open" title="Приложения" wide @close="emit('close')">
    <template #sticky>
      <p class="hint">
        <span>Весь трафик приложения — мимо VPN или через выбранный VPN или
          цепочку. «Отдельные маршруты» и обход блокировок главнее.</span>
        <b class="num">{{ loading ? "загружаю…" : `${rules.length}` }}</b>
      </p>
    </template>

    <p v-if="loading" class="note">Загружаю правила…</p>
    <p v-else-if="!rules.length && !picking" class="empty">
      Правил пока нет. Например, банковское приложение можно пустить мимо VPN, а
      мессенджер — через отдельный профиль.
    </p>

    <div v-for="(rule, i) in rules" :key="rule.match" class="card">
      <div class="head">
        <img v-if="iconOf(rule.match)" class="ico" :src="iconOf(rule.match)" alt="" />
        <span v-else class="ico ph" aria-hidden="true">{{ (rule.name || rule.match).slice(0, 1).toUpperCase() }}</span>
        <div class="who">
          <input v-model="rule.name" class="name-inp" placeholder="Название" aria-label="Название" />
          <span class="mono sub" :title="rule.match">{{ rule.match }}</span>
        </div>
        <button class="del" type="button" @click="remove(i)">Убрать</button>
      </div>
      <select v-model="rule.route" class="sel full" aria-label="Куда направить трафик приложения">
        <option value="direct">Мимо VPN — напрямую</option>
        <optgroup v-if="profileOptions.length" label="Через профиль">
          <option v-for="t in profileOptions" :key="t.id" :value="t.id">{{ t.name }}</option>
        </optgroup>
        <optgroup v-if="chainOptions.length" label="Через цепочку">
          <option v-for="t in chainOptions" :key="t.id" :value="t.id">{{ t.name }}</option>
        </optgroup>
        <option
          v-if="rule.route !== 'direct' && !targets.some((t) => t.id === rule.route)"
          :value="rule.route"
        >
          {{ rule.route }} (не найден — пойдёт общим путём)
        </option>
      </select>
      <p v-if="by === 'process' && isPath(rule.match)" class="sub">
        Правило действует на все программы внутри этой папки.
      </p>
    </div>

    <div v-if="picking" class="card picker">
      <div class="row">
        <input
          v-model="query"
          class="inp"
          type="search"
          placeholder="Поиск по названию"
          aria-label="Поиск приложения"
          autocomplete="off"
        />
        <UiButton :busy="appsLoading" @click="emit('refreshApps')">Обновить</UiButton>
      </div>
      <p class="sub">
        {{
          appsSource === "running"
            ? "Запущенные сейчас программы. Нужной нет — запустите её и нажмите «Обновить» или впишите имя ниже."
            : "Установленные приложения с доступом в интернет."
        }}
      </p>
      <p v-if="appsLoading && !apps.length" class="note">Собираю список…</p>
      <ul v-else class="list">
        <li v-for="a in choices" :key="a.match">
          <button
            type="button"
            class="pick"
            :disabled="taken.has(a.match.toLowerCase())"
            :title="a.path || a.match"
            @click="add(a.match, a.name)"
          >
            <img v-if="a.icon" class="ico" :src="`data:image/png;base64,${a.icon}`" alt="" />
            <span v-else class="ico ph" aria-hidden="true">{{ a.name.slice(0, 1).toUpperCase() }}</span>
            <span class="who">
              <span class="nm">{{ a.name }}</span>
              <span class="mono sub">{{ a.path || a.match }}</span>
            </span>
            <span v-if="taken.has(a.match.toLowerCase())" class="sub">уже в списке</span>
          </button>
        </li>
        <li v-if="!choices.length" class="sub">Ничего не нашлось.</li>
      </ul>
      <label v-if="hiddenSystem || showSystem" class="chk">
        <input v-model="showSystem" type="checkbox" />
        Показать системные ({{ apps.filter((a) => a.system).length }})
      </label>
      <div class="row">
        <input
          v-model="manual"
          class="inp mono"
          spellcheck="false"
          autocapitalize="off"
          autocomplete="off"
          :placeholder="manualHint"
          aria-label="Приложение вручную"
          @keydown.enter.prevent="addManual"
        />
        <UiButton :disabled="!manual.trim()" @click="addManual">Добавить</UiButton>
      </div>
      <UiButton variant="ghost" @click="picking = false">Скрыть выбор</UiButton>
    </div>

    <UiButton v-if="!picking" class="add" @click="startPick">Добавить приложение</UiButton>

    <p class="note">
      Сохранение перестраивает конфигурацию и перезапускает подключение. При
      включённом «Всё через VPN» правила «мимо VPN» не действуют.
      <template v-if="by === 'package'">
        Приложения «мимо VPN» система выводит из туннеля целиком.
      </template>
    </p>

    <template #footer>
      <UiButton variant="primary" :busy="busy" :disabled="loading || busy" @click="emit('save', serialize(rules))">
        Сохранить и применить
      </UiButton>
      <UiButton :disabled="busy" @click="emit('close')">Отмена</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.hint {
  font-size: 12.5px;
  color: var(--dim);
  display: flex;
  gap: 10px;
  align-items: baseline;
  flex-wrap: wrap;
}
.hint b {
  margin-left: auto;
  color: var(--ink);
  font-weight: 600;
  white-space: nowrap;
}
.empty {
  border: 1px dashed var(--line-2);
  border-radius: var(--radius-sm);
  padding: 14px;
  color: var(--dim);
  font-size: 13.5px;
}
.card {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 9px;
  margin-bottom: 12px;
  min-width: 0;
}
.head {
  display: flex;
  gap: 10px;
  align-items: center;
  min-width: 0;
}
.who {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  gap: 2px;
}
.ico {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  flex: none;
}
.ph {
  display: grid;
  place-items: center;
  background: var(--panel);
  border: 1px solid var(--line-2);
  color: var(--dim);
  font-weight: 600;
  font-size: 14px;
}
.name-inp {
  border: 0;
  background: transparent;
  color: var(--ink);
  font-size: 15px;
  font-weight: 600;
  padding: 0;
  min-width: 0;
}
.name-inp:focus {
  outline: none;
  border-bottom: 1px solid var(--accent);
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.sel,
.inp {
  flex: 1 1 200px;
  min-width: 0;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--ink);
  padding: 9px 10px;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  min-height: 44px;
}
.sel.full {
  flex: none;
  width: 100%;
}
.inp:focus,
.sel:focus {
  border-color: var(--accent);
  outline: none;
}
.del {
  border: 1px solid var(--line-2);
  background: transparent;
  color: var(--bad);
  border-radius: var(--radius-sm);
  padding: 8px 12px;
  font-size: 13px;
  min-height: 44px;
  flex: none;
}
.del:hover {
  background: color-mix(in srgb, var(--bad) 12%, transparent);
  border-color: var(--bad);
}
.add {
  width: 100%;
  justify-content: center;
}
.sub {
  font-size: 12px;
  color: var(--dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.list {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: min(50vh, 420px);
  overflow-y: auto;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel);
}
.pick {
  display: flex;
  gap: 10px;
  align-items: center;
  width: 100%;
  min-height: 48px;
  padding: 6px 10px;
  border: 0;
  border-bottom: 1px solid var(--line);
  background: transparent;
  color: var(--ink);
  text-align: left;
  cursor: pointer;
}
.pick:hover:not(:disabled) {
  background: var(--panel-2);
}
.pick:disabled {
  opacity: 0.5;
  cursor: default;
}
.nm {
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.chk {
  display: flex;
  gap: 8px;
  align-items: center;
  font-size: 13px;
  color: var(--dim);
}
.note {
  margin-top: 12px;
  font-size: 12px;
  color: var(--faint);
}
@media (max-width: 700px) {
  :deep(.btn) {
    min-height: 44px;
  }
}
</style>
