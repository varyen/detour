<script setup lang="ts">
/* Подписки: роутер сам скачивает список серверов и раскладывает его профилями
   в указанную группу. Файл подписки на роутере хранит поля title/autoupdate/
   interval_hours — их и пишем, а лишние поля исходной записи сохраняем как
   есть, чтобы правка в панели не стирала то, чего панель не показывает
   (last_* отчёты фонового обновления, apply_routing и прочее). */
import { computed, onMounted, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import PField from "@/components/profiles/PField.vue";
import QrScanner from "@/components/QrScanner.vue";
import { slugify } from "@/components/profiles/uri";
import { cameraSupported, decodeFile } from "@/lib/qr";
import { parseSubscriptionLink } from "@/lib/sublink";
import { subscriptions } from "@/api";
import type { Subscription } from "@/api";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";
import { asNum, fmtAgo, fmtBytes, fmtInt } from "@/lib/format";
import { useStatusStore } from "@/stores/status";

const status = useStatusStore();

/** Запись подписки на диске шире, чем то, что объявлено в контракте панели. */
type SubRecord = Subscription & {
  title?: string;
  autoupdate?: boolean;
  interval_hours?: number;
  apply_routing?: boolean;
  last_status?: string;
  last_saved?: number;
  /** Каким маршрутом дошёл последний фид: "vpn" | "direct". */
  last_via?: string;
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

/* Пресеты User-Agent. Поставщики отдают разным клиентам разный формат: одному
   sing-box-JSON, другому base64-список ссылок, — и когда подписка «приходит
   пустой», дело обычно в UA. Пресет не заменяет поле, а заполняет его: строку
   всё равно можно поправить руками. Значения совпадают со старой панелью, чтобы
   запись читалась там же (она хранит выбор в `user_agent_preset`). */
const UA_PRESETS: { key: string; label: string; ua: string }[] = [
  { key: "singbox", label: "sing-box", ua: "sing-box/1.13.2" },
  { key: "happ", label: "Happ", ua: "Happ/1.43.0" },
  { key: "v2rayng", label: "v2rayNG", ua: "v2rayNG/1.8.27" },
  { key: "clash", label: "ClashMetaForAndroid", ua: "ClashMetaForAndroid/2.11.13.Meta" },
];

const uaPreset = ref("custom");

/** Какому пресету отвечает строка в поле; «custom» — своя или пустая. */
function presetForUa(ua?: string): string {
  const v = (ua ?? "").trim();
  return UA_PRESETS.find((p) => p.ua === v)?.key ?? "custom";
}

/* Строку правят и руками — тогда выбор пресета обязан честно съехать на «свой»,
   иначе список показывал бы Happ там, где написано что-то своё. */
watch(
  () => draft.value.user_agent,
  (v) => {
    uaPreset.value = presetForUa(v);
  },
);

function pickUaPreset(key: string) {
  uaPreset.value = key;
  const p = UA_PRESETS.find((x) => x.key === key);
  /* «Свой» ничего не подставляет и НЕ чистит поле: стирать уже введённую
     строку при выборе этого варианта — потеря данных. */
  if (p) draft.value.user_agent = p.ua;
}

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

/** `url` — ссылка уже есть (QR из формы профиля): сразу пробуем её загрузить. */
function openNew(url?: string, name?: string) {
  draft.value = blank();
  uaPreset.value = presetForUa(draft.value.user_agent);
  isNew.value = true;
  probe.value = "";
  probeLines.value = [];
  sheetOpen.value = true;
  if (url) {
    draft.value.url = url;
    if (name) draft.value.title = name;
    void tryFetch();
  }
}

/* В поле ссылки вставили обёртку (sing-box://…, happ://add/…) — заменяем её
   вложенным адресом, название из обёртки — в «Название». */
watch(
  () => draft.value.url,
  (v) => {
    if (!isNew.value || /^https?:\/\//i.test(v.trim())) return;
    const sub = parseSubscriptionLink(v);
    if (!sub) return;
    if ("error" in sub) {
      probe.value = sub.error;
      return;
    }
    draft.value.url = sub.url;
    if (sub.name && !draft.value.title) draft.value.title = sub.name;
    void tryFetch();
  },
);

/* ---------- QR со ссылкой подписки ---------- */
const canCamera = cameraSupported();
const scanOpen = ref(false);
const qrBusy = ref(false);
const imgEl = ref<HTMLInputElement | null>(null);

function takeQr(text: string) {
  scanOpen.value = false;
  const sub = parseSubscriptionLink(text) ?? (/^https?:\/\//i.test(text.trim()) ? { url: text.trim(), name: "" } : null);
  if (!sub) {
    probe.value = "В QR-коде не ссылка подписки. Если это ссылка на один сервер — добавьте её как профиль";
    return;
  }
  if ("error" in sub) {
    probe.value = sub.error;
    return;
  }
  draft.value.url = sub.url;
  if (sub.name && !draft.value.title) draft.value.title = sub.name;
  void tryFetch();
}

async function pickImage(ev: Event) {
  const input = ev.target as HTMLInputElement;
  const f = input.files?.[0];
  input.value = "";
  scanOpen.value = false;
  if (!f) return;
  qrBusy.value = true;
  try {
    const text = await decodeFile(f);
    if (text) takeQr(text);
    else probe.value = "На картинке не нашёлся QR-код";
  } finally {
    qrBusy.value = false;
  }
}

/* Новой подписке идентификатор и группу можно не придумывать: берутся из
   названия (поставщик присылает его в заголовке) или из адреса. */
function autoFill() {
  if (!isNew.value) return;
  let host = "";
  try {
    host = new URL(draft.value.url.trim()).hostname;
  } catch {
    host = "";
  }
  if (!draft.value.title?.trim() && host) draft.value.title = host;
  const base = draft.value.title?.trim() || host;
  if (!draft.value.id.trim() && base) {
    const taken = new Set(items.value.map((x) => x.id));
    let id = slugify(base).replace(/[^a-zA-Z0-9._-]/g, "_");
    for (let n = 2; taken.has(id); n++) id = `${slugify(base)}_${n}`;
    draft.value.id = id;
  }
  if (!draft.value.group?.trim() && base) draft.value.group = base;
}

function openEdit(s: SubRecord) {
  draft.value = { ...s };
  uaPreset.value = presetForUa(draft.value.user_agent);
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
    autoFill();
  } catch (e) {
    probe.value = e instanceof Error ? e.message : "Не удалось загрузить";
    autoFill();
  } finally {
    busy.value = "";
  }
}

/**
 * Сохранить запись. `refreshAfter` — сразу же дёрнуть обновление этой подписки:
 * добавили ссылку и хотим профили сейчас, а не через сутки по расписанию. Это
 * два запроса подряд (отдельного «сохранить и обновить» у роутера нет), поэтому
 * неудача обновления сообщается отдельно — запись уже сохранена.
 */
async function save(refreshAfter = false) {
  autoFill();
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
  busy.value = refreshAfter ? "save-refresh" : "save";
  try {
    await subscriptions.saveOne({
      ...draft.value,
      id,
      url,
      group: draft.value.group.trim(),
      title: draft.value.title?.trim() ?? "",
      user_agent: draft.value.user_agent?.trim() ?? "",
      /* Выбранный пресет пишем рядом со строкой: по одной строке нельзя
         отличить «свой UA» от пресета, который поставщик потом сменит. */
      user_agent_preset: uaPreset.value,
      interval_hours: intervalOf(draft.value),
      autoupdate: autoOf(draft.value),
      apply_routing: draft.value.apply_routing === true,
    } as Subscription);
    sheetOpen.value = false;
    if (!refreshAfter) toast.ok("Подписка сохранена");
    await load();
    if (!refreshAfter) return;
    toast.info("Сохранено, забираю список серверов — это может занять минуту");
    try {
      await subscriptions.refreshOne(id);
      toast.ok("Подписка сохранена и обновлена");
      await load();
      await store.load(true);
    } catch (e) {
      /* Отдельный текст: запись на роутере уже лежит, повторно сохранять нечего
         — достаточно нажать «Обновить» в списке. */
      toast.fromError(e, "Подписка сохранена, но обновить её не удалось");
    }
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
  if (!window.confirm(`Удалить подписку «${labelOf(s)}»?`)) return;
  /* Профили подписки — это её группа. Без группы их не отличить от своих,
     поэтому тогда и не предлагаем. */
  const group = s.group || "";
  const ids = group ? store.idsInGroup(group) : [];
  const withProfiles =
    ids.length > 0 &&
    window.confirm(
      `Удалить заодно все профили этой подписки (${ids.length}, группа «${group}»)?\n` +
        "«Отмена» — подписка удалится, профили останутся.",
    );
  busy.value = `del:${s.id}`;
  try {
    await subscriptions.deleteOne(s.id);
    if (withProfiles) {
      const r = await store.removeMany(ids);
      toast[r.failed.length ? "error" : "ok"](
        r.failed.length
          ? `Подписка удалена, профилей удалено ${r.done} из ${ids.length}: подключённый и хопы цепочек удалить нельзя`
          : `Подписка удалена вместе с профилями (${r.done})`,
      );
      await store.load(true);
    } else {
      toast.ok("Подписка удалена");
    }
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
  <TileCard icon="rss" title="Подписки">
    <p class="lead">
      Подписка — ссылка поставщика, из которой
      {{ status.isClient ? "приложение само" : "роутер сам" }} достаёт список серверов и
      раскладывает их профилями в указанную группу.
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
            <!-- Настройка меняет маршрутизацию всего роутера, а не только эту
                 подписку, — про включённую стоит знать из списка. -->
            <span v-if="s.apply_routing">применяет routing поставщика</span>
            <!-- Адрес подписки берётся сначала через активный VPN и только потом
                 напрямую. Отмечаем лишь первый случай: он означает, что напрямую
                 панель поставщика, скорее всего, недоступна. -->
            <span v-if="s.last_via === 'vpn'">пришла через VPN</span>
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
      <UiButton variant="primary" @click="openNew()">Добавить подписку</UiButton>
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
    <div v-if="isNew" class="qrrow">
      <UiButton v-if="canCamera" @click="scanOpen = true">Сканировать QR-код</UiButton>
      <UiButton :busy="qrBusy" @click="imgEl?.click()">{{ canCamera ? "QR с картинки" : "QR-код: фото или картинка" }}</UiButton>
      <input ref="imgEl" type="file" accept="image/*" hidden @change="pickImage" />
    </div>
    <div class="grid">
      <PField label="Ссылка подписки" wide>
        <input
          v-model="draft.url"
          type="url"
          spellcheck="false"
          placeholder="https://panel.example.com/sub/token"
        />
      </PField>

      <PField
        label="Идентификатор"
        :hint="`Имя файла на ${status.isClient ? 'устройстве' : 'роутере'}: латиница, цифры, . _ -`"
      >
        <input v-model="draft.id" type="text" :disabled="!isNew" spellcheck="false" :placeholder="isNew ? 'авто' : ''" />
      </PField>

      <PField label="Название" hint="Как подписку показывать в панели">
        <input v-model="draft.title" type="text" placeholder="Например, Example VPN" />
      </PField>

      <PField label="Группа профилей" hint="В неё попадут скачанные профили. Пусто — по названию подписки">
        <input v-model="draft.group" type="text" :placeholder="isNew ? 'авто' : 'Например, Example VPN'" />
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

      <PField label="Клиент для подписки" hint="Подставляет готовую строку User-Agent">
        <select
          :value="uaPreset"
          @change="pickUaPreset(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="p in UA_PRESETS" :key="p.key" :value="p.key">{{ p.label }}</option>
          <option value="custom">свой</option>
        </select>
      </PField>

      <PField
        label="User-Agent"
        hint="Некоторые поставщики отдают разный формат разным клиентам. Пусто — sing-box"
      >
        <input v-model="draft.user_agent" type="text" spellcheck="false" placeholder="sing-box" />
      </PField>
    </div>

    <div class="flags">
      <SwitchToggle
        :model-value="draft.apply_routing === true"
        label="Применять routing из подписки"
        hint="Режим маршрутизации и список исключений берутся из ответа поставщика (all-except + bypass) и перезаписывают текущие"
        @update:model-value="draft.apply_routing = $event"
      />
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
      <!-- Новой подписке нужнее сразу забрать серверы — эта кнопка главная. -->
      <UiButton :variant="isNew ? undefined : 'primary'" :busy="busy === 'save'" @click="save()">Сохранить</UiButton>
      <UiButton :variant="isNew ? 'primary' : undefined" :busy="busy === 'save-refresh'" @click="save(true)">
        Сохранить и обновить
      </UiButton>
      <UiButton @click="sheetOpen = false">Отмена</UiButton>
    </template>
  </DrawerSheet>
  <QrScanner :open="scanOpen" @result="takeQr" @close="scanOpen = false" @image="imgEl?.click()" />
</template>

<style scoped>
.qrrow {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 12px;
}
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
/* Тумблер вне сетки полей: он занимает всю ширину и с двумя строками подписи в
   колонку 210px не влезает. */
.flags {
  margin-top: 14px;
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
