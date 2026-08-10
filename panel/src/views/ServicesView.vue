<script setup lang="ts">
/* «Сервисы и доступ» — всё, что касается роутера снаружи и самой панели:
   публикация домашних устройств в интернет, свой домен с защищённым
   соединением, уведомления в браузер, аппаратное ускорение, файл подкачки,
   резервная копия настроек и вход в панель.

   Экран собран списком областей, а не сеткой карточек: областей семь, часть из
   них на конкретном роутере вообще недоступна, и длинный список коротких строк
   переживает узкий экран лучше любой сетки. */
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import ServicePanel from "@/components/services/ServicePanel.vue";
import PortmapSheet from "@/components/services/PortmapSheet.vue";
import CertSheet from "@/components/services/CertSheet.vue";
import KeenHttpsSheet from "@/components/services/KeenHttpsSheet.vue";
import PasswordSheet from "@/components/services/PasswordSheet.vue";
import FormField from "@/components/services/FormField.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { overview, poll, services } from "@/api";
import type { LanClient, SwapStatus } from "@/api";
import { useStatusStore } from "@/stores/status";
import { useSessionStore } from "@/stores/session";
import { useToastStore } from "@/stores/toast";
import { useCommandStore } from "@/stores/commands";
import { fmtAgo, fmtDate, fmtInt } from "@/lib/format";
import { getRegistration, swSupported } from "@/pwa";

const status = useStatusStore();
const session = useSessionStore();
const toast = useToastStore();
const commands = useCommandStore();

/* Роутер отдаёт больше полей, чем описано в общем контракте (он один на все
   разделы). Читаем их здесь явно, а не расширяем контракт ради одного экрана. */
function wide<T>(v: unknown): T {
  return v as T;
}

type PortmapMode = "https" | "dnat";

interface PortmapRow {
  id: string;
  name?: string;
  enabled: boolean;
  mode: PortmapMode;
  listen_port: number;
  proto: string;
  target_ip: string;
  target_port: number;
  scheme?: string;
  src?: string;
  auth_user?: string;
  auth?: boolean;
  listening?: boolean;
}

interface PortmapCaps {
  engine?: string;
  https_supported?: boolean;
  https_reason?: string;
  dnat_supported?: boolean;
  dnat_reason?: string;
  auth_supported?: boolean;
  auth_reason?: string;
  domain?: string;
  lan_cidr?: string;
  mappings?: PortmapRow[];
  entries?: PortmapRow[];
}

interface CertState {
  domain?: string;
  email?: string;
  target?: string;
  expiry?: string;
  last_result?: string;
  last_error?: string;
  last_ts?: number;
}

/** Отправитель уведомлений сообщает о канале доставки строкой, а не флагом. */
interface PushCfg {
  available?: boolean;
  vapid_public?: string;
  sub_count?: number;
  bypass?: string;
}

interface CertDetect {
  acme_present?: boolean;
  http80?: string;
  target?: string;
  wan_ip?: string;
  domain?: string;
  expiry?: string;
  serving_ok?: boolean;
  serving_cn?: string;
}

interface OffloadState {
  supported?: boolean;
  mode?: "auto" | "notify" | "off";
  last_action?: string;
  last_result?: string;
  last_sirq?: number;
  last_ts?: number;
}

interface PortmapCheck {
  target_up?: boolean;
  listening?: boolean;
  wan_packets?: number;
  note?: string;
}

interface PortmapExternal {
  verdict?: string;
  message?: string;
  wan_ip?: string;
}

/* ---------------- состояние экрана ---------------- */

const open = reactive<Record<string, boolean>>({
  portmap: false,
  cert: false,
  push: false,
  offload: false,
  swap: false,
  backup: false,
  account: false,
});

/** Какое действие сейчас выполняется: одна строка на весь экран. */
const busy = ref("");

const pm = ref<PortmapCaps | null>(null);
const clients = ref<LanClient[]>([]);
const cert = ref<CertState | null>(null);
const certInfo = ref<CertDetect | null>(null);
const pushCfg = ref<PushCfg | null>(null);
const pushSub = ref<PushSubscription | null>(null);
const pushPerm = ref<NotificationPermission>("default");
const offload = ref<OffloadState | null>(null);
const swap = ref<SwapStatus | null>(null);

const secureContext = ref(true);
const checks = reactive<Record<string, string>>({});
const confirmDelete = ref("");
const confirmSwap = ref(false);
const swapSize = ref("512");
const swapProgress = ref("");
const importFile = ref<File | null>(null);
/* Файл разбираем сразу при выборе, а не при нажатии «Восстановить»: человек
   должен увидеть, что именно будет перезаписано, ДО перезаписи. */
const importDoc = ref<Record<string, unknown> | null>(null);
const importSections = ref<string[]>([]);
const importError = ref("");
const confirmImport = ref(false);

const sheetPortmap = ref(false);
const sheetCert = ref(false);
const sheetPassword = ref(false);
const sheetKeenHttps = ref(false);
const editing = ref<PortmapRow | null>(null);

const fileInput = ref<HTMLInputElement | null>(null);

/* ---------------- загрузка ---------------- */

async function loadPortmap() {
  pm.value = wide<PortmapCaps | null>(await services.portmapStatus());
}

async function loadCert() {
  const [s, d] = await Promise.allSettled([services.certStatus(), services.certDetect()]);
  if (s.status === "fulfilled") cert.value = wide<CertState | null>(s.value);
  if (d.status === "fulfilled") certInfo.value = wide<CertDetect | null>(d.value);
}

async function loadPush() {
  pushCfg.value = wide<PushCfg | null>(await services.pushConfig());
  await refreshPushSub();
}

async function loadOffload() {
  offload.value = wide<OffloadState | null>(await services.offloadStatus());
}

async function loadSwap() {
  swap.value = await services.swapStatus();
}

async function loadClients() {
  try {
    const r = await overview.lanClients();
    clients.value = r.clients ?? [];
  } catch {
    /* Список устройств — только подсказка для формы, без него она работает. */
  }
}

async function loadAll() {
  secureContext.value = window.isSecureContext;
  if ("Notification" in window) pushPerm.value = Notification.permission;
  await Promise.allSettled([
    loadPortmap(),
    loadCert(),
    loadPush(),
    loadOffload(),
    loadSwap(),
    loadClients(),
  ]);
}

/* ---------------- проброс сервисов ---------------- */

const rows = computed<PortmapRow[]>(() => pm.value?.mappings ?? pm.value?.entries ?? []);
const enabledRows = computed(() => rows.value.filter((r) => r.enabled).length);

const portmapSummary = computed(() => {
  if (!pm.value) return "Не удалось прочитать настройки";
  if (!rows.value.length) return "Снаружи ничего не опубликовано";
  return `${enabledRows.value} из ${rows.value.length} открыто наружу`;
});

const httpsSupported = computed(() => pm.value?.https_supported === true);
const dnatSupported = computed(() => pm.value?.dnat_supported === true);
const authSupported = computed(() => pm.value?.auth_supported === true);
const anyPortmapMode = computed(() => httpsSupported.value || dnatSupported.value);

function rowTitle(r: PortmapRow): string {
  return r.name || r.id;
}

function rowPath(r: PortmapRow): string {
  const via =
    r.mode === "https"
      ? `защищённо${pm.value?.domain ? ` на ${pm.value.domain}` : ""}`
      : `${r.proto.toUpperCase().replace(" ", " и ")}`;
  return `порт ${r.listen_port} (${via}) → ${r.target_ip}:${r.target_port}`;
}

/* Ссылка на опубликованный сервис — только для защищённого режима: у проброса
   порта на другом конце может быть что угодно, не обязательно веб. Домена может
   не быть — тогда ведём на хост, по которому открыта сама панель: этот же
   роутер и отвечает на опубликованном порту. */
function rowUrl(r: PortmapRow): string {
  if (r.mode !== "https") return "";
  const host = pm.value?.domain || location.hostname;
  return `https://${host}:${r.listen_port}/`;
}

function rowNote(r: PortmapRow): string {
  const bits: string[] = [];
  if (r.src === "lan") bits.push("только своя сеть");
  if (r.auth) bits.push(`вход по паролю: ${r.auth_user}`);
  if (r.mode === "https" && r.enabled && r.listening === false) {
    bits.push("порт не слушается — нажмите «Применить заново»");
  }
  return bits.join(" · ");
}

function addPortmap() {
  editing.value = null;
  open.portmap = true;
  sheetPortmap.value = true;
}

async function onPortmapSaved() {
  toast.ok("Доступ сохранён");
  await loadPortmap();
}

function editPortmap(r: PortmapRow) {
  editing.value = r;
  sheetPortmap.value = true;
}

async function togglePortmap(r: PortmapRow, on: boolean) {
  busy.value = `pm:${r.id}`;
  try {
    await services.portmapToggle(r.id, on);
    toast.ok(on ? `${rowTitle(r)} открыт наружу` : `${rowTitle(r)} закрыт`);
    await loadPortmap();
  } catch (e) {
    toast.fromError(e, "Не удалось переключить");
  } finally {
    busy.value = "";
  }
}

async function deletePortmap(r: PortmapRow) {
  busy.value = `pm:${r.id}`;
  try {
    await services.portmapDelete(r.id);
    confirmDelete.value = "";
    delete checks[r.id];
    toast.ok(`${rowTitle(r)} удалён`);
    await loadPortmap();
  } catch (e) {
    toast.fromError(e, "Не удалось удалить");
  } finally {
    busy.value = "";
  }
}

async function applyPortmap() {
  busy.value = "pm:apply";
  try {
    await services.portmapApply();
    toast.ok("Настройки применены заново");
    await loadPortmap();
  } catch (e) {
    toast.fromError(e, "Не удалось применить настройки");
  } finally {
    busy.value = "";
  }
}

async function checkPortmap(r: PortmapRow) {
  busy.value = `check:${r.id}`;
  checks[r.id] = "Проверяю…";
  try {
    const res = wide<PortmapCheck>(await services.portmapCheck(r.id));
    const parts: string[] = [];
    parts.push(
      res.target_up
        ? `устройство ${r.target_ip}:${r.target_port} отвечает`
        : res.note || "устройство не отвечает",
    );
    if (r.mode === "https") {
      parts.push(res.listening ? "роутер слушает порт" : "роутер порт не слушает");
    }
    if (typeof res.wan_packets === "number" && res.wan_packets >= 0) {
      parts.push(`снаружи пришло пакетов: ${fmtInt(res.wan_packets)}`);
    }
    checks[r.id] = parts.join(" · ");
  } catch (e) {
    checks[r.id] = e instanceof Error ? e.message : "Проверка не удалась";
  } finally {
    busy.value = "";
  }
}

async function checkPortmapExternal(r: PortmapRow) {
  busy.value = `ext:${r.id}`;
  checks[r.id] = "Проверяю снаружи — до тридцати секунд…";
  try {
    const res = wide<PortmapExternal>(await services.portmapCheckExternal(r.id));
    checks[r.id] = res.message || res.verdict || "Проверка завершилась без ответа";
  } catch (e) {
    checks[r.id] = e instanceof Error ? e.message : "Проверка снаружи не удалась";
  } finally {
    busy.value = "";
  }
}

/* ---------------- сертификат ---------------- */

const certOk = computed(() => !!cert.value?.domain && cert.value.last_result === "ok");

const certSummary = computed(() => {
  const d = cert.value?.domain || certInfo.value?.domain;
  if (!d) return "Панель открывается только по локальному адресу";
  const exp = cert.value?.expiry || certInfo.value?.expiry;
  if (cert.value?.last_result === "error") {
    return `${d} — последняя попытка не удалась`;
  }
  return exp ? `${d} — действует до ${fmtDate(exp)}` : d;
});

const certChip = computed(() => {
  if (cert.value?.last_result === "issuing") return "выпускается";
  if (certOk.value) return "есть";
  if (cert.value?.last_result === "error") return "ошибка";
  return "нет";
});

const certTone = computed<"ok" | "warn" | "bad" | undefined>(() => {
  if (certOk.value) return "ok";
  if (cert.value?.last_result === "error") return "bad";
  return "warn";
});

const certMismatch = computed(
  () => certInfo.value?.serving_ok === false && !!certInfo.value?.serving_cn,
);

async function refreshCert() {
  busy.value = "cert";
  try {
    await loadCert();
  } finally {
    busy.value = "";
  }
}

/* ---------------- уведомления ---------------- */

async function currentRegistration(): Promise<ServiceWorkerRegistration | null> {
  if (!("serviceWorker" in navigator)) return null;
  const known = getRegistration();
  if (known) return known;
  try {
    return (await navigator.serviceWorker.getRegistration()) ?? null;
  } catch {
    return null;
  }
}

async function refreshPushSub() {
  try {
    const reg = await currentRegistration();
    pushSub.value = reg ? await reg.pushManager.getSubscription() : null;
  } catch {
    pushSub.value = null;
  }
}

/** Ключ отправителя приходит в виде base64url — браузеру нужны байты. */
function keyToBytes(b64url: string) {
  const pad = "=".repeat((4 - (b64url.length % 4)) % 4);
  const b64 = (b64url + pad).replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(b64);
  const bytes = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

const pushReady = computed(
  () => secureContext.value && swSupported.value && pushCfg.value?.available === true,
);

const pushSummary = computed(() => {
  if (!secureContext.value || !swSupported.value) {
    return "Нужен защищённый доступ к панели — по обычному адресу браузер их не отдаёт";
  }
  if (pushCfg.value?.available === false) return "Роутер пока не умеет их отправлять";
  if (pushPerm.value === "denied") return "Браузер запретил уведомления для этой панели";
  if (pushSub.value) return "Этот браузер получает уведомления";
  const n = pushCfg.value?.sub_count ?? 0;
  return n
    ? `Здесь выключены, на других устройствах подписок: ${fmtInt(n)}`
    : "Никто не подписан";
});

async function pushOn() {
  busy.value = "push";
  try {
    const key = pushCfg.value?.vapid_public;
    if (!key) throw new Error("Роутер не отдал ключ отправителя");
    const perm = await Notification.requestPermission();
    pushPerm.value = perm;
    if (perm !== "granted") throw new Error("Браузер не разрешил уведомления");
    const reg = await currentRegistration();
    if (!reg) {
      throw new Error("Панель ещё не готова принимать уведомления — обновите страницу");
    }
    const sub = await reg.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: keyToBytes(key),
    });
    await services.pushSubscribe(sub.toJSON());
    pushSub.value = sub;
    toast.ok("Уведомления включены");
    await loadPush();
  } catch (e) {
    toast.fromError(e, "Не удалось включить уведомления");
  } finally {
    busy.value = "";
  }
}

async function pushOff() {
  busy.value = "push";
  try {
    const sub = pushSub.value;
    if (!sub) throw new Error("Подписки в этом браузере нет");
    await services.pushUnsubscribe(sub.endpoint);
    await sub.unsubscribe().catch(() => false);
    pushSub.value = null;
    toast.ok("Уведомления выключены");
    await loadPush();
  } catch (e) {
    toast.fromError(e, "Не удалось выключить уведомления");
  } finally {
    busy.value = "";
  }
}

async function pushTest() {
  busy.value = "push-test";
  try {
    const r = await services.pushTest();
    toast.ok(
      r.sent ? `Отправлено получателям: ${fmtInt(r.sent)}` : "Отправлять некому — никто не подписан",
    );
  } catch (e) {
    toast.fromError(e, "Не удалось отправить");
  } finally {
    busy.value = "";
  }
}

/* ---------------- аппаратное ускорение ---------------- */

const offloadSupported = computed(() => offload.value?.supported === true);

const offloadMode = computed({
  get: () => offload.value?.mode ?? "auto",
  set: (m: "auto" | "notify" | "off") => void setOffload(m),
});

const offloadSummary = computed(() => {
  const s = offload.value;
  if (!s) return "";
  const when = s.last_ts ? fmtAgo(s.last_ts) : "";
  const state =
    s.last_result === "healthy"
      ? "ускорение работает"
      : s.last_result
        ? `последняя проверка: ${s.last_result}`
        : "проверок ещё не было";
  return when ? `${state}, ${when}` : state;
});

async function setOffload(mode: "auto" | "notify" | "off") {
  busy.value = "offload";
  try {
    offload.value = wide<OffloadState>(await services.offloadSet(mode));
    toast.ok(
      mode === "off"
        ? "Роутер больше не следит за ускорением"
        : mode === "notify"
          ? "Роутер предупредит, но чинить не станет"
          : "Роутер починит ускорение сам",
    );
  } catch (e) {
    toast.fromError(e, "Не удалось изменить режим");
    await loadOffload();
  } finally {
    busy.value = "";
  }
}

async function kickOffload() {
  busy.value = "offload-kick";
  try {
    await services.offloadKick();
    toast.ok("Ускоритель перезапущен");
    await loadOffload();
  } catch (e) {
    toast.fromError(e, "Не удалось перезапустить ускоритель");
  } finally {
    busy.value = "";
  }
}

/* ---------------- файл подкачки ---------------- */

const swapSupported = computed(() => swap.value?.supported === true);

const swapSummary = computed(() => {
  const s = swap.value;
  if (!s) return "";
  if (!s.exists) return "Файла подкачки нет — при нехватке памяти панель может падать";
  return `${fmtInt(s.size_mb ?? 0)} МБ${s.active ? ", используется" : ", не подключён"}`;
});

async function createSwap() {
  const size = Number(swapSize.value);
  if (!Number.isInteger(size) || size < 64 || size > 4096) {
    toast.error("Размер должен быть от 64 до 4096 МБ");
    return;
  }
  busy.value = "swap";
  swapProgress.value = "Готовлю файл — на флешке это может занять несколько минут";
  try {
    await services.swapCreate(size);
    const final = await poll(() => services.swapStatus(), {
      intervalMs: 3000,
      timeoutMs: 900_000,
      done: (s) => s?.busy !== true,
      onTick: (s) => {
        if (s) swap.value = s;
      },
    });
    swapProgress.value = "";
    if (final?.last_result === "ok") toast.ok("Файл подкачки создан и подключён");
    else toast.error(final?.last_message || "Создать файл подкачки не удалось");
  } catch (e) {
    swapProgress.value = "";
    toast.fromError(e, "Создать файл подкачки не удалось");
  } finally {
    busy.value = "";
    await loadSwap();
  }
}

async function removeSwap() {
  busy.value = "swap";
  try {
    await services.swapRemove();
    confirmSwap.value = false;
    toast.ok("Файл подкачки удалён");
    await loadSwap();
  } catch (e) {
    toast.fromError(e, "Не удалось удалить файл подкачки");
  } finally {
    busy.value = "";
  }
}

/* ---------------- резервная копия ---------------- */

async function exportConfig() {
  busy.value = "export";
  try {
    const data = await services.exportConfig();
    const blob = new Blob([JSON.stringify(data, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `detour-${new Date().toISOString().slice(0, 10)}.json`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 2000);
    toast.ok("Копия сохранена на устройство");
  } catch (e) {
    toast.fromError(e, "Не удалось выгрузить настройки");
  } finally {
    busy.value = "";
  }
}

/* Разделы, которые роутер действительно умеет восстанавливать. Всё прочее в
   файле он молча пропустит, поэтому и обещать этого не нужно: показываем ровно
   то, что будет перезаписано. */
const SECTION_LABELS: Record<string, string> = {
  settings: "настройки панели",
  subscription: "подписка на профили",
  proxy_domains: "список доменов для VPN",
  whitelist_domains: "список исключений",
  zapret_conf: "параметры обхода DPI",
  zapret_domains: "домены для обхода DPI",
};

function sectionsOf(doc: Record<string, unknown>): string[] {
  return Object.keys(SECTION_LABELS).filter((k) => {
    const v = doc[k];
    if (typeof v === "string") return v.length > 0;
    /* Пустой объект подписки роутер тоже не пишет — не обещаем его. */
    if (v && typeof v === "object") return Object.keys(v).length > 0;
    return false;
  });
}

function resetImport() {
  importFile.value = null;
  importDoc.value = null;
  importSections.value = [];
  importError.value = "";
  confirmImport.value = false;
  if (fileInput.value) fileInput.value.value = "";
}

async function pickFile(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0] ?? null;
  importFile.value = file;
  importDoc.value = null;
  importSections.value = [];
  importError.value = "";
  confirmImport.value = false;
  if (!file) return;
  try {
    const doc: unknown = JSON.parse(await file.text());
    if (!doc || typeof doc !== "object" || Array.isArray(doc)) {
      importError.value = "Это не файл настроек панели";
      return;
    }
    const found = sectionsOf(doc as Record<string, unknown>);
    if (!found.length) {
      importError.value = "В файле нет разделов, которые панель умеет восстанавливать";
      return;
    }
    importDoc.value = doc as Record<string, unknown>;
    importSections.value = found;
  } catch {
    importError.value = "Файл повреждён или это не JSON";
  }
}

async function importConfig() {
  const doc = importDoc.value;
  if (!doc) return;
  busy.value = "import";
  try {
    await services.importConfig(doc);
    resetImport();
    toast.ok("Настройки восстановлены");
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось восстановить настройки");
  } finally {
    busy.value = "";
  }
}

/* ---------------- палитра команд ---------------- */

let unregister: (() => void) | undefined;

onMounted(async () => {
  unregister = commands.register([
    {
      id: "svc:portmap-add",
      title: "Открыть доступ к устройству снаружи",
      group: "сервисы",
      keywords: "проброс порт публикация",
      run: () => addPortmap(),
    },
    {
      id: "svc:portmap-apply",
      title: "Применить настройки доступа заново",
      group: "сервисы",
      keywords: "проброс порт",
      run: () => void applyPortmap(),
    },
    {
      id: "svc:cert",
      title: "Выпустить сертификат для своего домена",
      group: "сервисы",
      keywords: "https сертификат домен",
      run: () => {
        open.cert = true;
        sheetCert.value = true;
      },
    },
    {
      id: "svc:push-test",
      title: "Отправить пробное уведомление",
      group: "сервисы",
      keywords: "push уведомления",
      available: () => pushReady.value,
      run: () => void pushTest(),
    },
    {
      id: "svc:offload-kick",
      title: "Перезапустить аппаратное ускорение",
      group: "сервисы",
      keywords: "offload скорость",
      available: () => offloadSupported.value,
      run: () => void kickOffload(),
    },
    {
      id: "svc:export",
      title: "Скачать резервную копию настроек",
      group: "сервисы",
      keywords: "бэкап экспорт",
      run: () => void exportConfig(),
    },
    {
      id: "svc:password",
      title: "Сменить пароль от панели",
      group: "доступ",
      keywords: "логин учётная запись",
      run: () => {
        open.account = true;
        sheetPassword.value = true;
      },
    },
  ]);
  await loadAll();
});

onBeforeUnmount(() => unregister?.());
</script>

<template>
  <div class="areas">
    <!-- ===== проброс сервисов ===== -->
    <ServicePanel
      v-model:open="open.portmap"
      title="Доступ к домашним устройствам снаружи"
      :summary="portmapSummary"
      :chip="enabledRows ? `${enabledRows} открыто` : 'закрыто'"
      :tone="enabledRows ? 'warn' : undefined"
    >
      <p class="lead">
        Роутер может пускать в дом из интернета: открыть веб-сервис по защищённому
        адресу или просто отдать наружу порт устройства. Открывайте только то, что
        действительно нужно снаружи — всё открытое видно всему интернету.
      </p>

      <p v-if="!anyPortmapMode" class="note warn">
        Сейчас открыть ничего нельзя.
        <template v-if="pm?.https_reason">Защищённый доступ: {{ pm.https_reason }}.</template>
        <template v-if="pm?.dnat_reason"> Проброс порта: {{ pm.dnat_reason }}.</template>
      </p>

      <p v-else-if="!rows.length" class="note">
        Пока ничего не опубликовано.
      </p>

      <p v-if="httpsSupported && pm?.engine" class="note faint">
        Защищённый доступ роутер выдаёт через {{ pm.engine }}<template v-if="pm?.domain">
          на домене {{ pm.domain }}</template>.
      </p>

      <div v-for="r in rows" :key="r.id" class="row">
        <SwitchToggle
          :model-value="r.enabled"
          :label="rowTitle(r)"
          :hint="rowPath(r)"
          :busy="busy === `pm:${r.id}`"
          @update:model-value="togglePortmap(r, $event)"
        />
        <p v-if="rowNote(r)" class="row-note">{{ rowNote(r) }}</p>
        <a
          v-if="rowUrl(r)"
          class="row-link"
          :href="rowUrl(r)"
          target="_blank"
          rel="noopener"
        >{{ rowUrl(r) }}</a>
        <p v-if="checks[r.id]" class="row-check">{{ checks[r.id] }}</p>
        <div class="row-actions">
          <UiButton @click="editPortmap(r)">Изменить</UiButton>
          <UiButton :busy="busy === `check:${r.id}`" @click="checkPortmap(r)">
            Проверить
          </UiButton>
          <UiButton
            v-if="r.enabled && r.src !== 'lan'"
            :busy="busy === `ext:${r.id}`"
            @click="checkPortmapExternal(r)"
          >
            Проверить снаружи
          </UiButton>
          <template v-if="confirmDelete === r.id">
            <UiButton
              variant="danger"
              :busy="busy === `pm:${r.id}`"
              @click="deletePortmap(r)"
            >
              Точно удалить
            </UiButton>
            <UiButton @click="confirmDelete = ''">Отмена</UiButton>
          </template>
          <UiButton v-else variant="danger" @click="confirmDelete = r.id">
            Удалить
          </UiButton>
        </div>
      </div>

      <p v-if="rows.length" class="note faint">
        Проверка снаружи занимает до тридцати секунд и обращается к стороннему
        сервису проверки портов. Наружу уходит только внешний адрес роутера и номер
        порта — ни домена, ни адреса устройства, ни паролей.
      </p>

      <div class="actions">
        <UiButton variant="primary" :disabled="!anyPortmapMode" @click="addPortmap">
          Добавить
        </UiButton>
        <UiButton :busy="busy === 'pm:apply'" @click="applyPortmap">
          Применить заново
        </UiButton>
      </div>
    </ServicePanel>

    <!-- ===== сертификат ===== -->
    <ServicePanel
      v-model:open="open.cert"
      title="Свой домен и защищённое соединение"
      :summary="certSummary"
      :chip="certChip"
      :tone="certTone"
    >
      <p class="lead">
        С сертификатом панель открывается по своему домену и защищённому
        соединению. Без него браузер считает панель незащищённой и не даёт включить
        уведомления.
      </p>

      <p v-if="certMismatch" class="note warn">
        По защищённому адресу роутер сейчас отдаёт сертификат
        «{{ certInfo?.serving_cn }}», а не {{ cert?.domain }} — браузер будет ругаться.
        Попробуйте выпустить сертификат заново.
      </p>

      <p v-if="cert?.last_result === 'error' && cert?.last_error" class="note bad">
        Прошлая попытка: {{ cert.last_error }}
      </p>

      <p v-if="status.isKeenetic" class="note faint">
        На этом роутере защищённый доступ обычно уже даёт встроенный сервис имени —
        свой сертификат нужен, только если панель открывается по собственному домену.
      </p>

      <div class="actions">
        <UiButton variant="primary" @click="sheetCert = true">
          {{ certOk ? "Выпустить заново" : "Выпустить сертификат" }}
        </UiButton>
        <UiButton v-if="status.isKeenetic" @click="sheetKeenHttps = true">
          Как обойтись без сертификата
        </UiButton>
        <UiButton :busy="busy === 'cert'" @click="refreshCert">Обновить сведения</UiButton>
      </div>
    </ServicePanel>

    <!-- ===== уведомления ===== -->
    <ServicePanel
      v-model:open="open.push"
      title="Уведомления в браузер"
      :summary="pushSummary"
      :chip="pushSub ? 'включены' : 'выключены'"
      :tone="pushSub ? 'ok' : undefined"
    >
      <p class="lead">
        Роутер сам сообщит, если активный VPN перестал работать, если он переключился
        на запасной или если вышла новая версия панели. Уведомления приходят, даже
        когда вкладка закрыта.
      </p>

      <p v-if="!secureContext || !swSupported" class="note warn">
        Уведомления работают только при защищённом доступе к панели. Сейчас панель
        открыта по обычному адресу, и браузер их не разрешит. Выпустите сертификат
        для своего домена и заходите по нему.
      </p>
      <p v-else-if="pushCfg?.available === false" class="note warn">
        Роутер не может отправлять уведомления: на нём нет нужного средства отправки.
      </p>
      <p v-else-if="pushPerm === 'denied'" class="note warn">
        Уведомления для этой панели запрещены в настройках браузера — снимите запрет
        в свойствах сайта и вернитесь сюда.
      </p>

      <p v-if="pushCfg?.bypass === 'active'" class="note faint">
        Канал доставки уведомлений пущен мимо VPN — иначе сообщение о неработающем
        VPN не дошло бы именно тогда, когда оно нужно.
      </p>
      <p v-else-if="pushCfg?.bypass === 'inactive'" class="note warn">
        Канал доставки уведомлений пока идёт через VPN. Если активный VPN упадёт,
        уведомление об этом может не дойти — тишину нельзя считать признаком того,
        что всё в порядке. Роутер уводит канал мимо VPN сам, когда уведомления
        включают; если предупреждение осталось, выключите и включите их здесь заново.
      </p>

      <div class="actions">
        <UiButton
          v-if="!pushSub"
          variant="primary"
          :disabled="!pushReady || pushPerm === 'denied'"
          :busy="busy === 'push'"
          @click="pushOn"
        >
          Включить уведомления
        </UiButton>
        <UiButton v-else :busy="busy === 'push'" @click="pushOff">
          Выключить на этом устройстве
        </UiButton>
        <UiButton
          :disabled="!pushReady || !(pushCfg?.sub_count ?? 0)"
          :busy="busy === 'push-test'"
          @click="pushTest"
        >
          Отправить пробное
        </UiButton>
        <!-- Инструкция нужна ровно в том случае, когда уведомления недоступны:
             панель открыта по обычному адресу. Поэтому кнопка не прячется, когда
             всё остальное задизейблено. -->
        <UiButton v-if="status.isKeenetic" @click="sheetKeenHttps = true">
          Как открыть панель по HTTPS
        </UiButton>
      </div>
    </ServicePanel>

    <!-- ===== аппаратное ускорение ===== -->
    <ServicePanel
      v-if="offloadSupported"
      v-model:open="open.offload"
      title="Аппаратное ускорение"
      :summary="offloadSummary"
      :chip="offload?.mode === 'off' ? 'без присмотра' : 'под присмотром'"
      :tone="offload?.mode === 'off' ? 'warn' : 'ok'"
    >
      <p class="lead">
        Роутер разгоняет трафик отдельным блоком, минуя процессор. Изредка этот блок
        залипает: скорость падает до сотни мегабит и не возвращается до перезагрузки.
        Роутер умеет замечать это сам и приводить всё в порядок без перезагрузки.
      </p>

      <SegmentedControl
        v-model="offloadMode"
        label="Что делать при залипании"
        :busy="busy === 'offload'"
        :options="[
          { value: 'auto', label: 'Чинить сам' },
          { value: 'notify', label: 'Только сообщать' },
          { value: 'off', label: 'Не следить' },
        ]"
      />

      <p v-if="offload?.last_action && offload.last_action !== 'none'" class="note faint">
        Последнее действие: {{ offload.last_action }}<template v-if="offload.last_result">
          — {{ offload.last_result }}</template>
      </p>

      <!-- Доля времени процессора на обработку пакетов — единственный признак, по
           которому залипание видно снаружи: при работающем ускорителе трафик идёт
           мимо процессора, и цифра остаётся низкой. -->
      <p v-if="typeof offload?.last_sirq === 'number'" class="note faint">
        Процессор на обработке трафика при последней проверке: {{ offload.last_sirq }} %
      </p>

      <div class="actions">
        <UiButton :busy="busy === 'offload-kick'" @click="kickOffload">
          Привести в порядок сейчас
        </UiButton>
      </div>
    </ServicePanel>

    <!-- ===== файл подкачки ===== -->
    <ServicePanel
      v-if="swapSupported"
      v-model:open="open.swap"
      title="Файл подкачки"
      :summary="swapSummary"
      :chip="swap?.active ? 'подключён' : 'нет'"
      :tone="swap?.active ? 'ok' : 'warn'"
    >
      <p class="lead">
        Оперативной памяти на этом роутере немного, и под нагрузкой система может
        снимать нужные программы. Файл подкачки на накопителе даёт запас и делает
        работу панели устойчивее.
      </p>

      <p v-if="swap?.tools === false" class="note warn">
        Не хватает системных утилит для подкачки — установите пакет swap-utils.
      </p>

      <p class="note faint">
        Свободно на накопителе: {{ fmtInt(swap?.free_mb ?? 0) }} МБ
      </p>

      <FormField v-if="!swap?.exists" label="Размер файла, МБ" hint="От 64 до 4096">
        <input
          v-model="swapSize"
          type="number"
          inputmode="numeric"
          min="64"
          max="4096"
          :disabled="busy === 'swap'"
        />
      </FormField>

      <p v-if="swapProgress" class="note live">{{ swapProgress }}</p>
      <p v-else-if="swap?.last_message" class="note faint">{{ swap.last_message }}</p>

      <div class="actions">
        <UiButton
          v-if="!swap?.exists"
          variant="primary"
          :busy="busy === 'swap'"
          :disabled="swap?.tools === false"
          @click="createSwap"
        >
          Создать
        </UiButton>
        <template v-else>
          <template v-if="confirmSwap">
            <UiButton variant="danger" :busy="busy === 'swap'" @click="removeSwap">
              Точно удалить
            </UiButton>
            <UiButton @click="confirmSwap = false">Отмена</UiButton>
          </template>
          <UiButton v-else variant="danger" @click="confirmSwap = true">
            Удалить файл
          </UiButton>
        </template>
      </div>
    </ServicePanel>

    <!-- ===== резервная копия ===== -->
    <ServicePanel
      v-model:open="open.backup"
      title="Резервная копия настроек"
      summary="Списки доменов, правила обхода и настройки панели одним файлом"
    >
      <p class="lead">
        В копию попадают настройки панели, списки доменов и параметры обхода. Пароль
        от панели и VPN-профили в неё не входят: профили выгружаются отдельно, а
        пароль восстановлением не меняется.
      </p>

      <input
        ref="fileInput"
        class="file"
        type="file"
        accept="application/json,.json"
        @change="pickFile"
      />

      <p v-if="importError" class="note bad">
        {{ importFile ? `Файл «${importFile.name}»: ` : "" }}{{ importError }}
      </p>

      <template v-else-if="importDoc">
        <p class="note warn">
          Из файла «{{ importFile?.name }}» будет перезаписано:
        </p>
        <ul class="sections">
          <li v-for="s in importSections" :key="s">
            {{ SECTION_LABELS[s] }} <span class="raw">{{ s }}</span>
          </li>
        </ul>
        <p class="note faint">
          Остальное останется как есть. После восстановления роутер перезапустит
          VPN — соединения на несколько секунд прервутся. Вернуть текущие списки
          обратно можно будет только из другой копии.
        </p>
      </template>

      <div class="actions">
        <UiButton variant="primary" :busy="busy === 'export'" @click="exportConfig">
          Скачать копию
        </UiButton>
        <template v-if="confirmImport">
          <UiButton variant="danger" :busy="busy === 'import'" @click="importConfig">
            Точно перезаписать
          </UiButton>
          <UiButton @click="confirmImport = false">Отмена</UiButton>
        </template>
        <UiButton v-else :disabled="!importDoc" @click="confirmImport = true">
          Восстановить из файла
        </UiButton>
      </div>
    </ServicePanel>

    <!-- ===== вход в панель ===== -->
    <ServicePanel
      v-model:open="open.account"
      title="Вход в панель"
      :summary="session.user ? `Вы вошли как ${session.user}` : 'Учётная запись панели'"
    >
      <p class="lead">
        Пароль от панели хранится на роутере в зашифрованном виде. Если сменить
        логин, все открытые сессии завершатся и войти придётся заново.
      </p>

      <div class="actions">
        <UiButton variant="primary" @click="sheetPassword = true">
          Сменить логин или пароль
        </UiButton>
      </div>
    </ServicePanel>
  </div>

  <PortmapSheet
    :open="sheetPortmap"
    :entry="editing"
    :used-ids="rows.map((r) => r.id)"
    :https-supported="httpsSupported"
    :https-reason="pm?.https_reason ?? ''"
    :dnat-supported="dnatSupported"
    :dnat-reason="pm?.dnat_reason ?? ''"
    :auth-supported="authSupported"
    :auth-reason="pm?.auth_reason ?? ''"
    :domain="pm?.domain ?? ''"
    :clients="clients"
    @close="sheetPortmap = false"
    @saved="onPortmapSaved"
  />

  <CertSheet
    :open="sheetCert"
    :domain="cert?.domain ?? certInfo?.domain ?? ''"
    :email="cert?.email ?? ''"
    :http80="certInfo?.http80 ?? ''"
    :wan-ip="certInfo?.wan_ip ?? ''"
    :acme-present="certInfo?.acme_present !== false"
    :keenetic="status.isKeenetic"
    :panel-port="status.data?.panel_port"
    @close="sheetCert = false"
    @done="loadCert"
  />

  <KeenHttpsSheet
    :open="sheetKeenHttps"
    :panel-port="status.data?.panel_port"
    @close="sheetKeenHttps = false"
  />

  <PasswordSheet :open="sheetPassword" @close="sheetPassword = false" />
</template>

<style scoped>
.areas {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  /* Ширину не режем: экран из таких же раскрывающихся областей, что и
     «Правила», и разная ширина колонки при переходе между ними бросается
     в глаза. Длинные тексты внутри областей ограничены сами. */
}
.lead {
  font-size: 13px;
  color: var(--dim);
}
.note {
  font-size: 12.5px;
  color: var(--dim);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
  overflow-wrap: anywhere;
}
.note.warn {
  color: var(--warn);
  border-color: color-mix(in srgb, var(--warn) 45%, transparent);
}
.note.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
.note.live {
  color: var(--accent);
  border-color: var(--accent);
}
.note.faint {
  color: var(--faint);
  border-color: transparent;
  padding: 0 2px;
}
.actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

/* ---- строка проброса ---- */
.row {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 11px 12px;
  display: flex;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
  background: var(--panel-2);
}
/* Название и маршрут живут внутри переключателя: вся строка — одна цель
   нажатия, а адрес читается моноширинным, как везде в панели. */
.row :deep(.lbl) {
  font-weight: 600;
}
.row :deep(.text small) {
  font-family: var(--mono);
  overflow-wrap: anywhere;
}
.row-note {
  font-size: 12px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.row-check {
  font-size: 12.5px;
  color: var(--accent);
  overflow-wrap: anywhere;
}
/* Адрес сервиса — ссылка, но живёт внутри строки с переключателем: свой
   отдельный ряд, чтобы нажатие на неё не читалось как нажатие на строку. */
.row-link {
  font-size: 12px;
  font-family: var(--mono);
  color: var(--accent);
  align-self: flex-start;
  overflow-wrap: anywhere;
}
.row-actions {
  display: flex;
  gap: 7px;
  flex-wrap: wrap;
}

/* ---- разделы в выбранной копии ---- */
.sections {
  margin: 0;
  padding-left: 18px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12.5px;
  color: var(--dim);
  list-style: disc;
}
/* Имя раздела из файла — на случай, когда человеку нужно сверить его с копией,
   а не с нашим переводом. */
.sections .raw {
  font-family: var(--mono);
  font-size: 11.5px;
  color: var(--faint);
}

/* Поле выбора файла умеет растягивать родителя по имени файла — не даём. */
.file {
  font-size: 13px;
  color: var(--dim);
  width: 100%;
  max-width: 100%;
}
.file::file-selector-button {
  border: 1px solid var(--line-2);
  background: transparent;
  color: var(--ink);
  border-radius: var(--radius-sm);
  padding: 8px 12px;
  margin-right: 10px;
  min-height: 44px;
  font: inherit;
  font-size: 13px;
}

@media (max-width: 860px) {
  /* На телефоне в кнопку нужно попадать пальцем, а не курсором. */
  .actions :deep(.btn),
  .row-actions :deep(.btn) {
    min-height: 44px;
  }
  .row :deep(.text) {
    min-height: 44px;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
  /* Три варианта режима не влезают в строку на узком экране: пусть текст
     переносится внутри кнопок, а не обрезается вместе с последней. */
  :deep(.seg) {
    width: 100%;
  }
  :deep(.seg button) {
    flex: 1 1 auto;
    min-width: 0;
    white-space: normal;
  }
}
</style>
