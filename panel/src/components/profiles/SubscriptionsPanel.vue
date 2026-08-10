<script setup lang="ts">
/* Подписки: роутер сам скачивает список серверов и раскладывает его профилями
   в указанную группу. Файл подписки на роутере хранит поля title/autoupdate/
   interval_hours — их и пишем, а лишние поля исходной записи сохраняем как
   есть, чтобы правка в панели не стирала то, чего панель не показывает
   (last_* отчёты фонового обновления, apply_routing и прочее). */
import { computed, onMounted, ref } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import PField from "@/components/profiles/PField.vue";
import { subscriptions } from "@/api";
import type { Subscription } from "@/api";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";
import { asNum, fmtAgo, fmtBytes, fmtInt } from "@/lib/format";

/** Запись подписки на диске шире, чем то, что объявлено в контракте панели. */
type SubRecord = Subscription & {
  title?: string;
  autoupdate?: boolean;
  interval_hours?: number;
  apply_routing?: boolean;
  last_status?: string;
  last_saved?: number;
};

const store = useProfilesStore();
const toast = useToastStore();

const items = ref<SubRecord[]>([]);
const loading = ref(false);
const busy = ref("");
const sheetOpen = ref(false);
const isNew = ref(true);
const draft = ref<SubRecord>(blank());
const probe = ref("");
const probeLines = ref<string[]>([]);

function blank(): SubRecord {
  return {
    id: "",
    url: "",
    group: "",
    title: "",
    autoupdate: true,
    interval_hours: 24,
    user_agent: "",
  };
}

function labelOf(s: SubRecord): string {
  return s.name || s.title || s.id;
}

function countOf(s: SubRecord): number {
  return asNum(s.count ?? s.last_saved, 0);
}

function intervalOf(s: SubRecord): number {
  return asNum(s.interval_hours ?? s.interval, 24) || 24;
}

function autoOf(s: SubRecord): boolean {
  return (s.autoupdate ?? s.enabled ?? true) !== false;
}

const anyBusy = computed(() => busy.value !== "");

async function load() {
  loading.value = true;
  try {
    const r = await subscriptions.list();
    items.value = (r.subscriptions ?? []) as SubRecord[];
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать список подписок");
  } finally {
    loading.value = false;
  }
}

function openNew() {
  draft.value = blank();
  isNew.value = true;
  probe.value = "";
  probeLines.value = [];
  sheetOpen.value = true;
}

function openEdit(s: SubRecord) {
  draft.value = { ...s };
  isNew.value = false;
  probe.value = "";
  probeLines.value = [];
  sheetOpen.value = true;
}

/** «upload=…; download=…; total=…; expire=…» — там и лежат лимиты трафика. */
function readUserinfo(raw: string): string[] {
  const out: string[] = [];
  const map: Record<string, number> = {};
  for (const part of raw.split(";")) {
    const [k, v] = part.split("=");
    if (!k || v === undefined) continue;
    map[k.trim().toLowerCase()] = Number(v.trim());
  }
  const used = (map.upload || 0) + (map.download || 0);
  if (map.total) {
    out.push(`Трафик: ${fmtBytes(used)} из ${fmtBytes(map.total)}`);
  } else if (used) {
    out.push(`Трафик: ${fmtBytes(used)}`);
  }
  if (map.expire) {
    out.push(`Действует до ${new Date(map.expire * 1000).toLocaleDateString("ru-RU")}`);
  }
  return out;
}

async function tryFetch() {
  const url = draft.value.url.trim();
  if (!url) return;
  busy.value = "probe";
  probe.value = "Загружаю…";
  probeLines.value = [];
  try {
    const r = await subscriptions.fetch(url, draft.value.user_agent || undefined);
    const h = r.headers ?? {};
    const lines: string[] = [];
    if (h["profile-title"]) lines.push(`Название у поставщика: ${h["profile-title"]}`);
    if (h["subscription-userinfo"]) lines.push(...readUserinfo(h["subscription-userinfo"]));
    if (h["profile-update-interval"]) {
      lines.push(`Рекомендованный интервал: ${h["profile-update-interval"]} ч`);
    }
    if (h["content-type"]) lines.push(`Формат ответа: ${h["content-type"]}`);
    const bytes = (r.body ?? "").length;
    probe.value = `Ответ получен, ${fmtInt(bytes)} символов.`;
    probeLines.value = lines;
    if (!draft.value.title && h["profile-title"]) draft.value.title = h["profile-title"];
  } catch (e) {
    probe.value = e instanceof Error ? e.message : "Не удалось загрузить";
  } finally {
    busy.value = "";
  }
}

async function save() {
  const id = draft.value.id.trim();
  const url = draft.value.url.trim();
  if (!/^[a-zA-Z0-9._-]+$/.test(id)) {
    toast.error("Идентификатор: только латиница, цифры и знаки . _ -");
    return;
  }
  if (!/^https?:\/\//.test(url)) {
    toast.error("Ссылка должна начинаться с http:// или https://");
    return;
  }
  if (!draft.value.group?.trim()) {
    toast.error("Укажите группу — в неё попадут профили из подписки");
    return;
  }
  busy.value = "save";
  try {
    await subscriptions.saveOne({
      ...draft.value,
      id,
      url,
      group: draft.value.group.trim(),
      title: draft.value.title?.trim() ?? "",
      user_agent: draft.value.user_agent?.trim() ?? "",
      interval_hours: intervalOf(draft.value),
      autoupdate: autoOf(draft.value),
    } as Subscription);
    toast.ok("Подписка сохранена");
    sheetOpen.value = false;
    await load();
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить подписку");
  } finally {
    busy.value = "";
  }
}

async function refreshOne(s: SubRecord) {
  busy.value = `up:${s.id}`;
  try {
    await subscriptions.refreshOne(s.id);
    toast.ok(`Подписка «${labelOf(s)}» обновлена`);
    await load();
    await store.load(true);
  } catch (e) {
    toast.fromError(e, "Обновление не удалось");
  } finally {
    busy.value = "";
  }
}

async function refreshAll() {
  if (!items.value.length) return;
  if (
    !window.confirm(
      "Обновить все подписки? Роутер обойдёт их по очереди — это может занять несколько минут.",
    )
  )
    return;
  busy.value = "all";
  toast.info("Обновляю все подписки — это надолго, не закрывайте вкладку");
  try {
    await subscriptions.refreshAll();
    toast.ok("Все подписки обновлены");
    await load();
    await store.load(true);
  } catch (e) {
    toast.fromError(e, "Обновление всех подписок не удалось");
  } finally {
    busy.value = "";
  }
}

async function remove(s: SubRecord) {
  if (!window.confirm(`Удалить подписку «${labelOf(s)}»? Уже загруженные профили останутся.`))
    return;
  busy.value = `del:${s.id}`;
  try {
    await subscriptions.deleteOne(s.id);
    toast.ok("Подписка удалена");
    await load();
  } catch (e) {
    toast.fromError(e, "Не удалось удалить подписку");
  } finally {
    busy.value = "";
  }
}

onMounted(load);

defineExpose({ openNew, refreshAll, reload: load });
</script>

<template>
  <TileCard title="Подписки">
    <p class="lead">
      Подписка — ссылка поставщика, из которой роутер сам достаёт список серверов
      и раскладывает их профилями в указанную группу.
    </p>

    <p v-if="loading && !items.length" class="empty">Читаю список…</p>
    <p v-else-if="!items.length" class="empty">Подписок нет.</p>

    <ul v-else class="subs">
      <li v-for="s in items" :key="s.id">
        <div class="st">
          <p class="sname">
            {{ labelOf(s) }}
            <span v-if="!autoOf(s)" class="tag">без автообновления</span>
          </p>
          <p class="url mono">{{ s.url }}</p>
          <p class="meta">
            <span v-if="s.group">группа {{ s.group }}</span>
            <span v-if="countOf(s)">{{ fmtInt(countOf(s)) }} профилей</span>
            <span v-if="s.last_refresh">обновлена {{ fmtAgo(s.last_refresh) }}</span>
            <span>раз в {{ intervalOf(s) }} ч</span>
          </p>
          <p v-if="s.last_error" class="err">Последняя ошибка: {{ s.last_error }}</p>
        </div>
        <div class="sact">
          <UiButton :busy="busy === `up:${s.id}`" :disabled="anyBusy" @click="refreshOne(s)">
            Обновить
          </UiButton>
          <UiButton :disabled="anyBusy" @click="openEdit(s)">Править</UiButton>
          <UiButton
            variant="danger"
            :busy="busy === `del:${s.id}`"
            :disabled="anyBusy"
            @click="remove(s)"
          >
            Удалить
          </UiButton>
        </div>
      </li>
    </ul>

    <template #actions>
      <UiButton variant="primary" @click="openNew">Добавить подписку</UiButton>
      <UiButton :busy="busy === 'all'" :disabled="anyBusy || !items.length" @click="refreshAll">
        Обновить все
      </UiButton>
      <span v-if="busy === 'all'" class="hint">Идёт обход подписок, это может занять минуты</span>
    </template>
  </TileCard>

  <DrawerSheet
    :open="sheetOpen"
    :title="isNew ? 'Новая подписка' : `Подписка: ${labelOf(draft)}`"
    wide
    @close="sheetOpen = false"
  >
    <div class="grid">
      <PField label="Ссылка подписки" wide>
        <input
          v-model="draft.url"
          type="url"
          spellcheck="false"
          placeholder="https://panel.example.com/sub/token"
        />
      </PField>

      <PField label="Идентификатор" hint="Имя файла на роутере: латиница, цифры, . _ -">
        <input v-model="draft.id" type="text" :disabled="!isNew" spellcheck="false" />
      </PField>

      <PField label="Название" hint="Как подписку показывать в панели">
        <input v-model="draft.title" type="text" placeholder="Например, Example VPN" />
      </PField>

      <PField label="Группа профилей" hint="Обязательна: в неё попадут скачанные профили">
        <input v-model="draft.group" type="text" placeholder="Например, Example VPN" />
      </PField>

      <PField label="Интервал обновления, часов">
        <input
          :value="intervalOf(draft)"
          type="text"
          inputmode="numeric"
          @input="draft.interval_hours = asNum(($event.target as HTMLInputElement).value, 24)"
        />
      </PField>

      <PField label="Автообновление">
        <select
          :value="autoOf(draft) ? '1' : '0'"
          @change="draft.autoupdate = ($event.target as HTMLSelectElement).value === '1'"
        >
          <option value="1">включено</option>
          <option value="0">выключено</option>
        </select>
      </PField>

      <PField
        label="User-Agent"
        hint="Некоторые поставщики отдают разный формат разным клиентам. Пусто — sing-box"
        wide
      >
        <input v-model="draft.user_agent" type="text" spellcheck="false" placeholder="sing-box" />
      </PField>
    </div>

    <div class="probe">
      <UiButton :busy="busy === 'probe'" :disabled="!draft.url.trim()" @click="tryFetch">
        Пробная загрузка
      </UiButton>
      <p v-if="probe" class="pnote">{{ probe }}</p>
      <ul v-if="probeLines.length" class="plist">
        <li v-for="l in probeLines" :key="l">{{ l }}</li>
      </ul>
    </div>

    <template #footer>
      <UiButton variant="primary" :busy="busy === 'save'" @click="save">Сохранить</UiButton>
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
.subs {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.subs li {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.st {
  flex: 1 1 240px;
  min-width: 0;
}
.sname {
  font-size: 14.5px;
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.tag {
  font-size: 10.5px;
  color: var(--dim);
  border: 1px solid var(--line-2);
  border-radius: 999px;
  padding: 1px 7px;
}
.url {
  font-size: 11.5px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.meta {
  font-size: 12px;
  color: var(--dim);
  display: flex;
  gap: 4px 12px;
  flex-wrap: wrap;
}
.err {
  font-size: 12px;
  color: var(--bad);
  overflow-wrap: anywhere;
}
.sact {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
.grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
}
.probe {
  margin-top: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: flex-start;
}
.pnote {
  font-size: 13px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.plist {
  margin: 0;
  padding-left: 18px;
  font-size: 13px;
  color: var(--ink);
}
</style>
