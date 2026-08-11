<script setup lang="ts">
/* Раздел «Профили»: сами профили, цепочки, подписки и Cloudflare WARP.
   Вкладки, а не одна простыня: профилей бывает под сотню, и мешать их со
   списком подписок — значит каждый раз пролистывать чужое.

   Панели вкладок держим смонтированными (v-show, не v-if): подписки и WARP
   грузят своё состояние один раз, а по WARP ещё и решается, показывать ли
   вкладку вообще. */
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import PField from "@/components/profiles/PField.vue";
import ProfileList from "@/components/profiles/ProfileList.vue";
import ProfileSheet from "@/components/profiles/ProfileSheet.vue";
import ChainsPanel from "@/components/profiles/ChainsPanel.vue";
import SubscriptionsPanel from "@/components/profiles/SubscriptionsPanel.vue";
import WarpPanel from "@/components/profiles/WarpPanel.vue";
import { diag, profiles as profilesApi } from "@/api";
import type { ProfileRow } from "@/stores/profiles";
import { useProfilesStore } from "@/stores/profiles";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { useCommandStore } from "@/stores/commands";
import { draftFromProfile, profileFromDraft } from "@/components/profiles/uri";
import type { ProfileDraft } from "@/components/profiles/uri";
import { ccFromName, countryName, fmtAgo, fmtSpeedKbps } from "@/lib/format";

type Tab = "profiles" | "chains" | "subs" | "warp";

const store = useProfilesStore();
const status = useStatusStore();
const toast = useToastStore();
const commands = useCommandStore();

const tab = ref<Tab>("profiles");
const selected = ref<string[]>([]);
const busy = ref("");
const probing = ref("");
const flagBusy = ref("");
const geoBusy = ref(false);
const warpSupported = ref(false);
/* Подписи целей проверки идут тем же ответом health_status, что и результаты, —
   их складывает store.loadProbes(). Отдельный запрос ради заголовков означал бы
   второе чтение результатов по всей сотне профилей. */
const healthTargets = computed(() => store.healthTargets);

const sheetOpen = ref(false);
const sheetDraft = ref<ProfileDraft | null>(null);
const rowOpen = ref(false);
const rowItem = ref<ProfileRow | null>(null);

const chainsRef = ref<InstanceType<typeof ChainsPanel> | null>(null);
const subsRef = ref<InstanceType<typeof SubscriptionsPanel> | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);

const tabs = computed(() => {
  const list: { value: Tab; label: string }[] = [
    { value: "profiles", label: "Профили" },
    { value: "chains", label: "Цепочки" },
    { value: "subs", label: "Подписки" },
  ];
  if (warpSupported.value) list.push({ value: "warp", label: "WARP" });
  return list;
});

const groups = computed(() => {
  const set = new Set<string>();
  for (const r of store.rows) if (r.group) set.add(r.group);
  return [...set].sort((a, b) => a.localeCompare(b, "ru"));
});

const rowState = computed(() => {
  const r = rowItem.value;
  if (!r) return [] as string[];
  const out: string[] = [`Протокол: ${r.type}`, `Группа: ${r.group || "без группы"}`];
  if (r.ping?.ok) {
    const rtt = r.ping.rtt ?? -1;
    out.push(`Пинг: ${rtt > 0 ? `${Math.round(rtt)} мс` : "отвечает"}`);
  }
  else if (r.ping && r.ping.ok === false) out.push("Пинг: нет ответа");
  if (r.health && r.health.ok !== undefined) {
    out.push(
      `Проверка: ${r.health.ok ? "проходит" : "не проходит"}${
        r.health.ts ? ` · ${fmtAgo(r.health.ts)}` : ""
      }`,
    );
    /* По каждой цели отдельно: «не проходит» само по себе не говорит, что
       именно отвалилось — YouTube или весь выход в сеть. */
    const delays = r.health.delays ?? [];
    for (let i = 0; i < delays.length; i++) {
      const label = healthTargets.value[i]?.label || `Цель ${i + 1}`;
      const d = delays[i];
      /* Точка вместо отступа: пробелы в начале <li> браузер всё равно сожмёт. */
      out.push(
        `· ${label}: ${typeof d === "number" && d >= 0 ? `${Math.round(d)} мс` : "не отвечает"}`,
      );
    }
  }
  const speed = fmtSpeedKbps(r.health?.dl);
  out.push(`Скорость: ${speed ? `↓ ${speed}` : "ещё не измерена"}`);
  /* Страна УЗЛА, к которому подключаемся, — не та, что обещает имя профиля.
     У подписок с разделением вход/выход они почти всегда разные (на живом
     роутере совпали у 5 профилей из 83), поэтому строка подписана явно и
     показывается только когда действительно отличается от флага в имени. */
  const nodeCc = (r.cc || "").toUpperCase();
  if (nodeCc && nodeCc !== ccFromName(r.name)) {
    out.push(`Узел подключения: ${countryName(nodeCc) || nodeCc}`);
  }
  return out;
});

/* ---------- список ---------- */

async function reload(force = true) {
  await store.load(force);
  void store.loadProbes();
}

function openRow(r: ProfileRow) {
  rowItem.value = r;
  rowOpen.value = true;
}

async function connect(r: ProfileRow) {
  if (store.switching) return;
  rowOpen.value = false;
  try {
    await store.activate(r.id);
    toast.ok(`Подключено: ${r.name}`);
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось переключить профиль");
  }
}

/* «Стоп» у активного профиля в списке — тот же самый, что на «Обзоре»:
   останавливается служба целиком, потому что активный профиль в ней один. */
async function stopActive(r: ProfileRow) {
  rowOpen.value = false;
  try {
    await diag.singboxStop();
    toast.ok(`Остановлено: ${r.name} — трафик идёт напрямую`);
    void status.refresh(true);
    void store.load(true);
  } catch (e) {
    toast.fromError(e, "Не удалось остановить");
  }
}

async function pingOne(r: ProfileRow) {
  if (probing.value) return;
  probing.value = r.id;
  try {
    const res = await diag.pingCheck(r.id);
    store.ping[r.id] = res;
    const rtt = res.rtt ?? -1;
    toast.info(
      res.ok
        ? `${r.name}: ${rtt > 0 ? `${Math.round(rtt)} мс` : "отвечает"}`
        : `${r.name}: не отвечает`,
    );
  } catch (e) {
    toast.fromError(e, "Пинг не удался");
  } finally {
    probing.value = "";
  }
}

async function healthOne(r: ProfileRow) {
  if (probing.value) return;
  probing.value = r.id;
  toast.info(`Проверяю ${r.name} — это занимает 5–10 секунд`);
  try {
    /* Вердикт лежит в result.ok: внешнее ok — это «запрос обработан», оно true
       и для профиля, который проверку не прошёл. */
    const res = (await diag.healthCheckOne(r.id)) as unknown as {
      result?: { ok?: boolean; dl?: number };
    };
    await store.loadProbes();
    const passed = res.result?.ok ?? store.health[r.id]?.ok === true;
    /* Заодно замеряется скорость — если она есть, показываем сразу: ради неё
       эту проверку чаще всего и запускают вручную. */
    const speed = fmtSpeedKbps(res.result?.dl ?? store.health[r.id]?.dl);
    toast[passed ? "ok" : "error"](
      passed
        ? `${r.name}: проверка пройдена${speed ? ` · ↓ ${speed}` : ""}`
        : `${r.name}: проверка не пройдена`,
    );
  } catch (e) {
    toast.fromError(e, "Проверка не удалась");
  } finally {
    probing.value = "";
  }
}

/** Флаг одного профиля: то же, что массовая операция, но на один id. */
async function setFlag(r: ProfileRow, kind: "autoswitch" | "speedcheck", value: boolean) {
  if (flagBusy.value) return;
  flagBusy.value = `${r.id}:${kind}`;
  try {
    if (kind === "autoswitch") await profilesApi.setAutoswitch([r.id], value);
    else await profilesApi.setSpeedcheck([r.id], value);
    /* Правим строку на месте: перечитывать сотню профилей и все пинги ради
       одного флага — заметная пауза на каждое нажатие. */
    const item = store.items.find((p) => p.id === r.id);
    if (item) item[kind] = value;
    /* Шторка держит снимок строки, а не саму строку списка — без этого тумблер
       в ней остался бы в прежнем положении. */
    if (rowItem.value?.id === r.id) rowItem.value[kind] = value;
  } catch (e) {
    toast.fromError(e, "Не удалось изменить настройку профиля");
  } finally {
    flagBusy.value = "";
  }
}

/**
 * Определить страны эндпоинтов (detour-geo). Долгая операция: сотня nslookup-ов,
 * плюс 11 МБ базы, если попались незнакомые адреса, — поэтому явный тост о
 * начале и перезагрузка списка в конце, а не молчаливое ожидание.
 */
async function scanGeo() {
  if (geoBusy.value) return;
  geoBusy.value = true;
  toast.info("Определяю страны — это займёт до минуты");
  try {
    const st = await profilesApi.geoScan();
    await store.load();
    const known = Number(st?.known ?? 0);
    if (known > 0) toast.ok(`Страна определена у ${known} профилей`);
    else toast.error(st?.error || "Страны определить не удалось");
  } catch (e) {
    toast.fromError(e, "Не удалось определить страны");
  } finally {
    geoBusy.value = false;
  }
}

async function editRow(r: ProfileRow) {
  rowOpen.value = false;
  busy.value = "open";
  try {
    const raw = await profilesApi.get(r.id);
    sheetDraft.value = draftFromProfile(raw);
    sheetOpen.value = true;
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать профиль");
  } finally {
    busy.value = "";
  }
}

function addProfile() {
  tab.value = "profiles";
  sheetDraft.value = null;
  sheetOpen.value = true;
}

async function saveProfile(payload: { draft: ProfileDraft; activate: boolean }) {
  busy.value = "save";
  try {
    const profile = profileFromDraft(payload.draft);
    await profilesApi.save(profile);
    toast.ok("Профиль сохранён");
    sheetOpen.value = false;
    await reload(true);
    if (payload.activate) {
      const id = String(profile.id);
      await store.activate(id);
      toast.ok(`Подключено: ${payload.draft.name}`);
      void status.refresh(true);
    }
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить профиль");
  } finally {
    busy.value = "";
  }
}

async function removeRow(r: ProfileRow) {
  if (!window.confirm(`Удалить профиль «${r.name}»?`)) return;
  rowOpen.value = false;
  busy.value = "del";
  try {
    await profilesApi.remove(r.id);
    toast.ok("Профиль удалён");
    selected.value = selected.value.filter((id) => id !== r.id);
    await reload(true);
  } catch (e) {
    /* Роутер отказывает, если профиль активен или входит в цепочку —
       показываем его формулировку, она объясняет причину. */
    toast.fromError(e, "Не удалось удалить профиль");
  } finally {
    busy.value = "";
  }
}

/* ---------- массовые действия ---------- */

async function bulkFlag(kind: "autoswitch" | "speedcheck", eligible: boolean) {
  if (!selected.value.length) return;
  busy.value = `bulk:${kind}`;
  try {
    if (kind === "autoswitch") await profilesApi.setAutoswitch(selected.value, eligible);
    else await profilesApi.setSpeedcheck(selected.value, eligible);
    const what = kind === "autoswitch" ? "авто-переключении" : "проверке скорости";
    toast.ok(
      `${selected.value.length} профилей ${eligible ? "участвуют" : "не участвуют"} в ${what}`,
    );
    await reload(true);
  } catch (e) {
    toast.fromError(e, "Не удалось изменить настройку");
  } finally {
    busy.value = "";
  }
}

async function bulkDelete() {
  const ids = [...selected.value];
  if (!ids.length) return;
  if (!window.confirm(`Удалить выбранные профили (${ids.length})? Отменить будет нельзя.`)) return;
  busy.value = "bulk:del";
  let done = 0;
  const failed: string[] = [];
  try {
    for (const id of ids) {
      try {
        await profilesApi.remove(id);
        done++;
      } catch {
        failed.push(id);
      }
    }
    selected.value = failed;
    if (failed.length) {
      toast.error(
        `Удалено ${done}, отказано ${failed.length}: активный профиль и хопы цепочек роутер удалять не даёт`,
      );
    } else {
      toast.ok(`Удалено профилей: ${done}`);
    }
    await reload(true);
  } finally {
    busy.value = "";
  }
}

/* ---------- папки (группы) ---------- */

/* Папка — это строковое поле `group` у самого профиля, отдельной сущности на
   роутере нет. Отсюда и всё поведение: «переименовать» — перезаписать поле у
   всех профилей папки, «удалить» — очистить его (профили остаются, просто без
   папки), а новая папка возникает в тот момент, когда в неё переносят первый
   профиль. Пустых папок поэтому не существует: старая панель держала их в
   sessionStorage, то есть они пропадали при закрытии вкладки. */

/** Служебное значение в выборе папки. Такое имя папки запрещаем при вводе. */
const NEW_FOLDER = "__new__";

const moveOpen = ref(false);
const moveTarget = ref("");
const moveName = ref("");
const foldersOpen = ref(false);
const renamingFrom = ref("");
const renameTo = ref("");
/* Роутер правит профили по одному, а их в папке бывает несколько десятков —
   без счётчика операция выглядит как зависшая кнопка. */
const groupProgress = ref("");

const folders = computed(() =>
  groups.value.map((name) => ({ name, count: idsInGroup(name).length })),
);

function idsInGroup(name: string): string[] {
  return store.rows.filter((r) => (r.group || "") === name).map((r) => r.id);
}

/** Проставить папку списку профилей. Возвращает число успешно сохранённых. */
async function applyGroup(ids: string[], group: string): Promise<number> {
  let done = 0;
  for (let i = 0; i < ids.length; i++) {
    groupProgress.value = `Сохраняю ${i + 1} из ${ids.length}`;
    try {
      await profilesApi.setGroup(ids[i], group);
      done++;
    } catch {
      /* Один нечитаемый профиль не должен обрывать остальные — сколько прошло,
         скажем в тосте. */
    }
  }
  groupProgress.value = "";
  return done;
}

function openMove() {
  /* Если папок ещё нет, единственный осмысленный вариант — создать первую. */
  moveTarget.value = groups.value[0] ?? NEW_FOLDER;
  moveName.value = "";
  moveOpen.value = true;
}

async function moveSelected() {
  const ids = [...selected.value];
  if (!ids.length) return;
  let target = moveTarget.value;
  if (target === NEW_FOLDER) {
    target = moveName.value.trim();
    if (!target) {
      toast.error("Введите название папки");
      return;
    }
    if (target === NEW_FOLDER) {
      toast.error("Такое название занято служебным значением — выберите другое");
      return;
    }
  }
  busy.value = "bulk:group";
  try {
    const done = await applyGroup(ids, target);
    const where = target ? `в «${target}»` : "из папок";
    toast[done === ids.length ? "ok" : "error"](
      done === ids.length
        ? `Перенесено профилей ${where}: ${done}`
        : `Перенесено ${done} из ${ids.length} — остальные роутер не отдал`,
    );
    moveOpen.value = false;
    await reload(true);
  } finally {
    busy.value = "";
  }
}

function startRename(name: string) {
  renamingFrom.value = name;
  renameTo.value = name;
}

async function renameFolder() {
  const from = renamingFrom.value;
  const to = renameTo.value.trim();
  if (!to || to === from) {
    renamingFrom.value = "";
    return;
  }
  if (to === NEW_FOLDER) {
    toast.error("Такое название занято служебным значением — выберите другое");
    return;
  }
  const ids = idsInGroup(from);
  /* Имя существующей папки — это не ошибка, а слияние; но молча сливать чужие
     профили нельзя, поэтому спрашиваем. */
  if (
    groups.value.includes(to) &&
    !window.confirm(`Папка «${to}» уже есть — профили (${ids.length}) сложатся вместе. Продолжить?`)
  ) {
    return;
  }
  busy.value = `folder:${from}`;
  try {
    const done = await applyGroup(ids, to);
    toast[done === ids.length ? "ok" : "error"](
      done === ids.length
        ? `Папка переименована в «${to}» (профилей: ${done})`
        : `Переименовано ${done} из ${ids.length} профилей — остальные роутер не отдал`,
    );
    renamingFrom.value = "";
    await reload(true);
  } finally {
    busy.value = "";
  }
}

async function deleteFolder(name: string) {
  const ids = idsInGroup(name);
  if (
    !window.confirm(
      `Удалить папку «${name}»? Профили (${ids.length}) останутся, но окажутся без папки.`,
    )
  ) {
    return;
  }
  busy.value = `folder:${name}`;
  try {
    const done = await applyGroup(ids, "");
    toast[done === ids.length ? "ok" : "error"](
      done === ids.length
        ? `Папка «${name}» удалена, профилей освобождено: ${done}`
        : `Освобождено ${done} из ${ids.length} профилей — остальные роутер не отдал`,
    );
    if (renamingFrom.value === name) renamingFrom.value = "";
    await reload(true);
  } finally {
    busy.value = "";
  }
}

/* ---------- импорт и экспорт ---------- */

async function exportAll() {
  busy.value = "export";
  try {
    const data = await profilesApi.exportAll();
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `detour-profiles-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
    toast.ok("Файл с профилями сохранён");
  } catch (e) {
    toast.fromError(e, "Экспорт не удался");
  } finally {
    busy.value = "";
  }
}

async function importFile(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (!file) return;
  busy.value = "import";
  try {
    const parsed = JSON.parse(await file.text()) as unknown;
    const list = Array.isArray(parsed)
      ? parsed
      : ((parsed as { profiles?: unknown[] }).profiles ?? []);
    if (!Array.isArray(list) || !list.length) {
      toast.error("В файле не нашлось профилей");
      return;
    }
    let done = 0;
    for (const p of list) {
      if (!p || typeof p !== "object") continue;
      try {
        await profilesApi.save(p as Record<string, unknown>);
        done++;
      } catch {
        /* один битый профиль не должен обрывать импорт остальных */
      }
    }
    toast.ok(`Загружено профилей: ${done} из ${list.length}`);
    await reload(true);
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать файл");
  } finally {
    busy.value = "";
  }
}

/* ---------- переходы из палитры команд ---------- */

async function goChains() {
  tab.value = "chains";
  await nextTick();
  chainsRef.value?.openNew();
}

async function goSubs() {
  tab.value = "subs";
  await nextTick();
  subsRef.value?.openNew();
}

let unregister: (() => void) | undefined;

onMounted(async () => {
  unregister = commands.register([
    {
      id: "pr:new",
      title: "Новый профиль",
      group: "профили",
      keywords: "добавить vless trojan ссылка",
      run: () => addProfile(),
    },
    {
      id: "pr:export",
      title: "Экспортировать профили в файл",
      group: "профили",
      keywords: "сохранить json бэкап",
      run: () => void exportAll(),
    },
    {
      id: "pr:reload",
      title: "Перечитать профили",
      group: "профили",
      keywords: "обновить список",
      run: () => void reload(true),
    },
    {
      id: "pr:folders",
      title: "Управление папками профилей",
      group: "профили",
      keywords: "группы переименовать удалить папку",
      run: () => {
        tab.value = "profiles";
        foldersOpen.value = true;
      },
    },
    {
      id: "pr:chain",
      title: "Новая цепочка",
      group: "профили",
      keywords: "хопы двойной прыжок",
      run: () => void goChains(),
    },
    {
      id: "pr:sub",
      title: "Добавить подписку",
      group: "профили",
      keywords: "subscription ссылка список серверов",
      run: () => void goSubs(),
    },
    {
      id: "pr:subs-all",
      title: "Обновить все подписки",
      group: "профили",
      run: () => {
        tab.value = "subs";
        void nextTick().then(() => subsRef.value?.refreshAll());
      },
    },
  ]);
  await store.load();
  void store.loadProbes();
  void store.loadChains();
});

onBeforeUnmount(() => unregister?.());
</script>

<template>
  <div class="tabs scroll-x">
    <SegmentedControl v-model="tab" label="Что показывать" :options="tabs" />
  </div>

  <div v-show="tab === 'profiles'" class="pane">
    <TileCard title="Профили">
      <p v-if="store.loading && !store.rows.length" class="lead">Читаю список профилей…</p>
      <ProfileList
        v-else
        v-model:selected="selected"
        :rows="store.rows"
        :switching="store.switching"
        :probing="probing"
        :targets="healthTargets"
        :flag-busy="flagBusy"
        :geo-busy="geoBusy"
        @open="openRow"
        @connect="connect"
        @stop="stopActive"
        @ping="pingOne"
        @health="healthOne"
        @flag="setFlag($event.row, $event.kind, $event.value)"
        @geoscan="scanGeo"
      />
      <template #actions>
        <UiButton variant="primary" @click="addProfile">Добавить профиль</UiButton>
        <UiButton :busy="busy === 'export'" @click="exportAll">Экспорт в файл</UiButton>
        <UiButton :busy="busy === 'import'" @click="fileInput?.click()">Импорт из файла</UiButton>
        <UiButton :busy="store.loading" @click="reload(true)">Обновить</UiButton>
        <!-- Флаги «участвует в проверке / в замерах» стоят на профилях здесь, а
             сама проверка (расписание, цели, объём замера) настраивается в
             «Журнале» — без этой кнопки связь между экранами не видна. -->
        <UiButton @click="$router.push({ path: '/journal', query: { focus: 'health' } })">
          Настройки проверки
        </UiButton>
        <UiButton :disabled="!folders.length" @click="foldersOpen = true">
          Управление папками
        </UiButton>
        <input
          ref="fileInput"
          class="hidden-file"
          type="file"
          accept="application/json,.json"
          @change="importFile"
        />
      </template>
    </TileCard>

    <div v-if="selected.length" class="bulk" role="group" aria-label="Действия над выбранными">
      <span class="bcount">Выбрано: {{ selected.length }}</span>
      <UiButton :busy="busy === 'bulk:group'" @click="openMove">Перенести в папку</UiButton>
      <UiButton :busy="busy === 'bulk:autoswitch'" @click="bulkFlag('autoswitch', true)">
        Разрешить авто-переключение
      </UiButton>
      <UiButton :busy="busy === 'bulk:autoswitch'" @click="bulkFlag('autoswitch', false)">
        Запретить авто-переключение
      </UiButton>
      <UiButton :busy="busy === 'bulk:speedcheck'" @click="bulkFlag('speedcheck', true)">
        Разрешить проверку скорости
      </UiButton>
      <UiButton :busy="busy === 'bulk:speedcheck'" @click="bulkFlag('speedcheck', false)">
        Запретить проверку скорости
      </UiButton>
      <UiButton variant="danger" :busy="busy === 'bulk:del'" @click="bulkDelete">
        Удалить выбранные
      </UiButton>
      <UiButton @click="selected = []">Снять выделение</UiButton>
    </div>
  </div>

  <div v-show="tab === 'chains'" class="pane">
    <ChainsPanel ref="chainsRef" />
  </div>

  <div v-show="tab === 'subs'" class="pane">
    <SubscriptionsPanel ref="subsRef" />
  </div>

  <!-- WARP смонтирован всегда: по его же ответу решается, показывать ли вкладку. -->
  <div class="pane" :class="{ hidden: tab !== 'warp' }">
    <WarpPanel @supported="warpSupported = $event" />
  </div>

  <ProfileSheet
    :open="sheetOpen"
    :draft="sheetDraft"
    :busy="busy === 'save'"
    :groups="groups"
    @close="sheetOpen = false"
    @save="saveProfile"
  />

  <DrawerSheet
    :open="rowOpen"
    :title="rowItem?.name ?? 'Профиль'"
    @close="rowOpen = false"
  >
    <ul v-if="rowItem" class="info">
      <!-- Ключ по индексу: у двух целей проверки могут совпасть и подпись, и
           задержка, а одинаковые строки-ключи Vue не переживает. -->
      <li v-for="(line, i) in rowState" :key="i">{{ line }}</li>
    </ul>
    <!-- На телефоне иконки в строке слишком мелкие, поэтому те же два флага
         дублируются здесь полноразмерными тумблерами. -->
    <div v-if="rowItem" class="rowflags">
      <SwitchToggle
        :model-value="rowItem.autoswitch !== false"
        label="Участвует в авто-переключении"
        :busy="flagBusy === `${rowItem.id}:autoswitch`"
        @update:model-value="setFlag(rowItem, 'autoswitch', $event)"
      />
      <SwitchToggle
        :model-value="rowItem.speedcheck !== false"
        label="Участвует в замерах скорости"
        :busy="flagBusy === `${rowItem.id}:speedcheck`"
        @update:model-value="setFlag(rowItem, 'speedcheck', $event)"
      />
    </div>
    <div v-if="rowItem" class="rowacts">
      <UiButton
        variant="primary"
        :disabled="!!store.switching || rowItem.isActive"
        @click="connect(rowItem)"
      >
        {{ rowItem.isActive ? "Уже подключён" : "Подключить" }}
      </UiButton>
      <UiButton :busy="probing === rowItem.id" @click="pingOne(rowItem)">Проверить пинг</UiButton>
      <UiButton :busy="probing === rowItem.id" @click="healthOne(rowItem)">
        Проверить работу и скорость
      </UiButton>
      <UiButton :busy="busy === 'open'" @click="editRow(rowItem)">Править</UiButton>
      <UiButton variant="danger" :busy="busy === 'del'" @click="removeRow(rowItem)">
        Удалить
      </UiButton>
    </div>
  </DrawerSheet>

  <!-- Перенос выбранных профилей в папку: та же операция, что и правка поля
       «Папка» в самом профиле, только сразу по всему выделению. -->
  <DrawerSheet :open="moveOpen" title="Перенести в папку" @close="moveOpen = false">
    <p class="lead">
      Выбрано профилей: {{ selected.length }}. Папка — это метка у профиля, на
      подключение она не влияет.
    </p>
    <div class="mgrid">
      <PField label="Куда перенести">
        <select v-model="moveTarget">
          <option value="">Без папки</option>
          <option v-for="g in groups" :key="g" :value="g">{{ g }}</option>
          <option :value="NEW_FOLDER">Новая папка…</option>
        </select>
      </PField>
      <PField
        v-if="moveTarget === NEW_FOLDER"
        label="Название новой папки"
        hint="Папка появится, как только в неё попадёт первый профиль"
      >
        <input
          v-model="moveName"
          type="text"
          spellcheck="false"
          placeholder="Например, Европа"
          @keydown.enter.prevent="moveSelected"
        />
      </PField>
    </div>
    <p v-if="groupProgress" class="prog">{{ groupProgress }}</p>
    <template #footer>
      <UiButton
        variant="primary"
        :busy="busy === 'bulk:group'"
        :disabled="!selected.length"
        @click="moveSelected"
      >
        Перенести
      </UiButton>
      <UiButton @click="moveOpen = false">Отмена</UiButton>
    </template>
  </DrawerSheet>

  <!-- Папки целиком: переименование и удаление. Создание — переносом профилей
       (см. шторку выше): папки без профилей роутер не хранит. -->
  <DrawerSheet :open="foldersOpen" title="Папки профилей" @close="foldersOpen = false">
    <p class="lead">
      Переименование меняет папку у всех её профилей, удаление папки профили не
      удаляет — они остаются без папки. Новая папка создаётся переносом:
      выделите профили в списке и нажмите «Перенести в папку».
    </p>
    <p v-if="!folders.length" class="lead">Пока ни один профиль не разложен по папкам.</p>
    <ul v-else class="folders">
      <li v-for="f in folders" :key="f.name">
        <template v-if="renamingFrom === f.name">
          <input
            v-model="renameTo"
            class="rn"
            type="text"
            spellcheck="false"
            :aria-label="`Новое название папки ${f.name}`"
            @keydown.enter.prevent="renameFolder"
            @keydown.esc="renamingFrom = ''"
          />
          <UiButton variant="primary" :busy="busy === `folder:${f.name}`" @click="renameFolder">
            Сохранить
          </UiButton>
          <UiButton @click="renamingFrom = ''">Отмена</UiButton>
        </template>
        <template v-else>
          <span class="fname">
            {{ f.name }}
            <small>{{ f.count }} профилей</small>
          </span>
          <UiButton :disabled="!!busy" @click="startRename(f.name)">Переименовать</UiButton>
          <UiButton
            variant="danger"
            :busy="busy === `folder:${f.name}`"
            :disabled="!!busy && busy !== `folder:${f.name}`"
            @click="deleteFolder(f.name)"
          >
            Удалить
          </UiButton>
        </template>
      </li>
    </ul>
    <p v-if="groupProgress" class="prog">{{ groupProgress }}</p>
    <template #footer>
      <UiButton @click="foldersOpen = false">Закрыть</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.tabs {
  margin-bottom: 14px;
  padding-bottom: 2px;
}
.pane {
  /* Колонка, а не сетка: карточки раздела всегда во всю ширину, и ни одна из
     них не может случайно растянуть страницу вбок. */
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}
.pane.hidden {
  display: none;
}
.lead {
  font-size: 13px;
  color: var(--dim);
}
.bulk {
  position: sticky;
  bottom: 12px;
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
  border: 1px solid var(--line-2);
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--ground) 92%, transparent);
  backdrop-filter: blur(10px);
  padding: 10px 12px;
  box-shadow: var(--shadow);
}
.bcount {
  font-size: 13px;
  color: var(--dim);
  margin-right: 4px;
}
.hidden-file {
  display: none;
}
.info {
  list-style: none;
  margin: 0 0 14px;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13.5px;
  color: var(--dim);
}
.rowflags {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-bottom: 14px;
  padding-bottom: 14px;
  border-bottom: 1px solid var(--line);
}
.rowacts {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.rowacts :deep(.btn) {
  justify-content: center;
  min-height: 46px;
}
.mgrid {
  display: grid;
  gap: 12px;
  margin-bottom: 12px;
}
/* Счётчик прогресса: роутер сохраняет профили по одному, и на папке из
   нескольких десятков профилей операция идёт заметное время. */
.prog {
  font-size: 12.5px;
  color: var(--faint);
  font-variant-numeric: tabular-nums;
}
.folders {
  list-style: none;
  margin: 10px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.folders li {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
}
.fname {
  flex: 1 1 140px;
  min-width: 0;
  font-size: 14px;
  overflow-wrap: anywhere;
}
.fname small {
  display: block;
  font-size: 11.5px;
  color: var(--faint);
}
.rn {
  flex: 1 1 140px;
  min-width: 0;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 8px 10px;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  min-height: 44px;
  outline: none;
}
.rn:focus {
  border-color: var(--accent);
}

@media (max-width: 860px) {
  /* Нижний бар разделов не должен перекрывать панель массовых действий. */
  .bulk {
    bottom: calc(var(--tabbar) + 12px + env(safe-area-inset-bottom));
  }
}
</style>
