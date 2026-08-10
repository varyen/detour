<script setup lang="ts">
/* Раздел «Правила»: кто ходит через туннель, кто в обход, а кто напрямую.
   Вместо семи вкладок старой панели — вертикальный список областей: строка
   «что сейчас настроено» плюс раскрытие. Большие списки редактируются в
   шторке, потому что на самом экране важнее видеть состояние целиком. */
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import RuleSection from "@/components/rules/RuleSection.vue";
import ListDrawer from "@/components/rules/ListDrawer.vue";
import RouteMapEditor from "@/components/rules/RouteMapEditor.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { overview, rules } from "@/api";
import type { HostsStatus, RoutingMode, RulistStatus, SingboxMode } from "@/api";
import { useStatusStore } from "@/stores/status";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";
import { useCommandStore } from "@/stores/commands";
import { asNum, fmtAgo, fmtBytes, fmtInt } from "@/lib/format";
import {
  countEntries,
  domainsLabel,
  entriesLabel,
  plural,
} from "@/components/rules/entries";

const status = useStatusStore();
const profilesStore = useProfilesStore();
const toast = useToastStore();
const commands = useCommandStore();

/* Бэкенд отдаёт больше полей, чем описано в типах панели: детали конкретного
   хелпера (detour-hosts, detour-rulist) типам API не принадлежат, но показать
   их человеку надо. */
type HostsFull = HostsStatus & {
  supported?: boolean;
  exclude_proxied?: boolean;
  bytes?: number;
  excluded?: number;
  error?: string;
};
type RulistFull = RulistStatus & {
  source_label?: string;
  live_entries?: number;
  excluded?: number;
  error?: string;
};

const sb = computed(() => status.data?.singbox);
const zp = computed(() => status.data?.zapret);

/* ---------- раскрытие областей ---------- */
const opened = reactive<Record<string, boolean>>({});
function toggle(id: string) {
  opened[id] = !opened[id];
  if (opened[id]) void onExpand(id);
}

/* ---------- списки ---------- */
type ListKey =
  | "domains"
  | "whitelist"
  | "zapret"
  | "routemap"
  | "udp"
  | "egress"
  | "ruexclude"
  | "hostsCustom"
  | "hostsView";

interface ListDef {
  title: string;
  hint: string;
  placeholder?: string;
  applyNote?: string;
  readonly?: boolean;
  /* Список применяется сразу при сохранении — второй кнопки не нужно. */
  singleSave?: boolean;
  applyLabel?: string;
  saveLabel?: string;
  load: () => Promise<string>;
  save?: (text: string) => Promise<unknown>;
  apply: (text: string) => Promise<unknown>;
  /* Что перечитать после удачного сохранения. */
  after?: () => void | Promise<void>;
}

const SLOW_NOTE =
  "Применение перестраивает конфигурацию и перезапускает подключение со сбросом" +
  " текущих соединений — на больших списках это может занять несколько минут.";

const LISTS: Record<ListKey, ListDef> = {
  domains: {
    title: "Сайты через VPN",
    hint: "Через туннель ходят только перечисленные сайты и адреса. Остальное идёт напрямую.",
    placeholder:
      "// Комментарий к разделу\nexample.com\nsub.example.com // и так тоже можно\n203.0.113.0/24",
    applyNote: SLOW_NOTE,
    load: async () => (await rules.domainsGet()).domains ?? "",
    save: (t) => rules.domainsSave(t),
    apply: (t) => rules.domainsSaveRestart(t),
    after: () => void status.refresh(true),
  },
  whitelist: {
    title: "Всегда напрямую",
    hint: "Эти сайты и подсети никогда не идут в туннель — даже когда включён режим «всё, кроме списка».",
    placeholder: "// Банки, госуслуги, локальные адреса\nexample.com\n192.168.0.0/16",
    applyNote: SLOW_NOTE,
    load: async () => (await rules.whitelistGet()).whitelist ?? "",
    save: (t) => rules.whitelistSave(t),
    apply: (t) => rules.whitelistSaveRestart(t),
    after: () => void status.refresh(true),
  },
  zapret: {
    title: "Сайты через обход DPI",
    hint: "Список общий для обеих стратегий обхода. Эти сайты открываются без туннеля — провайдеру просто мешают их распознать.",
    placeholder: "// Комментарий\nexample.com\n*.example.com",
    applyNote:
      "Применение обновляет список адресов и перезапускает обход DPI — обычно это несколько десятков секунд.",
    load: async () => (await rules.zapretDomainsGet()).domains ?? "",
    save: (t) => rules.zapretDomainsSave(t),
    apply: (t) => rules.zapretDomainsSaveRestart(t),
    after: () => void status.refresh(true),
  },
  routemap: {
    title: "Отдельные маршруты",
    hint: "Сайты, которым назначено собственное подключение.",
    applyNote: SLOW_NOTE,
    singleSave: true,
    load: async () => (await rules.routeMapGet()).routemap ?? "",
    apply: (t) => rules.routeMapSave(t),
    after: () => void status.refresh(true),
  },
  udp: {
    title: "UDP через VPN — список",
    hint: "Голос и игры по UDP заворачиваются в туннель только для этих адресов, подсетей, доменов и портов.",
    placeholder:
      "# хосты / подсети / домены / порты\n203.0.113.10            // весь UDP к этому адресу\n203.0.113.10:8211       // только порт 8211\n198.51.100.0/24         // подсеть\ngame.example.com        // домен\n:27015                  // порт с любого адреса\n19294:19344             // диапазон портов",
    applyNote:
      "Список применяется сразу: правила перехвата UDP перестраиваются, текущие голосовые соединения оборвутся.",
    singleSave: true,
    load: async () => (await overview.udpVpnListGet()).list ?? "",
    apply: (t) => overview.udpVpnListSet(t),
    after: () => void status.refreshExtras(),
  },
  egress: {
    title: "Запрещённые адреса",
    hint: "Соединения к этим адресам не выпускаются наружу ни от роутера, ни от устройств в сети. По одному IPv4 в строке.",
    placeholder: "// по одному адресу в строке\n203.0.113.7\n198.51.100.42",
    applyNote: "Правила файрвола применяются сразу, перезапуск подключения не нужен.",
    singleSave: true,
    load: async () => (await rules.egressBlocklistGet()).list ?? "",
    apply: (t) => rules.egressBlocklistSet(t),
  },
  ruexclude: {
    title: "Исключения из российских подсетей",
    hint: "Подсети, которые числятся российскими, но на деле работают из-за рубежа: они будут ходить через туннель, а не напрямую.",
    placeholder: "// по одной подсети или адресу в строке\n203.0.113.0/24\n198.51.100.7",
    applyNote: "Исключения применяются к списку адресов сразу.",
    singleSave: true,
    load: async () => (await rules.rulistExcludeGet()).exclude ?? "",
    apply: (t) => rules.rulistExcludeSet(t),
    after: () => loadRulist(),
  },
  hostsCustom: {
    title: "Свои записи hosts",
    hint: "Свои пары «адрес — имя». Они не теряются при обновлении списка из источника.",
    placeholder: "# адрес и имя через пробел\n203.0.113.10 example.com\n203.0.113.11 www.example.com",
    applyNote: "Записи применяются к DNS роутера сразу после сохранения.",
    singleSave: true,
    load: async () => (await rules.hostsCustomGet()).custom ?? "",
    apply: async (t) => {
      hosts.value = await rules.hostsCustomSave(t);
    },
  },
  hostsView: {
    title: "Что сейчас в списке hosts",
    hint: "Готовый список, который отдаёт DNS роутера. Только для просмотра.",
    readonly: true,
    load: async () => (await rules.hostsGet()).hosts ?? "",
    apply: async () => {},
  },
};

const listText = reactive<Record<ListKey, string>>({
  domains: "",
  whitelist: "",
  zapret: "",
  routemap: "",
  udp: "",
  egress: "",
  ruexclude: "",
  hostsCustom: "",
  hostsView: "",
});
const listLoaded = reactive<Record<string, boolean>>({});
const listLoading = ref<ListKey | "">("");
const listBusy = ref("");
const openList = ref<ListKey | "">("");
const openRoutes = ref(false);

async function ensureList(key: ListKey, force = false) {
  if (listLoaded[key] && !force) return;
  listLoading.value = key;
  try {
    listText[key] = await LISTS[key].load();
    listLoaded[key] = true;
  } catch (e) {
    toast.fromError(e, `Не удалось загрузить: ${LISTS[key].title}`);
  } finally {
    listLoading.value = "";
  }
}

function openEditor(key: ListKey) {
  if (key === "routemap") {
    openRoutes.value = true;
  } else {
    openList.value = key;
  }
  void ensureList(key);
}

async function saveList(key: ListKey, apply: boolean, text?: string) {
  const def = LISTS[key];
  const body = text ?? listText[key];
  listBusy.value = apply ? "apply" : "save";
  try {
    if (apply || !def.save) await def.apply(body);
    else await def.save(body);
    listText[key] = body;
    listLoaded[key] = true;
    toast.ok(apply ? `${def.title}: сохранено и применено` : `${def.title}: сохранено`);
    if (apply) await def.after?.();
    openList.value = "";
    openRoutes.value = false;
  } catch (e) {
    toast.fromError(e, "Сохранить не удалось");
  } finally {
    listBusy.value = "";
  }
}

function listCount(key: ListKey): number {
  return countEntries(listText[key]);
}

/* ---------- режимы маршрутизации ---------- */
const routingMode = ref<RoutingMode>("proxy-list");
const singboxMode = ref<SingboxMode>("single");
const busy = ref("");

async function loadSettings() {
  try {
    const s = await rules.settingsGet();
    if (s.routing_mode) routingMode.value = s.routing_mode;
    if (s.singbox_mode) singboxMode.value = s.singbox_mode;
  } catch {
    /* Состояние есть и в общем статусе — молча берём оттуда. */
    if (sb.value?.routing_mode) routingMode.value = sb.value.routing_mode;
    if (sb.value?.singbox_mode) singboxMode.value = sb.value.singbox_mode;
  }
}

async function applySettings(patch: {
  routing_mode?: RoutingMode;
  singbox_mode?: SingboxMode;
}) {
  const prevRouting = routingMode.value;
  const prevSingbox = singboxMode.value;
  if (patch.routing_mode) routingMode.value = patch.routing_mode;
  if (patch.singbox_mode) singboxMode.value = patch.singbox_mode;
  busy.value = "settings";
  try {
    await rules.settingsSet(patch);
    toast.ok("Режим применён");
    void status.refresh(true);
    void loadSelfIntercept();
  } catch (e) {
    routingMode.value = prevRouting;
    singboxMode.value = prevSingbox;
    toast.fromError(e, "Не удалось сменить режим");
  } finally {
    busy.value = "";
  }
}

const routingModeProxy = computed<RoutingMode>({
  get: () => routingMode.value,
  set: (v) => {
    if (v !== routingMode.value) void applySettings({ routing_mode: v });
  },
});
const singboxModeProxy = computed<SingboxMode>({
  get: () => singboxMode.value,
  set: (v) => {
    if (v !== singboxMode.value) void applySettings({ singbox_mode: v });
  },
});

/* ---------- перехват трафика роутера ---------- */
interface SelfIntercept {
  targets: string[];
  full_targets: string[];
  eligible: string[];
  mode: SingboxMode;
}
const si = ref<SelfIntercept | null>(null);
const siMode = reactive<Record<string, "off" | "split" | "full">>({});

async function loadSelfIntercept() {
  try {
    const d = await rules.selfInterceptGet();
    si.value = {
      targets: d.targets ?? [],
      full_targets: d.full_targets ?? [],
      eligible: d.eligible ?? [],
      mode: d.mode ?? "single",
    };
    /* Этот же ответ — самый свежий источник правды о числе процессов. */
    if (d.mode) singboxMode.value = d.mode;
    for (const id of si.value.eligible) {
      siMode[id] = si.value.full_targets.includes(id)
        ? "full"
        : si.value.targets.includes(id)
          ? "split"
          : "off";
    }
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать настройки перехвата");
  }
}

async function applySelfIntercept() {
  const csv = (si.value?.eligible ?? [])
    .filter((id) => siMode[id] && siMode[id] !== "off")
    .map((id) => (siMode[id] === "full" ? `${id}:full` : id))
    .join(",");
  busy.value = "si";
  try {
    const d = await rules.selfInterceptSet(csv);
    if (si.value) {
      si.value.targets = d.targets ?? [];
      si.value.full_targets = d.full_targets ?? [];
    }
    toast.ok(csv ? "Перехват применён" : "Перехват выключен");
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось применить перехват");
  } finally {
    busy.value = "";
  }
}

/* ---------- российские подсети ---------- */
const rulist = ref<RulistFull | null>(null);

/* rulist_set/rulist_update отвечают статусом самого хелпера — без обёртки
   "supported", которую добавляет только rulist_status. Затирать флаг нельзя,
   иначе после любой правки область объявит себя недоступной. */
function mergeRulist(d: RulistFull) {
  const prev = rulist.value;
  rulist.value = { ...(prev ?? {}), ...d, supported: d.supported ?? prev?.supported ?? true };
}

async function loadRulist() {
  try {
    rulist.value = (await rules.rulistStatus()) as RulistFull | null;
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать состояние списка подсетей");
  }
}

async function rulistSet(patch: {
  source?: "maxmind" | "rir";
  auto?: boolean;
  enabled?: boolean;
}) {
  busy.value = "rulist";
  try {
    mergeRulist((await rules.rulistSet(patch)) as RulistFull);
    toast.ok("Настройка сохранена");
  } catch (e) {
    toast.fromError(e, "Не удалось изменить настройку");
    void loadRulist();
  } finally {
    busy.value = "";
  }
}

async function rulistUpdate() {
  busy.value = "rulist-update";
  toast.info("Скачиваю список подсетей — это может занять пару минут");
  try {
    const d = (await rules.rulistUpdate()) as RulistFull;
    mergeRulist(d);
    if (d.error) toast.error(d.error);
    else toast.ok(`Список обновлён: ${fmtInt(asNum(d.count))} подсетей`);
  } catch (e) {
    toast.fromError(e, "Обновить список не удалось");
  } finally {
    busy.value = "";
  }
}

const rulistSource = computed<"maxmind" | "rir">({
  get: () => rulist.value?.source ?? "maxmind",
  set: (v) => {
    if (v !== rulist.value?.source) void rulistSet({ source: v });
  },
});

/* ---------- приоритетный hosts и шифрованный DNS ---------- */
const hosts = ref<HostsFull | null>(null);
const hostsUrl = ref("");
const secureOn = ref(false);
const secureList = ref("");
const fileEl = ref<HTMLInputElement | null>(null);

async function loadHosts() {
  try {
    const d = (await rules.hostsStatus()) as HostsFull | null;
    hosts.value = d;
    if (d) {
      if (d.url) hostsUrl.value = d.url;
      secureOn.value = d.secure_dns_mode === "secure";
      secureList.value = (d.secure_dns_list ?? "").trim().replace(/\s+/g, "\n");
    }
  } catch (e) {
    toast.fromError(e, "Не удалось прочитать состояние hosts");
  }
}

async function hostsSet(patch: { url?: string; enabled?: boolean }, what: string) {
  busy.value = "hosts";
  try {
    hosts.value = (await rules.hostsSet(patch)) as HostsFull;
    toast.ok(what);
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить");
    void loadHosts();
  } finally {
    busy.value = "";
  }
}

async function hostsRefresh() {
  busy.value = "hosts-refresh";
  toast.info("Скачиваю список — это может занять пару минут");
  try {
    const d = (await rules.hostsRefresh()) as HostsFull;
    hosts.value = d;
    listLoaded.hostsView = false;
    if (d.error) toast.error(d.error);
    else toast.ok(`Список обновлён: ${fmtInt(asNum(d.count))} записей`);
  } catch (e) {
    toast.fromError(e, "Обновить список не удалось");
  } finally {
    busy.value = "";
  }
}

async function hostsToggle(kind: "enabled" | "exclude" | "custom", on: boolean) {
  busy.value = `hosts-${kind}`;
  try {
    if (kind === "enabled") hosts.value = (await rules.hostsSet({ enabled: on })) as HostsFull;
    if (kind === "exclude") hosts.value = (await rules.hostsExclude(on)) as HostsFull;
    if (kind === "custom") hosts.value = (await rules.hostsCustomToggle(on)) as HostsFull;
    toast.ok("Настройка сохранена");
  } catch (e) {
    toast.fromError(e, "Не удалось изменить настройку");
    void loadHosts();
  } finally {
    busy.value = "";
  }
}

async function onHostsFile(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;
  busy.value = "hosts-upload";
  try {
    hosts.value = (await rules.hostsUpload(await file.text())) as HostsFull;
    listLoaded.hostsView = false;
    toast.ok("Файл загружен и применён");
  } catch (err) {
    toast.fromError(err, "Загрузить файл не удалось");
  } finally {
    busy.value = "";
    input.value = "";
  }
}

async function saveSecureDns() {
  busy.value = "secure-dns";
  try {
    await rules.secureDnsSet(
      secureOn.value ? "secure" : "auto",
      secureOn.value ? secureList.value.trim() : "",
    );
    toast.ok(
      secureOn.value
        ? "Запросы к DNS теперь шифруются"
        : "DNS снова берётся у провайдера",
    );
    void loadHosts();
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить настройку DNS");
  } finally {
    busy.value = "";
  }
}

/* ---------- сводные строки ---------- */
const modeSummary = computed(() => {
  const scope =
    routingMode.value === "all-except"
      ? "через туннель идёт всё, кроме списка исключений"
      : "через туннель идут только выбранные сайты";
  const inst = singboxMode.value === "single" ? "один процесс" : "по процессу на профиль";
  return `${scope} · ${inst}`;
});

const domainsSummary = computed(() =>
  listLoaded.domains
    ? domainsLabel(listCount("domains"))
    : domainsLabel(asNum(sb.value?.domains)),
);

const whitelistSummary = computed(() => {
  const base = listLoaded.whitelist
    ? entriesLabel(listCount("whitelist"))
    : "открыть, чтобы посмотреть";
  return routingMode.value === "all-except"
    ? `${base} · сейчас определяет, что идёт мимо туннеля`
    : `${base} · работает в режиме «всё, кроме списка» и при «все через VPN»`;
});

const zapretSummary = computed(() =>
  listLoaded.zapret
    ? domainsLabel(listCount("zapret"))
    : domainsLabel(asNum(zp.value?.domains)),
);

const routeCount = computed(() =>
  listLoaded.routemap
    ? (listText.routemap.match(/^\s*\/\/\s*===\s*route:/gm) ?? []).length
    : asNum(sb.value?.route_targets),
);
const routeSummary = computed(() =>
  routeCount.value
    ? `${routeCount.value} ${plural(routeCount.value, ["отдельный маршрут", "отдельных маршрута", "отдельных маршрутов"])}`
    : "особых маршрутов нет",
);

const siNames = computed(() =>
  (si.value?.targets ?? []).map((id) => profileName(id)).join(", "),
);
const siSummary = computed(() => {
  if (singboxMode.value !== "single") return "доступно только при одном процессе";
  if (!si.value?.eligible.length) return "подходящих прокси-целей нет";
  return siNames.value ? `перехватываются: ${siNames.value}` : "выключен";
});

const udpSummary = computed(() => {
  if (!status.udpVpnSupported) return "на этой платформе недоступно";
  const mode = status.udp?.mode ?? "off";
  const list = listLoaded.udp ? entriesLabel(listCount("udp")) : "список не загружен";
  if (mode === "off") return `выключено на «Обзоре» · ${list}`;
  if (mode === "all") return `в туннель идёт весь UDP · список не используется`;
  return `по списку · ${list}`;
});

const egressSummary = computed(() =>
  listLoaded.egress
    ? listCount("egress")
      ? `${entriesLabel(listCount("egress"))} заблокировано`
      : "ничего не заблокировано"
    : "открыть, чтобы посмотреть",
);

const rulistSummary = computed(() => {
  const r = rulist.value;
  if (!r || r.supported === false) return "не установлено на этом роутере";
  if (r.enabled === false) return "выключено — российские адреса не выделяются";
  const parts = [`${fmtInt(asNum(r.count))} подсетей`];
  if (r.source_label) parts.push(r.source_label);
  else parts.push(r.source === "rir" ? "по регистрации" : "по расположению");
  if (r.updated) parts.push(`обновлено ${fmtAgo(asNum(r.updated))}`);
  if (r.auto) parts.push("обновляется само");
  return parts.join(" · ");
});

const hostsSummary = computed(() => {
  const h = hosts.value;
  if (!h || h.supported === false) return "не установлено на этом роутере";
  if (!h.enabled) return "выключено";
  const parts = [`${fmtInt(asNum(h.count))} записей`];
  if (h.bytes) parts.push(fmtBytes(asNum(h.bytes)));
  if (h.updated) parts.push(`обновлено ${fmtAgo(asNum(h.updated))}`);
  return parts.join(" · ");
});

/* Встроенное шифрование DNS живёт в прошивке GL.iNet: если бэкенд не вернул
   режим, управлять нечем — показываем объяснение, а не сломанный контрол. */
const secureDnsAvailable = computed(() => {
  const mode = hosts.value?.secure_dns_mode;
  return typeof mode === "string" && mode !== "";
});

function profileName(id: string): string {
  return profilesStore.items.find((p) => p.id === id)?.name ?? id;
}

/* Цели маршрутов: профили и сохранённые цепочки в одном списке. */
const routeTargets = computed(() => [
  ...profilesStore.items.map((p) => ({
    id: p.id,
    name: p.group ? `${p.name} · ${p.group}` : p.name,
    isChain: false,
  })),
  ...profilesStore.chainList.map((c) => ({
    id: c.id,
    name: `${c.name} (${c.hops.length} шт.)`,
    isChain: true,
  })),
]);

/* ---------- загрузка по требованию ---------- */
async function onExpand(id: string) {
  if (id === "domains") await ensureList("domains");
  else if (id === "whitelist") await ensureList("whitelist");
  else if (id === "zapret") await ensureList("zapret");
  else if (id === "routes") await ensureList("routemap");
  else if (id === "udp") await ensureList("udp");
  else if (id === "egress") await ensureList("egress");
  else if (id === "rulist") await ensureList("ruexclude");
  else if (id === "hosts") await ensureList("hostsCustom");
}

let unregister: (() => void) | undefined;

onMounted(async () => {
  unregister = commands.register([
    {
      id: "rules:domains",
      title: "Открыть список сайтов через VPN",
      group: "правила",
      keywords: "домены список туннель",
      run: () => openEditor("domains"),
    },
    {
      id: "rules:whitelist",
      title: "Открыть список «всегда напрямую»",
      group: "правила",
      keywords: "белый исключения whitelist",
      run: () => openEditor("whitelist"),
    },
    {
      id: "rules:zapret",
      title: "Открыть список сайтов обхода DPI",
      group: "правила",
      keywords: "zapret домены dpi",
      run: () => openEditor("zapret"),
    },
    {
      id: "rules:routes",
      title: "Открыть отдельные маршруты",
      group: "правила",
      keywords: "route map домен профиль цепочка",
      run: () => openEditor("routemap"),
    },
    {
      id: "rules:udp",
      title: "Открыть список UDP через VPN",
      group: "правила",
      keywords: "udp игры голос",
      available: () => status.udpVpnSupported,
      run: () => openEditor("udp"),
    },
    {
      id: "rules:egress",
      title: "Открыть список запрещённых адресов",
      group: "правила",
      keywords: "блок blocklist ip",
      run: () => openEditor("egress"),
    },
    {
      id: "rules:rulist",
      title: "Обновить российские подсети",
      group: "правила",
      keywords: "ru подсети geo",
      available: () => rulist.value?.supported !== false,
      run: () => void rulistUpdate(),
    },
    {
      id: "rules:hosts",
      title: "Обновить приоритетный hosts",
      group: "правила",
      keywords: "dns hosts",
      available: () => hosts.value?.supported !== false,
      run: () => void hostsRefresh(),
    },
  ]);

  await Promise.allSettled([
    loadSettings(),
    loadSelfIntercept(),
    loadRulist(),
    loadHosts(),
    profilesStore.load(),
  ]);
  void profilesStore.loadChains();
});

onBeforeUnmount(() => unregister?.());
</script>

<template>
  <div class="areas">
    <!-- 1. Режимы -->
    <RuleSection
      title="Что уходит в туннель"
      :summary="modeSummary"
      :open="!!opened.mode"
      @toggle="toggle('mode')"
    >
      <div class="field">
        <p class="lbl">Правило по умолчанию</p>
        <div class="scroll-x">
          <SegmentedControl
            v-model="routingModeProxy"
            label="Правило по умолчанию"
            :busy="busy === 'settings'"
            :options="[
              { value: 'proxy-list', label: 'Только выбранные сайты' },
              { value: 'all-except', label: 'Всё, кроме исключений' },
            ]"
          />
        </div>
        <p class="hint">
          «Только выбранные сайты» — через туннель ходит список ниже. «Всё, кроме
          исключений» — наоборот: в туннеле всё, что не попало в список
          «всегда напрямую».
        </p>
      </div>

      <div class="field">
        <p class="lbl">Как работает подключение</p>
        <div class="scroll-x">
          <SegmentedControl
            v-model="singboxModeProxy"
            label="Как работает подключение"
            :busy="busy === 'settings'"
            :options="[
              { value: 'single', label: 'Один процесс' },
              { value: 'multi', label: 'По процессу на профиль' },
            ]"
          />
        </div>
        <p class="hint">
          Один процесс экономит память роутера. Отдельные процессы нужны, когда
          разные сайты постоянно ходят через разные подключения.
        </p>
      </div>

      <p class="warn">
        Смена режима перестраивает конфигурацию и перезапускает подключение —
        это занимает несколько секунд, текущие соединения оборвутся.
      </p>
    </RuleSection>

    <!-- 2. Домены через VPN -->
    <RuleSection
      title="Сайты через VPN"
      :summary="domainsSummary"
      :open="!!opened.domains"
      :tag="routingMode === 'all-except' ? 'сейчас не применяется' : undefined"
      tone="warn"
      @toggle="toggle('domains')"
    >
      <p class="hint">
        Через туннель ходят только эти сайты и адреса. Домен, адрес, подсеть —
        по одному в строке; строка после <code>//</code> считается заметкой.
      </p>
      <p v-if="routingMode === 'all-except'" class="warn">
        Сейчас выбрано «всё, кроме исключений» — этот список не используется.
      </p>
      <p v-if="listLoading === 'domains'" class="hint">Загружаю список…</p>
      <div class="row">
        <UiButton variant="primary" @click="openEditor('domains')">
          Редактировать список
        </UiButton>
        <UiButton :busy="listLoading === 'domains'" @click="ensureList('domains', true)">
          Перечитать
        </UiButton>
      </div>
    </RuleSection>

    <!-- 3. Белый список -->
    <RuleSection
      title="Всегда напрямую"
      :summary="whitelistSummary"
      :open="!!opened.whitelist"
      @toggle="toggle('whitelist')"
    >
      <p class="hint">
        Банки, госуслуги, локальная сеть — всё, что должно ходить в обход
        туннеля даже когда включено «все через VPN».
      </p>
      <div class="row">
        <UiButton variant="primary" @click="openEditor('whitelist')">
          Редактировать список
        </UiButton>
        <UiButton
          :busy="listLoading === 'whitelist'"
          @click="ensureList('whitelist', true)"
        >
          Перечитать
        </UiButton>
      </div>
    </RuleSection>

    <!-- 4. Домены обхода DPI -->
    <RuleSection
      title="Сайты через обход DPI"
      :summary="zapretSummary"
      :open="!!opened.zapret"
      @toggle="toggle('zapret')"
    >
      <p class="hint">
        Эти сайты открываются без туннеля: трафик идёт напрямую, но так, что
        провайдер не может его распознать. Список общий для обеих стратегий
        обхода — сама стратегия включается на «Обзоре».
      </p>
      <div class="row">
        <UiButton variant="primary" @click="openEditor('zapret')">
          Редактировать список
        </UiButton>
        <UiButton :busy="listLoading === 'zapret'" @click="ensureList('zapret', true)">
          Перечитать
        </UiButton>
      </div>
    </RuleSection>

    <!-- 5. Маршруты -->
    <RuleSection
      title="Отдельные маршруты"
      :summary="routeSummary"
      :open="!!opened.routes"
      @toggle="toggle('routes')"
    >
      <p class="hint">
        Обычно весь туннельный трафик идёт через одно активное подключение.
        Здесь можно назначить отдельным сайтам своё: например, рабочие сервисы —
        через прокси, а всё остальное — как раньше.
      </p>
      <div class="row">
        <UiButton variant="primary" @click="openEditor('routemap')">
          Настроить маршруты
        </UiButton>
        <UiButton :busy="listLoading === 'routemap'" @click="ensureList('routemap', true)">
          Перечитать
        </UiButton>
      </div>
    </RuleSection>

    <!-- 6. Перехват прокси -->
    <RuleSection
      title="Перехват трафика к прокси"
      :summary="siSummary"
      :open="!!opened.si"
      :tag="singboxMode !== 'single' ? 'недоступно' : undefined"
      @toggle="toggle('si')"
    >
      <p class="hint">
        Если устройство жёстко настроено на прокси, который здесь назначен целью
        маршрута, роутер может перехватить это соединение и сам решить, что идёт
        через прокси, а что — обычным путём.
      </p>
      <p v-if="singboxMode !== 'single'" class="warn">
        Работает только при одном процессе. Смените режим в области
        «Что уходит в туннель».
      </p>
      <p v-else-if="!si?.eligible.length" class="warn">
        Подходящих целей нет: перехват возможен только для SOCKS- и
        HTTP-профилей, назначенных целью отдельного маршрута.
      </p>
      <template v-else>
        <div v-for="id in si?.eligible ?? []" :key="id" class="si-row">
          <span class="si-name">{{ profileName(id) }}</span>
          <div class="scroll-x">
            <SegmentedControl
              v-model="siMode[id]"
              :label="`Перехват для ${profileName(id)}`"
              :busy="busy === 'si'"
              :options="[
                { value: 'off', label: 'Выкл' },
                { value: 'split', label: 'По маршруту' },
                { value: 'full', label: 'Весь трафик' },
              ]"
            />
          </div>
        </div>
        <p class="hint">
          «По маршруту» — сайты маршрута идут через этот прокси, остальное
          обычным путём. «Весь трафик» — через прокси уходит всё, что пришло на
          перехваченное соединение.
        </p>
        <div class="row">
          <UiButton variant="primary" :busy="busy === 'si'" @click="applySelfIntercept">
            Применить перехват
          </UiButton>
        </div>
        <p class="hint">
          Применение перезапускает подключение — несколько секунд без сети.
        </p>
      </template>
    </RuleSection>

    <!-- 7. UDP через VPN -->
    <RuleSection
      title="UDP через VPN — список"
      :summary="udpSummary"
      :open="!!opened.udp"
      :tag="status.udpVpnSupported ? undefined : 'недоступно'"
      @toggle="toggle('udp')"
    >
      <p class="hint">
        Голосовые чаты и игры работают по UDP, и обычный обход им не помогает.
        Здесь перечислено, какой именно UDP-трафик заворачивать в туннель. Сам
        режим включается на «Обзоре».
      </p>
      <p v-if="!status.udpVpnSupported" class="warn">
        На этой платформе перехват UDP недоступен: нужен TPROXY, которого здесь
        нет.
      </p>
      <div v-else class="row">
        <UiButton variant="primary" @click="openEditor('udp')">
          Редактировать список
        </UiButton>
        <UiButton :busy="listLoading === 'udp'" @click="ensureList('udp', true)">
          Перечитать
        </UiButton>
      </div>
    </RuleSection>

    <!-- 8. Блок-лист исходящих -->
    <RuleSection
      title="Запрещённые адреса"
      :summary="egressSummary"
      :open="!!opened.egress"
      @toggle="toggle('egress')"
    >
      <p class="hint">
        Соединения к этим адресам роутер не выпускает — ни свои, ни от устройств
        в сети. Полезно, когда какой-то сервис нужно отрезать целиком.
      </p>
      <div class="row">
        <UiButton variant="primary" @click="openEditor('egress')">
          Редактировать список
        </UiButton>
        <UiButton :busy="listLoading === 'egress'" @click="ensureList('egress', true)">
          Перечитать
        </UiButton>
      </div>
    </RuleSection>

    <!-- 9. Российские подсети -->
    <RuleSection
      title="Российские адреса — напрямую"
      :summary="rulistSummary"
      :open="!!opened.rulist"
      :tag="rulist?.supported === false ? 'недоступно' : undefined"
      @toggle="toggle('rulist')"
    >
      <p class="hint">
        Готовый список российских подсетей, чтобы местные сайты не ходили через
        зарубежный туннель. Обновляется с публичного источника.
      </p>
      <p v-if="rulist?.supported === false" class="warn">
        Управление списком недоступно: на роутере нет нужного компонента.
      </p>
      <template v-else>
        <SwitchToggle
          :model-value="rulist?.enabled !== false"
          label="Использовать список"
          :busy="busy === 'rulist'"
          hint="Выключение вернёт российские адреса в общий поток"
          @update:model-value="(v) => rulistSet({ enabled: v })"
        />
        <SwitchToggle
          :model-value="rulist?.auto === true"
          label="Обновлять самому"
          :busy="busy === 'rulist'"
          hint="Раз в несколько дней роутер скачает свежий список"
          @update:model-value="(v) => rulistSet({ auto: v })"
        />
        <div class="field">
          <p class="lbl">Откуда брать список</p>
          <div class="scroll-x">
            <SegmentedControl
              v-model="rulistSource"
              label="Источник списка подсетей"
              :busy="busy === 'rulist'"
              :options="[
                { value: 'maxmind', label: 'По расположению' },
                { value: 'rir', label: 'По регистрации' },
              ]"
            />
          </div>
          <p class="hint">
            «По расположению» точнее: подсеть, зарегистрированная в России, но
            размещённая за рубежом, туда не попадёт. Смена источника только
            запоминает выбор — список нужно обновить кнопкой.
          </p>
        </div>
        <p v-if="rulist?.error" class="warn">{{ rulist.error }}</p>
        <p v-if="rulist?.excluded" class="hint">
          Исключений: {{ fmtInt(asNum(rulist.excluded)) }}
        </p>
        <div class="row">
          <UiButton
            variant="primary"
            :busy="busy === 'rulist-update'"
            @click="rulistUpdate"
          >
            Обновить список
          </UiButton>
          <UiButton @click="openEditor('ruexclude')">Исключения</UiButton>
        </div>
      </template>
    </RuleSection>

    <!-- 10. Приоритетный hosts + Secure DNS -->
    <RuleSection
      title="Свой DNS-список и шифрование DNS"
      :summary="hostsSummary"
      :open="!!opened.hosts"
      :tag="hosts?.supported === false ? 'недоступно' : undefined"
      @toggle="toggle('hosts')"
    >
      <p class="hint">
        Список «имя — адрес», который роутер отдаёт в первую очередь, минуя
        обычный DNS. Такие сайты идут прямо на указанный адрес, не через туннель
        и не через обход.
      </p>
      <p v-if="hosts?.supported === false" class="warn">
        Управление списком недоступно: на роутере нет нужного компонента.
      </p>
      <template v-else>
        <SwitchToggle
          :model-value="hosts?.enabled === true"
          label="Использовать список"
          :busy="busy === 'hosts-enabled'"
          @update:model-value="(v) => hostsToggle('enabled', v)"
        />
        <SwitchToggle
          :model-value="hosts?.exclude_proxied !== false"
          label="Пропускать то, что и так идёт через туннель"
          :busy="busy === 'hosts-exclude'"
          hint="Иначе сайт из списка VPN пойдёт мимо туннеля"
          @update:model-value="(v) => hostsToggle('exclude', v)"
        />
        <SwitchToggle
          :model-value="hosts?.custom_enabled !== false"
          label="Применять мои записи"
          :busy="busy === 'hosts-custom'"
          @update:model-value="(v) => hostsToggle('custom', v)"
        />

        <div class="field">
          <label class="lbl" for="hosts-url">Ссылка на источник</label>
          <input
            id="hosts-url"
            v-model="hostsUrl"
            class="inp mono"
            type="url"
            inputmode="url"
            spellcheck="false"
            autocapitalize="off"
            autocomplete="off"
            placeholder="https://panel.example.com/hosts.txt"
          />
          <div class="row">
            <UiButton
              :busy="busy === 'hosts'"
              :disabled="!hostsUrl.trim()"
              @click="hostsSet({ url: hostsUrl.trim() }, 'Ссылка сохранена')"
            >
              Сохранить ссылку
            </UiButton>
            <UiButton :busy="busy === 'hosts-refresh'" @click="hostsRefresh">
              Скачать заново
            </UiButton>
          </div>
        </div>

        <p v-if="hosts?.error" class="warn">{{ hosts.error }}</p>
        <p v-if="hosts?.excluded" class="hint">
          Пропущено записей: {{ fmtInt(asNum(hosts.excluded)) }}
        </p>

        <div class="row">
          <UiButton @click="openEditor('hostsCustom')">Мои записи</UiButton>
          <UiButton @click="openEditor('hostsView')">Посмотреть список</UiButton>
          <UiButton :busy="busy === 'hosts-upload'" @click="fileEl?.click()">
            Загрузить файлом
          </UiButton>
          <input
            ref="fileEl"
            class="file"
            type="file"
            accept=".txt,.list,text/plain"
            @change="onHostsFile"
          />
        </div>

        <div v-if="secureDnsAvailable" class="field bordered">
          <p class="lbl">Шифрование DNS</p>
          <SwitchToggle
            v-model="secureOn"
            label="Скрывать DNS-запросы от провайдера"
            hint="Роутер сам обращается к зашифрованным DNS-серверам вместо провайдерских"
          />
          <textarea
            v-if="secureOn"
            v-model="secureList"
            class="inp ta mono"
            spellcheck="false"
            autocapitalize="off"
            autocomplete="off"
            aria-label="Адреса зашифрованных DNS-серверов"
            placeholder="https://dns.example.com/dns-query"
          ></textarea>
          <p class="hint">
            По одному адресу в строке. Пусто — роутер возьмёт свои встроенные
            серверы.
          </p>
          <div class="row">
            <UiButton
              variant="primary"
              :busy="busy === 'secure-dns'"
              @click="saveSecureDns"
            >
              Сохранить настройку DNS
            </UiButton>
          </div>
        </div>
        <p v-else class="hint">
          Встроенное шифрование DNS доступно только на роутерах GL.iNet.
        </p>
      </template>
    </RuleSection>
  </div>

  <ListDrawer
    v-for="key in (
      [
        'domains',
        'whitelist',
        'zapret',
        'udp',
        'egress',
        'ruexclude',
        'hostsCustom',
        'hostsView',
      ] as const
    )"
    :key="key"
    v-model="listText[key]"
    :open="openList === key"
    :title="LISTS[key].title"
    :hint="LISTS[key].hint"
    :placeholder="LISTS[key].placeholder"
    :apply-note="LISTS[key].applyNote"
    :readonly="LISTS[key].readonly"
    :single-save="LISTS[key].singleSave"
    :loading="listLoading === key"
    :busy="listBusy"
    @close="openList = ''"
    @save="(apply) => saveList(key, apply)"
  />

  <RouteMapEditor
    :open="openRoutes"
    :text="listText.routemap"
    :targets="routeTargets"
    :loading="listLoading === 'routemap'"
    :busy="listBusy === 'apply'"
    @close="openRoutes = false"
    @save="(text) => saveList('routemap', true, text)"
  />
</template>

<style scoped>
.areas {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.field.bordered {
  border-top: 1px solid var(--line);
  padding-top: 12px;
  margin-top: 2px;
}
.lbl {
  font-size: 13.5px;
  font-weight: 600;
}
.hint {
  font-size: 12.5px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.warn {
  font-size: 12.5px;
  color: var(--warn);
  border: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
  background: color-mix(in srgb, var(--warn) 8%, transparent);
  border-radius: var(--radius-sm);
  padding: 8px 11px;
}
.row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
}
.si-row {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
  padding: 6px 0;
  border-bottom: 1px solid var(--line);
}
.si-name {
  font-size: 14px;
  min-width: 0;
  overflow-wrap: anywhere;
  flex: 1 1 140px;
}
.inp {
  width: 100%;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 10px 12px;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  min-height: 44px;
  outline: none;
}
.inp:focus {
  border-color: var(--accent);
}
.ta {
  min-height: 92px;
  resize: vertical;
  line-height: 1.45;
}
.file {
  display: none;
}
@media (max-width: 700px) {
  .si-row {
    align-items: flex-start;
    flex-direction: column;
    gap: 6px;
  }
  /* На телефоне палец не целится: общие кнопки и переключатели дорастают до
     44 px, хотя на десктопе им хватает меньшего. */
  .row :deep(.btn),
  .scroll-x :deep(.seg button) {
    min-height: 44px;
  }
}
</style>
