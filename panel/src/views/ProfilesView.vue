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
import { fmtAgo } from "@/lib/format";

type Tab = "profiles" | "chains" | "subs" | "warp";

const store = useProfilesStore();
const status = useStatusStore();
const toast = useToastStore();
const commands = useCommandStore();

const tab = ref<Tab>("profiles");
const selected = ref<string[]>([]);
const busy = ref("");
const probing = ref("");
const warpSupported = ref(false);

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
  }
  if (r.autoswitch === false) out.push("Исключён из авто-переключения");
  if (r.speedcheck === false) out.push("Исключён из проверки скорости");
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
      result?: { ok?: boolean };
    };
    await store.loadProbes();
    const passed = res.result?.ok ?? store.health[r.id]?.ok === true;
    toast[passed ? "ok" : "error"](
      passed ? `${r.name}: проверка пройдена` : `${r.name}: проверка не пройдена`,
    );
  } catch (e) {
    toast.fromError(e, "Проверка не удалась");
  } finally {
    probing.value = "";
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
        @open="openRow"
        @connect="connect"
      />
      <template #actions>
        <UiButton variant="primary" @click="addProfile">Добавить профиль</UiButton>
        <UiButton :busy="busy === 'export'" @click="exportAll">Экспорт в файл</UiButton>
        <UiButton :busy="busy === 'import'" @click="fileInput?.click()">Импорт из файла</UiButton>
        <UiButton :busy="store.loading" @click="reload(true)">Обновить</UiButton>
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
      <li v-for="line in rowState" :key="line">{{ line }}</li>
    </ul>
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
        Проверить работоспособность
      </UiButton>
      <UiButton :busy="busy === 'open'" @click="editRow(rowItem)">Править</UiButton>
      <UiButton variant="danger" :busy="busy === 'del'" @click="removeRow(rowItem)">
        Удалить
      </UiButton>
    </div>
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
.rowacts {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.rowacts :deep(.btn) {
  justify-content: center;
  min-height: 46px;
}

@media (max-width: 860px) {
  /* Нижний бар разделов не должен перекрывать панель массовых действий. */
  .bulk {
    bottom: calc(var(--tabbar) + 12px + env(safe-area-inset-bottom));
  }
}
</style>
