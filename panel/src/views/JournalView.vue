<script setup lang="ts">
/* Раздел «Журнал» — всё, что нужно, когда что-то пошло не так: логи служб,
   конфигурация sing-box, ручное управление сервисами, правила файрвола,
   обновления и функциональная проверка. Области раскрываются по одной;
   тяжёлые вещи (редакторы, дампы, живой лог установки) — в шторках. */
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import JournalArea from "@/components/journal/JournalArea.vue";
import LogPane from "@/components/journal/LogPane.vue";
import CodeArea from "@/components/journal/CodeArea.vue";
import UpdateRow from "@/components/journal/UpdateRow.vue";
import { ServerRestartingError, diag, overview, poll, requestJson } from "@/api";
import type {
  ApplyLogResponse,
  HealthStatusResponse,
  LogName,
  UpdateChannelState,
  UpdatesOverview,
} from "@/api";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { useCommandStore } from "@/stores/commands";
import { fmtAgo, fmtInt, isSet } from "@/lib/format";

const status = useStatusStore();
const toast = useToastStore();
const commands = useCommandStore();

type AreaKey = "logs" | "config" | "services" | "firewall" | "updates" | "health";

const areas = reactive<Record<AreaKey, boolean>>({
  logs: true,
  config: false,
  services: false,
  firewall: false,
  updates: false,
  health: false,
});

/** Раскрытую из палитры область нужно ещё и показать — иначе «ничего не произошло». */
function openArea(key: AreaKey) {
  areas[key] = true;
  requestAnimationFrame(() => {
    document
      .getElementById(`area-${key}`)
      ?.scrollIntoView({ behavior: "smooth", block: "start" });
  });
}

function ask(text: string): boolean {
  return window.confirm(text);
}

/** После самообновления страницу надо перезагрузить: файлы панели уже другие. */
function reloadPage() {
  location.reload();
}

/* ==================== журналы ==================== */

const LOG_SOURCES: { value: LogName; label: string }[] = [
  { value: "singbox", label: "sing-box" },
  { value: "zapret", label: "zapret" },
  { value: "health", label: "проверка" },
  { value: "update", label: "обновления" },
  { value: "apply", label: "установка" },
];
const LOG_TITLE: Record<LogName, string> = {
  singbox: "sing-box",
  zapret: "zapret",
  health: "проверка профилей",
  update: "проверка обновлений",
  apply: "установка обновлений",
};

const logName = ref<LogName>("singbox");
const logText = ref("");
const logPath = ref("");
const logMissing = ref(false);
const logBusy = ref(false);
const logAuto = ref(false);
const logAt = ref(0);
let logTimer: number | undefined;

const logLines = computed(() =>
  logText.value.trim() ? logText.value.trim().split("\n").length : 0,
);

const logSummary = computed(() => {
  const what = LOG_TITLE[logName.value];
  if (logMissing.value) return `${what}: журнала ещё нет`;
  if (!logLines.value) return `${what}: пусто`;
  return `${what}: последние ${fmtInt(logLines.value)} строк`;
});

async function loadLog(quiet = false) {
  if (!quiet) logBusy.value = true;
  try {
    const r = await diag.logsView(logName.value);
    logText.value = r.log ?? "";
    logPath.value = r.path ?? "";
    logMissing.value = r.missing === true;
    logAt.value = Date.now() / 1000;
  } catch (e) {
    if (!quiet) toast.fromError(e, "Не удалось прочитать журнал");
  } finally {
    logBusy.value = false;
  }
}

async function clearLog() {
  const what = LOG_TITLE[logName.value];
  if (!ask(`Очистить журнал «${what}»? Записи не восстановить.`)) return;
  try {
    await diag.logsClear(logName.value);
    toast.ok(`Журнал «${what}» очищен`);
    await loadLog(true);
  } catch (e) {
    toast.fromError(e, "Не удалось очистить журнал");
  }
}

function stopLogTimer() {
  if (logTimer) window.clearInterval(logTimer);
  logTimer = undefined;
}

watch(logName, () => void loadLog());

watch(logAuto, (on) => {
  stopLogTimer();
  if (!on) return;
  logTimer = window.setInterval(() => {
    /* На фоновой вкладке и с закрытой областью дёргать роутер незачем. */
    if (document.visibilityState === "visible" && areas.logs) void loadLog(true);
  }, 5000);
});

/* --- мост в системный журнал --- */

const bridge = reactive({ enabled: false, singbox: false, busy: "", known: false });

async function loadBridge() {
  try {
    /* GET у log_config в контракте diag не описан, а обёртка logConfig() —
       всегда POST, который переписывает настройки и перезапускает мост.
       Читать состояние записью неправильно, поэтому здесь прямой GET. */
    const r = await requestJson<{ enabled?: boolean; singbox?: boolean }>("log_config");
    bridge.enabled = r.enabled === true;
    bridge.singbox = r.singbox === true;
    bridge.known = true;
  } catch {
    /* Старая прошивка без моста — просто не показываем переключатели. */
    bridge.known = false;
  }
}

async function setBridge(patch: { enabled?: boolean; singbox?: boolean }) {
  const prev = { enabled: bridge.enabled, singbox: bridge.singbox };
  bridge.busy = patch.enabled === undefined ? "singbox" : "enabled";
  Object.assign(bridge, patch);
  try {
    await diag.logConfig({ enabled: bridge.enabled, singbox: bridge.singbox });
    toast.ok(
      bridge.enabled
        ? "Записи уходят в системный журнал роутера"
        : "Запись в системный журнал выключена",
    );
  } catch (e) {
    Object.assign(bridge, prev);
    toast.fromError(e, "Не удалось изменить запись в системный журнал");
  } finally {
    bridge.busy = "";
  }
}

const bridgeOn = computed({
  get: () => bridge.enabled,
  set: (v: boolean) => void setBridge({ enabled: v }),
});
const bridgeSingbox = computed({
  get: () => bridge.singbox,
  set: (v: boolean) => void setBridge({ singbox: v }),
});

/* ==================== конфигурация sing-box ==================== */

const cfgOpen = ref(false);
const cfgText = ref("");
const cfgError = ref("");
const cfgLoading = ref(false);
const cfgSaving = ref(false);
const cfgLoaded = ref(false);

async function loadConfig() {
  cfgLoading.value = true;
  cfgError.value = "";
  try {
    const j = await diag.singboxConfigGet();
    cfgText.value = JSON.stringify(j, null, 2);
    cfgLoaded.value = true;
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать конфигурацию");
  } finally {
    cfgLoading.value = false;
  }
}

async function openConfig() {
  cfgOpen.value = true;
  if (!cfgLoaded.value) await loadConfig();
}

async function saveConfig() {
  cfgError.value = "";
  try {
    JSON.parse(cfgText.value);
  } catch (e) {
    cfgError.value = `Это не разбирается как JSON: ${e instanceof Error ? e.message : String(e)}`;
    toast.error("В тексте ошибка — sing-box такое не примет");
    return;
  }
  cfgSaving.value = true;
  try {
    await diag.singboxConfigSave(cfgText.value);
    toast.ok("Конфигурация сохранена — sing-box её принял");
    cfgOpen.value = false;
    void status.refresh(true);
  } catch (e) {
    /* Бэкенд сам прогоняет `sing-box check` и возвращает его вывод целиком —
       глотать его нельзя, там единственное объяснение отказа. */
    cfgError.value = e instanceof Error ? e.message : String(e);
    toast.error("sing-box отверг конфигурацию");
  } finally {
    cfgSaving.value = false;
  }
}

/* ==================== сервисы ==================== */

type Svc = "singbox" | "zapret";
type Op = "start" | "stop" | "restart" | "enable" | "disable";

const SVC_TITLE: Record<Svc, string> = { singbox: "sing-box", zapret: "zapret" };
const OP_DONE: Record<Op, string> = {
  start: "запущен",
  stop: "остановлен",
  restart: "перезапущен",
  enable: "будет стартовать сам",
  disable: "сам стартовать не будет",
};
const OPS: Record<Svc, Record<Op, () => Promise<unknown>>> = {
  singbox: {
    start: () => diag.singboxStart(),
    stop: () => diag.singboxStop(),
    restart: () => diag.singboxRestart(),
    enable: () => diag.singboxEnable(),
    disable: () => diag.singboxDisable(),
  },
  zapret: {
    start: () => diag.zapretStart(),
    stop: () => diag.zapretStop(),
    restart: () => diag.zapretRestart(),
    enable: () => diag.zapretEnable(),
    disable: () => diag.zapretDisable(),
  },
};
const STOP_WARN: Record<Svc, string> = {
  singbox:
    "Остановить sing-box? Туннель выключится, и трафик из списка пойдёт напрямую.",
  zapret: "Остановить zapret? Обход DPI перестанет работать.",
};

const svcBusy = ref("");
const sb = computed(() => status.data?.singbox);
const zp = computed(() => status.data?.zapret);

async function runOp(svc: Svc, op: Op) {
  if (op === "stop" && !ask(STOP_WARN[svc])) return;
  svcBusy.value = `${svc}:${op}`;
  try {
    await OPS[svc][op]();
    toast.ok(`${SVC_TITLE[svc]} ${OP_DONE[op]}`);
  } catch (e) {
    toast.fromError(e, `Команда не выполнилась (${SVC_TITLE[svc]})`);
  } finally {
    svcBusy.value = "";
    void status.refresh(true);
  }
}

const servicesSummary = computed(() => {
  const a = sb.value?.running ? "sing-box работает" : "sing-box остановлен";
  const b = zp.value?.running ? "zapret работает" : "zapret остановлен";
  return `${a} · ${b}`;
});

/* --- аргументы zapret --- */

const argsOpen = ref(false);
const argsText = ref("");
const argsLoaded = ref(false);
const argsBusy = ref(false);

async function openArgs() {
  argsOpen.value = true;
  if (argsLoaded.value) return;
  argsBusy.value = true;
  try {
    const r = await diag.zapretConfigGet();
    argsText.value = r.args ?? "";
    argsLoaded.value = true;
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать аргументы zapret");
  } finally {
    argsBusy.value = false;
  }
}

async function saveArgs() {
  argsBusy.value = true;
  try {
    await diag.zapretConfigSave(argsText.value.trim());
    toast.ok("Аргументы zapret сохранены");
    argsOpen.value = false;
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить аргументы");
  } finally {
    argsBusy.value = false;
  }
}

/* ==================== файрвол ==================== */

const fwOpen = ref(false);
const fwBusy = ref(false);
const fwTab = ref<"nat" | "nft">("nat");
const fw = ref<{ nat: string; nft: string } | null>(null);

const fwText = computed(() =>
  fwTab.value === "nat" ? (fw.value?.nat ?? "") : (fw.value?.nft ?? ""),
);

async function loadFw() {
  fwBusy.value = true;
  try {
    fw.value = await overview.firewall();
  } catch (e) {
    toast.fromError(e, "Не удалось получить правила файрвола");
  } finally {
    fwBusy.value = false;
  }
}

async function openFw() {
  fwOpen.value = true;
  if (!fw.value) await loadFw();
}

/* ==================== обновления ==================== */

type Channel = "panel" | "singbox" | "tpws" | "nfqws2";

const CH_TITLE: Record<Channel, string> = {
  panel: "Панель",
  singbox: "sing-box",
  tpws: "tpws (обход DPI)",
  nfqws2: "nfqws2 (zapret2)",
};
const CH_CHECK: Record<Channel, () => Promise<UpdateChannelState>> = {
  panel: () => diag.panelUpdateCheck(),
  singbox: () => diag.binsCheck(),
  tpws: () => diag.tpwsCheck(),
  nfqws2: () => diag.nfqws2Check(),
};
const CH_APPLY: Record<Channel, () => Promise<unknown>> = {
  panel: () => diag.panelUpdateApply(),
  singbox: () => diag.binsApply(),
  tpws: () => diag.tpwsApply(),
  nfqws2: () => diag.nfqws2Apply(),
};

const upd = ref<UpdatesOverview | null>(null);
const autocheck = ref(false);
const autocheckBusy = ref(false);
const updBusy = ref("");

const applyOpen = ref(false);
const applyText = ref("");
const applyNote = ref("");
const applyRunning = ref(false);

const clOpen = ref(false);
const clTitle = ref("");
const clText = ref("");

const localOpen = ref(false);
const sigText = ref("");
const ipkFile = ref<File | null>(null);

const updatesSummary = computed(() => {
  const o = upd.value;
  if (!o) return "проверка панели и бинарников";
  const hot = (Object.keys(CH_TITLE) as Channel[]).filter(
    (c) => o[c]?.update_available === true,
  );
  if (hot.length) return `есть обновления: ${hot.map((c) => CH_TITLE[c]).join(", ")}`;
  return `панель ${o.panel?.current_version || status.data?.version || "—"} · всё свежее`;
});

const nfqws2Visible = computed(
  () => !status.isKeenetic && status.data?.binaries?.nfqws2_supported !== false,
);

/** changelog приходит в base64 (UTF-8) — иначе кириллица не переживёт shell. */
function b64ToText(b64?: string): string {
  if (!b64) return "";
  try {
    const bin = atob(b64.replace(/\s+/g, ""));
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return "";
  }
}

function showChangelog(ch: Channel) {
  clTitle.value = `Что нового: ${CH_TITLE[ch]}`;
  clText.value =
    b64ToText(upd.value?.[ch]?.changelog_b64) || "Описание изменений не пришло.";
  clOpen.value = true;
}

async function loadUpdates() {
  const [o, a] = await Promise.allSettled([
    diag.updatesOverview(),
    diag.autocheckStatus(),
  ]);
  if (o.status === "fulfilled" && o.value) upd.value = o.value;
  if (a.status === "fulfilled") autocheck.value = a.value?.enabled === true;
}

async function checkChannel(ch: Channel) {
  updBusy.value = `check:${ch}`;
  try {
    const r = await CH_CHECK[ch]();
    const next: UpdatesOverview = { ...(upd.value ?? {}) };
    next[ch] = r;
    upd.value = next;
    if (r.update_available) {
      toast.ok(`${CH_TITLE[ch]}: доступна версия ${r.available_version || "новее текущей"}`);
    } else {
      toast.info(`${CH_TITLE[ch]}: обновлений нет`);
    }
  } catch (e) {
    toast.fromError(e, "Проверка не удалась");
  } finally {
    updBusy.value = "";
  }
}

/** Живой хвост отсоединённой установки: единственный способ узнать её исход. */
async function followApply(title: string, panelish: boolean) {
  try {
    const r = await poll<ApplyLogResponse | null>(() => diag.applyLog(), {
      done: (v) => v?.done === true,
      intervalMs: 2000,
      timeoutMs: 420_000,
      onTick: (v) => {
        if (v && typeof v.log === "string") applyText.value = v.log;
      },
    });
    const raw = String(r?.rc ?? "").trim();
    const rc = raw === "" ? null : Number(raw);
    if (r?.done !== true) {
      applyNote.value =
        "Установка ещё идёт. Журнал обновится, как только роутер снова ответит.";
    } else if (rc === 0) {
      applyNote.value = panelish
        ? "Готово. Обновите страницу, чтобы открылась новая версия панели."
        : "Готово.";
      toast.ok(`${title}: обновление установлено`);
    } else if (rc === null) {
      applyNote.value = "Установка завершилась, код возврата не пришёл — смотрите журнал.";
    } else {
      applyNote.value = `Установка завершилась с кодом ${rc}. Что именно не получилось — видно в журнале.`;
      toast.error(`${title}: установка не удалась`);
    }
  } catch (e) {
    applyNote.value = e instanceof Error ? e.message : "Не удалось дождаться конца установки";
  } finally {
    applyRunning.value = false;
    updBusy.value = "";
    void loadUpdates();
    void status.refresh(true);
  }
}

async function applyChannel(ch: Channel) {
  const title = CH_TITLE[ch];
  if (
    !ask(
      `Установить обновление «${title}»? Во время установки связь с панелью может ненадолго прерваться.`,
    )
  ) {
    return;
  }
  updBusy.value = `apply:${ch}`;
  applyText.value = "";
  applyNote.value = "";
  applyRunning.value = true;
  applyOpen.value = true;
  try {
    await CH_APPLY[ch]();
  } catch (e) {
    /* Пустой или оборванный ответ на самообновлении — ожидаемый исход:
       веб-сервер, который держит этот же CGI, перезапускается. */
    if (e instanceof ServerRestartingError) {
      applyNote.value = "Панель перезапускается — следим за журналом установки.";
    } else {
      applyRunning.value = false;
      updBusy.value = "";
      applyNote.value = e instanceof Error ? e.message : "Не удалось запустить установку";
      toast.fromError(e, "Не удалось запустить установку");
      return;
    }
  }
  await followApply(title, ch === "panel");
}

function pickIpk(e: Event) {
  const input = e.target as HTMLInputElement;
  ipkFile.value = input.files?.[0] ?? null;
}

async function installLocal() {
  const f = ipkFile.value;
  if (!f) {
    toast.error("Сначала выберите файл .ipk");
    return;
  }
  if (!ask(`Установить ${f.name}? Панель перезапустится.`)) return;
  updBusy.value = "apply:local";
  applyText.value = "";
  applyNote.value = "";
  applyRunning.value = true;
  try {
    /* Подпись кладётся отдельным шагом: установщик ищет её рядом с .ipk.
       Без подписи ставится обычным opkg — это осознанный выбор оператора. */
    if (sigText.value.trim()) await diag.panelUpdateSig(sigText.value.trim());
    await diag.panelUpdateLocal(f);
    localOpen.value = false;
    applyOpen.value = true;
  } catch (e) {
    if (e instanceof ServerRestartingError) {
      localOpen.value = false;
      applyOpen.value = true;
      applyNote.value = "Панель перезапускается — следим за журналом установки.";
    } else {
      applyRunning.value = false;
      updBusy.value = "";
      toast.fromError(e, "Не удалось загрузить файл");
      return;
    }
  }
  await followApply("Панель", true);
}

async function setAutocheck(on: boolean) {
  autocheckBusy.value = true;
  const prev = autocheck.value;
  autocheck.value = on;
  try {
    await diag.autocheckSet(on);
    toast.ok(on ? "Панель будет проверять обновления сама" : "Автопроверка выключена");
  } catch (e) {
    autocheck.value = prev;
    toast.fromError(e, "Не удалось изменить автопроверку");
  } finally {
    autocheckBusy.value = false;
  }
}

const autocheckOn = computed({
  get: () => autocheck.value,
  set: (v: boolean) => void setAutocheck(v),
});

/* ==================== проверка и пинги ==================== */

const health = ref<HealthStatusResponse | null>(null);
const healthBusy = ref("");
const urlsOpen = ref(false);
const urlsText = ref("");
const urlsLoaded = ref(false);
const urlsBusy = ref(false);

const ka = ref<Record<string, unknown> | null>(null);
const kaBusy = ref(false);

const healthStats = computed(() => {
  const res = health.value?.results ?? {};
  const rows = Object.values(res);
  const ok = rows.filter((r) => r.ok === true).length;
  const last = rows.reduce((m, r) => Math.max(m, r.ts ?? 0), 0);
  return { total: rows.length, ok, last };
});

const healthSummary = computed(() => {
  const h = health.value;
  if (!h) return "функциональная проверка профилей";
  if (h.supported === false) return "проверка на этой платформе недоступна";
  const s = healthStats.value;
  const on = h.enabled ? "включена" : "выключена";
  if (!s.total) return `${on} · результатов ещё нет`;
  return `${on} · ${s.ok} из ${s.total} проходят проверку`;
});

async function loadHealth() {
  try {
    health.value = await diag.healthStatus();
  } catch {
    /* Молча: раздел живёт и без сводки проверки. */
  }
}

async function setHealth(patch: {
  enabled?: boolean;
  auto_switch?: boolean;
  speed?: boolean;
}) {
  healthBusy.value = Object.keys(patch)[0] ?? "";
  try {
    await diag.healthConfig(patch);
    toast.ok("Настройки проверки сохранены");
    await loadHealth();
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить настройки проверки");
  } finally {
    healthBusy.value = "";
  }
}

const healthEnabled = computed({
  get: () => health.value?.enabled === true,
  set: (v: boolean) => void setHealth({ enabled: v }),
});
const healthAuto = computed({
  get: () => health.value?.auto_switch === true,
  set: (v: boolean) => void setHealth({ auto_switch: v }),
});
const healthSpeed = computed({
  get: () => health.value?.speed === true,
  set: (v: boolean) => void setHealth({ speed: v }),
});

async function checkAll() {
  healthBusy.value = "all";
  try {
    await diag.healthCheckAll();
    toast.info("Проверка запущена — результаты появляются по мере готовности");
  } catch (e) {
    toast.fromError(e, "Не удалось запустить проверку");
  } finally {
    healthBusy.value = "";
  }
}

async function openUrls() {
  urlsOpen.value = true;
  if (urlsLoaded.value) return;
  urlsBusy.value = true;
  try {
    const r = await diag.healthUrlsGet();
    urlsText.value = r.list ?? "";
    urlsLoaded.value = true;
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать список целей");
  } finally {
    urlsBusy.value = false;
  }
}

async function saveUrls() {
  urlsBusy.value = true;
  try {
    await diag.healthUrlsSet(urlsText.value);
    toast.ok("Список целей сохранён");
    urlsOpen.value = false;
    await loadHealth();
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить список целей");
  } finally {
    urlsBusy.value = false;
  }
}

const kaText = computed(() => {
  const k = ka.value;
  if (!k) return "";
  const ok = k.ok;
  const state = ok === true ? "соединение живое" : ok === false ? "соединение не отвечает" : "ещё не проверялось";
  const server = String(k.server ?? "");
  const port = String(k.port ?? "");
  const profile = String(k.profile ?? "");
  const when = fmtAgo(Number(k.last_checked ?? 0) || undefined);
  const parts = [state];
  if (profile) parts.push(profile);
  if (server) parts.push(isSet(port) ? `${server}:${port}` : server);
  if (when) parts.push(when);
  return parts.join(" · ");
});

async function loadKeepalive() {
  try {
    ka.value = await diag.keepaliveStatus();
  } catch {
    /* Хелпера может не быть — это не повод шуметь. */
  }
}

async function checkKeepalive() {
  kaBusy.value = true;
  try {
    ka.value = await diag.keepaliveCheck();
    toast.ok("Соединение проверено");
  } catch (e) {
    toast.fromError(e, "Проверка соединения не удалась");
  } finally {
    kaBusy.value = false;
  }
}

/* ==================== жизненный цикл ==================== */

let unregister: (() => void) | undefined;

onMounted(async () => {
  unregister = commands.register([
    {
      id: "jr:log-reload",
      title: "Журнал: перечитать",
      group: "журнал",
      keywords: "лог обновить tail",
      run: () => {
        openArea("logs");
        void loadLog();
      },
    },
    {
      id: "jr:log-clear",
      title: "Журнал: очистить текущий",
      group: "журнал",
      keywords: "лог стереть",
      run: () => void clearLog(),
    },
    {
      id: "jr:config",
      title: "Открыть конфигурацию sing-box",
      group: "журнал",
      keywords: "config json редактор",
      run: () => void openConfig(),
    },
    {
      id: "jr:sb-restart",
      title: "Перезапустить sing-box",
      group: "журнал",
      keywords: "сервис служба",
      run: () => void runOp("singbox", "restart"),
    },
    {
      id: "jr:zp-restart",
      title: "Перезапустить zapret",
      group: "журнал",
      keywords: "сервис служба dpi",
      run: () => void runOp("zapret", "restart"),
    },
    {
      id: "jr:firewall",
      title: "Показать правила файрвола",
      group: "журнал",
      keywords: "nat nft iptables",
      run: () => void openFw(),
    },
    {
      id: "jr:upd-check",
      title: "Проверить обновление панели",
      group: "журнал",
      keywords: "версия релиз",
      run: () => {
        openArea("updates");
        void checkChannel("panel");
      },
    },
    {
      id: "jr:upd-apply",
      title: "Установить обновление панели",
      group: "журнал",
      keywords: "версия релиз ipk",
      run: () => void applyChannel("panel"),
    },
    {
      id: "jr:upd-local",
      title: "Установить панель из файла .ipk",
      group: "журнал",
      keywords: "загрузить пакет подпись",
      run: () => {
        localOpen.value = true;
      },
    },
    {
      id: "jr:health-all",
      title: "Проверить все профили",
      group: "журнал",
      keywords: "здоровье health",
      run: () => {
        openArea("health");
        void checkAll();
      },
    },
    {
      id: "jr:keepalive",
      title: "Проверить соединение с сервером",
      group: "журнал",
      keywords: "keepalive пинг",
      run: () => void checkKeepalive(),
    },
  ]);

  await loadLog();
  void loadBridge();
  void loadUpdates();
  void loadHealth();
  void loadKeepalive();
});

onBeforeUnmount(() => {
  stopLogTimer();
  unregister?.();
});
</script>

<template>
  <div class="journal">
    <!-- ==================== журналы ==================== -->
    <JournalArea
      id="area-logs"
      v-model:open="areas.logs"
      title="Журналы"
      :summary="logSummary"
    >
      <div class="scroll-x">
        <SegmentedControl
          v-model="logName"
          label="Источник журнала"
          :options="LOG_SOURCES"
        />
      </div>

      <p v-if="logPath" class="path mono">{{ logPath }}</p>

      <LogPane
        :text="logText"
        :missing="logMissing"
        :follow="logAuto"
        height="46vh"
        empty-text="Журнал пуст — служба пока ничего не записала."
      />

      <div class="acts">
        <UiButton :busy="logBusy" @click="loadLog()">Обновить</UiButton>
        <UiButton variant="danger" @click="clearLog">Очистить</UiButton>
        <span v-if="logAt" class="hint">перечитано {{ fmtAgo(logAt) }}</span>
      </div>

      <SwitchToggle
        v-model="logAuto"
        label="Обновлять автоматически"
        hint="Перечитывать журнал каждые пять секунд, пока область открыта"
      />

      <template v-if="bridge.known">
        <hr class="sep" />
        <SwitchToggle
          v-model="bridgeOn"
          label="Дублировать в системный журнал"
          :busy="bridge.busy === 'enabled'"
          hint="Записи панели уходят в syslog роутера — их видно снаружи"
        />
        <SwitchToggle
          v-model="bridgeSingbox"
          label="И журнал sing-box тоже"
          :disabled="!bridge.enabled"
          :busy="bridge.busy === 'singbox'"
          hint="Работает только вместе с предыдущим переключателем"
        />
      </template>
    </JournalArea>

    <!-- ==================== конфигурация sing-box ==================== -->
    <JournalArea
      id="area-config"
      v-model:open="areas.config"
      title="Конфигурация sing-box"
      summary="весь config.json целиком; перед сохранением роутер сам проверит его"
    >
      <p class="hint">
        Панель обычно собирает конфигурацию сама из профилей и правил. Ручная правка
        живёт до ближайшей пересборки — пользуйтесь ей для разбора, а не как
        постоянной настройкой.
      </p>
      <div class="acts">
        <UiButton variant="primary" :busy="cfgLoading" @click="openConfig">
          Открыть редактор
        </UiButton>
      </div>
    </JournalArea>

    <!-- ==================== сервисы ==================== -->
    <JournalArea
      id="area-services"
      v-model:open="areas.services"
      title="Управление сервисами"
      :summary="servicesSummary"
    >
      <div class="svc">
        <p class="svc-name">
          sing-box
          <small :class="sb?.running ? 'ok' : 'bad'">
            {{ sb?.running ? "работает" : "остановлен" }}
            <template v-if="isSet(sb?.pid)"> · PID {{ sb?.pid }}</template>
            <template v-if="sb?.enabled !== undefined">
              · автозапуск {{ sb?.enabled ? "включён" : "выключен" }}
            </template>
          </small>
        </p>
        <div class="acts">
          <UiButton :busy="svcBusy === 'singbox:start'" @click="runOp('singbox', 'start')">
            Запустить
          </UiButton>
          <UiButton
            variant="danger"
            :busy="svcBusy === 'singbox:stop'"
            @click="runOp('singbox', 'stop')"
          >
            Остановить
          </UiButton>
          <UiButton
            :busy="svcBusy === 'singbox:restart'"
            @click="runOp('singbox', 'restart')"
          >
            Перезапустить
          </UiButton>
          <UiButton
            v-if="sb?.enabled"
            :busy="svcBusy === 'singbox:disable'"
            @click="runOp('singbox', 'disable')"
          >
            Убрать из автозапуска
          </UiButton>
          <UiButton
            v-else
            :busy="svcBusy === 'singbox:enable'"
            @click="runOp('singbox', 'enable')"
          >
            Добавить в автозапуск
          </UiButton>
        </div>
      </div>

      <div class="svc">
        <p class="svc-name">
          zapret (tpws)
          <small :class="zp?.running ? 'ok' : 'bad'">
            {{ zp?.running ? "работает" : "остановлен" }}
            <template v-if="isSet(zp?.port)"> · порт {{ zp?.port }}</template>
            <template v-if="zp?.enabled !== undefined">
              · автозапуск {{ zp?.enabled ? "включён" : "выключен" }}
            </template>
          </small>
        </p>
        <div class="acts">
          <UiButton :busy="svcBusy === 'zapret:start'" @click="runOp('zapret', 'start')">
            Запустить
          </UiButton>
          <UiButton
            variant="danger"
            :busy="svcBusy === 'zapret:stop'"
            @click="runOp('zapret', 'stop')"
          >
            Остановить
          </UiButton>
          <UiButton
            :busy="svcBusy === 'zapret:restart'"
            @click="runOp('zapret', 'restart')"
          >
            Перезапустить
          </UiButton>
          <UiButton
            v-if="zp?.enabled"
            :busy="svcBusy === 'zapret:disable'"
            @click="runOp('zapret', 'disable')"
          >
            Убрать из автозапуска
          </UiButton>
          <UiButton
            v-else
            :busy="svcBusy === 'zapret:enable'"
            @click="runOp('zapret', 'enable')"
          >
            Добавить в автозапуск
          </UiButton>
          <UiButton @click="openArgs">Аргументы</UiButton>
        </div>
      </div>
    </JournalArea>

    <!-- ==================== файрвол ==================== -->
    <JournalArea
      id="area-firewall"
      v-model:open="areas.firewall"
      title="Файрвол"
      summary="дампы nat и nft — куда на самом деле уходит трафик"
    >
      <p class="hint">
        Здесь видно правила перенаправления: попадает ли адрес в ipset и уходит ли он
        в туннель или в обход DPI.
      </p>
      <div class="acts">
        <UiButton variant="primary" :busy="fwBusy" @click="openFw">
          Показать правила
        </UiButton>
        <UiButton v-if="fw" :busy="fwBusy" @click="loadFw">Перечитать</UiButton>
      </div>
    </JournalArea>

    <!-- ==================== обновления ==================== -->
    <JournalArea
      id="area-updates"
      v-model:open="areas.updates"
      title="Обновления"
      :summary="updatesSummary"
    >
      <UpdateRow
        title="Панель"
        :state="upd?.panel ?? null"
        :installed="status.data?.version"
        :busy-check="updBusy === 'check:panel'"
        :busy-apply="updBusy === 'apply:panel'"
        @check="checkChannel('panel')"
        @apply="applyChannel('panel')"
        @changelog="showChangelog('panel')"
      />
      <UpdateRow
        title="sing-box"
        :state="upd?.singbox ?? null"
        :installed="status.data?.binaries?.singbox_version"
        :busy-check="updBusy === 'check:singbox'"
        :busy-apply="updBusy === 'apply:singbox'"
        note="Пакет берётся из нашего opkg-фида."
        @check="checkChannel('singbox')"
        @apply="applyChannel('singbox')"
        @changelog="showChangelog('singbox')"
      />
      <UpdateRow
        title="tpws (обход DPI)"
        :state="upd?.tpws ?? null"
        :installed="status.data?.binaries?.tpws_version"
        :busy-check="updBusy === 'check:tpws'"
        :busy-apply="updBusy === 'apply:tpws'"
        @check="checkChannel('tpws')"
        @apply="applyChannel('tpws')"
        @changelog="showChangelog('tpws')"
      />
      <UpdateRow
        v-if="nfqws2Visible"
        title="nfqws2 (zapret2)"
        :state="upd?.nfqws2 ?? null"
        :installed="status.data?.binaries?.nfqws2_version"
        :busy-check="updBusy === 'check:nfqws2'"
        :busy-apply="updBusy === 'apply:nfqws2'"
        @check="checkChannel('nfqws2')"
        @apply="applyChannel('nfqws2')"
        @changelog="showChangelog('nfqws2')"
      />

      <SwitchToggle
        v-model="autocheckOn"
        label="Проверять обновления сама"
        :busy="autocheckBusy"
        hint="Раз в шесть часов роутер сам спрашивает, не вышла ли новая версия"
      />

      <div class="acts">
        <UiButton @click="loadUpdates">Перечитать сводку</UiButton>
        <UiButton @click="localOpen = true">Установить из файла</UiButton>
        <UiButton v-if="applyText || applyNote" @click="applyOpen = true">
          Журнал установки
        </UiButton>
      </div>
    </JournalArea>

    <!-- ==================== проверка ==================== -->
    <JournalArea
      id="area-health"
      v-model:open="areas.health"
      title="Проверка и пинги"
      :summary="healthSummary"
    >
      <p v-if="health?.supported === false" class="hint">
        Функциональная проверка на этой платформе недоступна — помощник не установлен.
      </p>

      <SwitchToggle
        v-model="healthEnabled"
        label="Проверять профили по расписанию"
        :busy="healthBusy === 'enabled'"
        :disabled="health?.supported === false"
        hint="Роутер сам ходит по целям через каждый профиль и отмечает живые"
      />
      <SwitchToggle
        v-model="healthAuto"
        label="Переключаться на живой профиль"
        :busy="healthBusy === 'auto_switch'"
        :disabled="health?.supported === false"
        hint="Если активный профиль перестал отвечать — панель включит рабочий"
      />
      <SwitchToggle
        v-model="healthSpeed"
        label="Мерить скорость"
        :busy="healthBusy === 'speed'"
        :disabled="health?.supported === false"
        hint="Кроме доступности качается пробный файл — проверка идёт дольше"
      />

      <p class="hint">
        Целей проверки: {{ health?.urls?.length ?? 0 }}<template
          v-if="healthStats.total"
        >
          · результатов {{ healthStats.total }}<template v-if="healthStats.last">
            · последний {{ fmtAgo(healthStats.last) }} </template
          ></template
        >
      </p>

      <div class="acts">
        <UiButton variant="primary" :busy="healthBusy === 'all'" @click="checkAll">
          Проверить все профили
        </UiButton>
        <UiButton @click="openUrls">Цели проверки</UiButton>
        <UiButton @click="loadHealth">Обновить сводку</UiButton>
      </div>

      <hr class="sep" />

      <p class="hint">
        Соединение с сервером активного профиля:
        {{ kaText || "данных пока нет" }}
      </p>
      <div class="acts">
        <UiButton :busy="kaBusy" @click="checkKeepalive">Проверить сейчас</UiButton>
      </div>
    </JournalArea>
  </div>

  <!-- ==================== шторки ==================== -->

  <DrawerSheet
    :open="cfgOpen"
    wide
    title="Конфигурация sing-box"
    @close="cfgOpen = false"
  >
    <div class="sheetc">
      <p class="hint">
        Сохранение пройдёт только если sing-box примет файл: роутер запускает проверку
        и возвращает её вывод целиком.
      </p>
      <p v-if="cfgLoading" class="hint">Читаю конфигурацию…</p>
      <CodeArea v-model="cfgText" label="Конфигурация sing-box" :rows="20" />
      <div v-if="cfgError" class="err">
        <p class="err-h">sing-box отверг файл:</p>
        <pre class="err-b">{{ cfgError }}</pre>
      </div>
    </div>
    <template #footer>
      <div class="sheetc acts">
        <UiButton variant="primary" :busy="cfgSaving" @click="saveConfig">
          Проверить и сохранить
        </UiButton>
        <UiButton :busy="cfgLoading" @click="loadConfig">Вернуть с роутера</UiButton>
        <UiButton @click="cfgOpen = false">Закрыть</UiButton>
      </div>
    </template>
  </DrawerSheet>

  <DrawerSheet :open="argsOpen" title="Аргументы zapret" @close="argsOpen = false">
    <div class="sheetc">
      <p class="hint">
        Строка запуска tpws: стратегия обхода DPI. После сохранения сервис
        перезапустится.
      </p>
      <CodeArea
        v-model="argsText"
        label="Аргументы zapret"
        :rows="6"
        placeholder="--split-pos=1 --disorder"
      />
    </div>
    <template #footer>
      <div class="sheetc acts">
        <UiButton variant="primary" :busy="argsBusy" @click="saveArgs">Сохранить</UiButton>
        <UiButton @click="argsOpen = false">Закрыть</UiButton>
      </div>
    </template>
  </DrawerSheet>

  <DrawerSheet :open="fwOpen" wide title="Правила файрвола" @close="fwOpen = false">
    <template #sticky>
      <div class="scroll-x">
        <SegmentedControl
          v-model="fwTab"
          label="Таблица правил"
          :options="[
            { value: 'nat', label: 'nat (iptables)' },
            { value: 'nft', label: 'nft (fw4)' },
          ]"
        />
      </div>
    </template>
    <div class="sheetc">
      <p v-if="fwBusy" class="hint">Читаю правила…</p>
      <LogPane
        :text="fwText"
        height="66vh"
        empty-text="Дамп пуст — на этой платформе таблица не используется."
      />
    </div>
    <template #footer>
      <div class="sheetc acts">
        <UiButton :busy="fwBusy" @click="loadFw">Перечитать</UiButton>
        <UiButton @click="fwOpen = false">Закрыть</UiButton>
      </div>
    </template>
  </DrawerSheet>

  <DrawerSheet
    :open="applyOpen"
    wide
    title="Установка обновления"
    @close="applyOpen = false"
  >
    <div class="sheetc">
      <p v-if="applyRunning" class="hint">
        Установка идёт. Не закрывайте роутер и не выключайте питание — журнал
        обновляется сам.
      </p>
      <p v-if="applyNote" class="note-strong">{{ applyNote }}</p>
      <LogPane
        :text="applyText"
        follow
        height="60vh"
        empty-text="Журнал установки пока пуст."
      />
    </div>
    <template #footer>
      <div class="sheetc acts">
        <UiButton v-if="!applyRunning" variant="primary" @click="reloadPage">
          Обновить страницу
        </UiButton>
        <UiButton @click="applyOpen = false">Закрыть</UiButton>
      </div>
    </template>
  </DrawerSheet>

  <DrawerSheet :open="clOpen" wide :title="clTitle" @close="clOpen = false">
    <div class="sheetc">
      <LogPane :text="clText" height="70vh" empty-text="Описание изменений не пришло." />
    </div>
  </DrawerSheet>

  <DrawerSheet
    :open="localOpen"
    wide
    title="Установка из файла"
    @close="localOpen = false"
  >
    <div class="sheetc">
      <p class="hint">
        Файл .ipk и, если он есть, текст подписи .ipk.sig. Без подписи пакет поставится
        обычным opkg — так тоже можно, но проверить происхождение файла будет нечем.
      </p>
      <label class="field">
        <span class="lbl">Пакет .ipk</span>
        <input type="file" accept=".ipk" @change="pickIpk" />
      </label>
      <p v-if="ipkFile" class="hint mono">
        {{ ipkFile.name }} · {{ Math.round(ipkFile.size / 1024) }} КБ
      </p>
      <label class="field">
        <span class="lbl">Подпись .ipk.sig (необязательно)</span>
        <CodeArea v-model="sigText" label="Подпись .ipk.sig" :rows="4" />
      </label>
    </div>
    <template #footer>
      <div class="sheetc acts">
        <UiButton
          variant="primary"
          :busy="updBusy === 'apply:local'"
          :disabled="!ipkFile"
          @click="installLocal"
        >
          Установить
        </UiButton>
        <UiButton @click="localOpen = false">Закрыть</UiButton>
      </div>
    </template>
  </DrawerSheet>

  <DrawerSheet :open="urlsOpen" wide title="Цели проверки" @close="urlsOpen = false">
    <div class="sheetc">
      <p class="hint">
        По строке на цель, в виде «Название|адрес». Пример:
        <code>Пример|https://panel.example.com/</code>
      </p>
      <p v-if="urlsBusy" class="hint">Читаю список…</p>
      <CodeArea v-model="urlsText" label="Цели проверки" :rows="12" />
    </div>
    <template #footer>
      <div class="sheetc acts">
        <UiButton variant="primary" :busy="urlsBusy" @click="saveUrls">Сохранить</UiButton>
        <UiButton @click="urlsOpen = false">Закрыть</UiButton>
      </div>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.journal {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
.acts {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
}
.hint {
  font-size: 12.5px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.note-strong {
  font-size: 13.5px;
  color: var(--ink);
  border-left: 2px solid var(--accent);
  padding-left: 10px;
}
.path {
  font-size: 11.5px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.sep {
  border: 0;
  border-top: 1px solid var(--line);
  margin: 2px 0;
  width: 100%;
}
.svc {
  display: flex;
  flex-direction: column;
  gap: 9px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  padding: 11px 12px;
  min-width: 0;
}
.svc-name {
  font-size: 15px;
  font-weight: 600;
}
.svc-name small {
  display: block;
  font-size: 12.5px;
  font-weight: 400;
  color: var(--dim);
}
.svc-name small.ok {
  color: var(--ok);
}
.svc-name small.bad {
  color: var(--bad);
}
.sheetc {
  display: flex;
  flex-direction: column;
  gap: 11px;
  min-width: 0;
  width: 100%;
}
.sheetc.acts {
  flex-direction: row;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.lbl {
  font-size: 13px;
  color: var(--dim);
}
.field input[type="file"] {
  /* 16px — иначе iOS зумит страницу; высота — чтобы попасть пальцем. */
  font-size: 16px;
  min-height: 44px;
  border: 1px dashed var(--line-2);
  border-radius: var(--radius-sm);
  padding: 9px 10px;
  background: var(--panel-2);
  max-width: 100%;
}
.err {
  border: 1px solid color-mix(in srgb, var(--bad) 45%, transparent);
  background: color-mix(in srgb, var(--bad) 10%, transparent);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
  min-width: 0;
}
.err-h {
  font-size: 13px;
  color: var(--bad);
  font-weight: 600;
  margin-bottom: 5px;
}
.err-b {
  margin: 0;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.45;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  overflow-x: auto;
  max-height: 40vh;
  color: var(--ink);
}

@media (max-width: 860px) {
  /* Цели нажатия на телефоне — не меньше 44px. Кнопки и переключатели
     приходят из общих компонентов, поэтому дотягиваемся через :deep. */
  .journal :deep(.btn),
  .sheetc :deep(.btn),
  .journal :deep(.seg button),
  .sheetc :deep(.seg button) {
    min-height: 44px;
  }
  .journal :deep(.row .text),
  .sheetc :deep(.row .text) {
    min-height: 44px;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
}
</style>
