<script setup lang="ts">
/* Свой VPN-сервер на роутере: AmneziaWG (UDP) и VLESS-Reality (TCP), один
   набор клиентов. Клиенты попадают на отдельные интерфейсы, на которые роутер
   вешает те же правила, что и на домашнюю сеть:
   заблокированное идёт через VPN, российское — напрямую, zapret и «Отдельные
   маршруты» тоже работают. Пока область раскрыта, статус опрашивается каждые
   три секунды — по разнице счётчиков считается скорость клиентов. */
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import ServicePanel from "@/components/services/ServicePanel.vue";
import ServerClientSheet from "@/components/services/ServerClientSheet.vue";
import FormField from "@/components/services/FormField.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { diag, poll, services } from "@/api";
import type { ApplyLogResponse, ServerClient, ServerClientMode, ServerStatus } from "@/api";
import { fmtAgo, fmtBitrate, fmtBytes } from "@/lib/format";
import { useProfilesStore } from "@/stores/profiles";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";

const open = defineModel<boolean>("open", { required: true });
const toast = useToastStore();
const status = useStatusStore();
const profilesStore = useProfilesStore();

const st = ref<ServerStatus | null>(null);
const loadError = ref("");
const busy = ref("");
const confirmDelete = ref("");
const confirmRegen = ref(false);

const MODE_OPTIONS: { value: ServerClientMode; label: string; hint: string }[] = [
  { value: "full", label: "Весь трафик", hint: "Через роутер идёт всё, с его маршрутами" },
  { value: "lan", label: "Только домашняя сеть", hint: "Через роутер — только доступ к дому" },
];

/* ---------------- загрузка и скорость ---------------- */

interface Sample {
  rx: number;
  tx: number;
  t: number;
}
const prev = new Map<string, Sample>();
/** Байт/с к клиенту (down) и от клиента (up). */
const speed = reactive<Record<string, { down: number; up: number }>>({});

async function load() {
  try {
    const s = await services.serverStatus();
    if (!s) {
      loadError.value = "Роутер не отдал состояние сервера — обновите панель";
      return;
    }
    const t = Date.now() / 1000;
    for (const c of s.clients ?? []) {
      const p = prev.get(c.id);
      if (p && t > p.t && c.rx >= p.rx && c.tx >= p.tx) {
        speed[c.id] = { down: (c.tx - p.tx) / (t - p.t), up: (c.rx - p.rx) / (t - p.t) };
      } else if (!c.online) {
        speed[c.id] = { down: 0, up: 0 };
      }
      prev.set(c.id, { rx: c.rx, tx: c.tx, t });
    }
    st.value = s;
    loadError.value = "";
  } catch (e) {
    loadError.value = e instanceof Error ? e.message : "Не удалось прочитать состояние сервера";
  }
}

let timer: ReturnType<typeof setInterval> | undefined;
function startPolling() {
  stopPolling();
  timer = setInterval(() => {
    if (document.visibilityState === "visible" && !busy.value) void load();
  }, 3000);
}
function stopPolling() {
  if (timer) clearInterval(timer);
  timer = undefined;
}

watch(
  open,
  (o) => {
    if (o) {
      void load();
      startPolling();
    } else {
      stopPolling();
    }
  },
  { immediate: true },
);
onBeforeUnmount(stopPolling);
/* Сводка в свёрнутом виде нужна сразу, без раскрытия. */
if (!open.value) void load();

/* ---------------- сводка ---------------- */

const clients = computed<ServerClient[]>(() => st.value?.clients ?? []);
const onlineCount = computed(() => clients.value.filter((c) => c.online).length);

/* Входы. Без поля vless (старый роутер, приложение) есть только AmneziaWG. */
const vl = computed(() => st.value?.vless);
const awgReady = computed(() => !!st.value?.supported && !!st.value?.installed);
const vlessReady = computed(() => !!vl.value?.supported);
const anyReady = computed(() => awgReady.value || vlessReady.value);
const canInstall = computed(() => !!st.value?.can_install || !!vl.value?.can_install);
const awgOn = computed(() => awgReady.value && (st.value?.awg_enabled ?? true));
const vlessOn = computed(() => vlessReady.value && !!vl.value?.enabled);

const summary = computed(() => {
  const s = st.value;
  if (!s) return loadError.value || "Читаю состояние…";
  if (!anyReady.value && canInstall.value) return "Нужно установить компоненты сервера";
  if (!anyReady.value) return s.reason || vl.value?.reason || "Недоступно на этом роутере";
  if (!s.enabled) return "Выключен";
  const n = clients.value.length;
  if (!n) return "Включён, клиентов пока нет";
  return `Клиентов: ${n}, в сети: ${onlineCount.value}`;
});

const chip = computed(() => {
  const s = st.value;
  if (!s || !s.supported) return undefined;
  if (s.enabled && s.running) return onlineCount.value ? `${onlineCount.value} в сети` : "включён";
  if (s.enabled && !s.running) return "не запущен";
  return "выключен";
});
const tone = computed(() => {
  const s = st.value;
  if (s?.enabled && s.running) return "ok" as const;
  if (s?.enabled && !s.running) return "bad" as const;
  return undefined;
});

/* ---------------- установка ---------------- */

const installLog = ref("");
async function install() {
  busy.value = "install";
  installLog.value = "";
  try {
    await services.serverInstall();
    const r = await poll<ApplyLogResponse | null>(() => diag.applyLog(), {
      done: (v) => v?.done === true,
      intervalMs: 2000,
      timeoutMs: 300_000,
      onTick: (v) => {
        if (v && typeof v.log === "string") installLog.value = v.log;
      },
    });
    if (r?.done && Number(r.rc) === 0) toast.ok("Компоненты сервера установлены");
    else toast.error("Установка не удалась — подробности в журнале ниже");
  } catch (e) {
    toast.fromError(e, "Не удалось установить компоненты сервера");
  } finally {
    busy.value = "";
    await load();
  }
}

/* ---------------- сервер ---------------- */

async function toggleServer(on: boolean) {
  busy.value = "server";
  try {
    await services.serverSet({ enabled: on });
    toast.ok(on ? "VPN-сервер включён" : "VPN-сервер выключен");
  } catch (e) {
    toast.fromError(e, "Не удалось переключить сервер");
  } finally {
    busy.value = "";
    await load();
  }
}

async function toggleEntry(kind: "awg" | "vless", on: boolean) {
  busy.value = kind;
  try {
    await services.serverSet(kind === "awg" ? { awg: on } : { vless: on });
    const name = kind === "awg" ? "AmneziaWG" : "VLESS-Reality";
    toast.ok(on ? `Вход ${name} включён` : `Вход ${name} выключен`);
  } catch (e) {
    toast.fromError(e, "Не удалось переключить вход");
  } finally {
    busy.value = "";
    await load();
  }
}

const entriesHint = computed(() => {
  const s = st.value;
  if (!s) return "";
  if (!s.configured) return "Ключи и порты создадутся при первом включении";
  const parts: string[] = [];
  if (awgOn.value) parts.push(`AmneziaWG ${s.port}/udp`);
  if (vlessOn.value && vl.value) parts.push(`VLESS ${vl.value.port}/tcp`);
  if (!parts.length) parts.push("ни один вход не включён");
  return `${parts.join(" · ")} · сеть ${s.net}`;
});

const form = reactive({ endpoint: "", port: "", net: "", dns: "", mtu: "", vport: "", sni: "" });
const showSettings = ref(false);
watch(showSettings, (v) => {
  if (!v || !st.value) return;
  form.endpoint = st.value.endpoint;
  form.vport = String(vl.value?.port || "");
  form.sni = vl.value?.sni ?? "";
  form.port = String(st.value.port || "");
  form.net = st.value.net;
  form.dns = st.value.dns;
  form.mtu = String(st.value.mtu || "");
});

async function saveSettings() {
  const s = st.value;
  if (!s) return;
  const patch: Parameters<typeof services.serverSet>[0] = {};
  if (form.endpoint.trim() !== s.endpoint) patch.endpoint = form.endpoint.trim();
  if (form.dns.trim() !== s.dns) patch.dns = form.dns.trim();
  if (Number(form.port) && Number(form.port) !== s.port) patch.port = Number(form.port);
  if (Number(form.mtu) && Number(form.mtu) !== s.mtu) patch.mtu = Number(form.mtu);
  if (form.net.trim() && form.net.trim() !== s.net) patch.net = form.net.trim();
  if (vl.value) {
    if (Number(form.vport) && Number(form.vport) !== vl.value.port) patch.vport = Number(form.vport);
    if (form.sni.trim() && form.sni.trim() !== vl.value.sni) patch.sni = form.sni.trim();
  }
  if (!Object.keys(patch).length) {
    showSettings.value = false;
    return;
  }
  busy.value = "settings";
  try {
    await services.serverSet(patch);
    toast.ok("Настройки сервера сохранены");
    showSettings.value = false;
    if (patch.port || patch.net || patch.endpoint || patch.dns || patch.mtu || patch.vport || patch.sni) {
      toast.push("Конфиги клиентов изменились — импортируйте их заново", "info", 8000);
    }
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить настройки");
  } finally {
    busy.value = "";
    await load();
  }
}

async function regen() {
  busy.value = "regen";
  try {
    await services.serverRegen();
    toast.push("Маскировка обновлена — всем клиентам нужно импортировать конфиг заново", "info", 8000);
  } catch (e) {
    toast.fromError(e, "Не удалось обновить маскировку");
  } finally {
    confirmRegen.value = false;
    busy.value = "";
    await load();
  }
}

/* ---------------- клиенты ---------------- */

const newName = ref("");
const newMode = ref<ServerClientMode>("full");

const sheet = reactive<{ open: boolean; id: string; tab: "conf" | "vless" | "history" }>({
  open: false,
  id: "",
  tab: "conf",
});
const sheetClient = computed(() => clients.value.find((c) => c.id === sheet.id) ?? null);
function openSheet(c: ServerClient, tab: "conf" | "vless" | "history") {
  sheet.id = c.id;
  sheet.tab = tab;
  sheet.open = true;
}

async function addClient() {
  const name = newName.value.trim();
  if (!name) {
    toast.error("Введите имя клиента — например, «Телефон»");
    return;
  }
  busy.value = "add";
  try {
    const r = await services.serverClientAdd(name, newMode.value);
    newName.value = "";
    await load();
    const c = clients.value.find((x) => x.id === r.id);
    if (c) openSheet(c, awgOn.value ? "conf" : "vless");
  } catch (e) {
    toast.fromError(e, "Не удалось добавить клиента");
  } finally {
    busy.value = "";
  }
}

async function toggleClient(c: ServerClient, on: boolean) {
  busy.value = `c:${c.id}`;
  try {
    await services.serverClientSet(c.id, { enabled: on });
  } catch (e) {
    toast.fromError(e, "Не удалось переключить клиента");
  } finally {
    busy.value = "";
    await load();
  }
}

async function setMode(c: ServerClient, mode: ServerClientMode) {
  if (mode === c.mode) return;
  busy.value = `c:${c.id}`;
  try {
    await services.serverClientSet(c.id, { mode });
    toast.push(`«${c.name}»: импортируйте конфиг заново — режим меняется на стороне клиента`, "info", 8000);
  } catch (e) {
    toast.fromError(e, "Не удалось сменить режим");
  } finally {
    busy.value = "";
    await load();
  }
}

/* ---------------- маршрут клиента (роутер) ---------------- */

/* На роутере маршрут исполняет механизм «Правила → Устройства» по адресам
   клиента, в приложении — правила движка по логину клиента во входе сервера.
   В обоих «Отдельные маршруты» и обход DPI главнее. */
const canRoute = computed(() => true);
watch(
  () => open.value && canRoute.value,
  (v) => {
    if (!v) return;
    void profilesStore.load();
    void profilesStore.loadChains();
  },
  { immediate: true },
);
const routeProfiles = computed(() =>
  profilesStore.items.map((p) => ({ value: `vpn:${p.id}`, label: p.group ? `${p.name} · ${p.group}` : p.name })),
);
const routeChains = computed(() =>
  profilesStore.chainList.map((c) => ({ value: `vpn:${c.id}`, label: `${c.name} (${c.hops.length} шт.)` })),
);
function routeKnown(r: string): boolean {
  return !r || r === "direct" || [...routeProfiles.value, ...routeChains.value].some((o) => o.value === r);
}
function routeLabel(r: string | undefined): string {
  if (!r) return "";
  if (r === "direct") return "мимо VPN";
  const o = [...routeProfiles.value, ...routeChains.value].find((x) => x.value === r);
  return `через «${o?.label ?? r.slice(4)}»`;
}

async function setRoute(c: ServerClient, route: string) {
  if (route === (c.route ?? "")) return;
  busy.value = `c:${c.id}`;
  try {
    await services.serverClientSet(c.id, { route });
    toast.ok(route ? `«${c.name}»: ${routeLabel(route)}` : `«${c.name}»: по правилам роутера`);
  } catch (e) {
    toast.fromError(e, "Не удалось сменить маршрут");
  } finally {
    busy.value = "";
    await load();
  }
}

async function removeClient(c: ServerClient) {
  busy.value = `c:${c.id}`;
  try {
    await services.serverClientDel(c.id);
    toast.ok(`«${c.name}» удалён — его конфиг больше не работает`);
  } catch (e) {
    toast.fromError(e, "Не удалось удалить клиента");
  } finally {
    confirmDelete.value = "";
    busy.value = "";
    await load();
  }
}

function fmtDuration(sec: number): string {
  if (sec < 60) return "меньше минуты";
  const m = Math.round(sec / 60);
  if (m < 60) return `${m} мин`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} ч ${m % 60} мин`;
  return `${Math.floor(h / 24)} д ${h % 24} ч`;
}

function presence(c: ServerClient): string {
  if (!c.enabled) return "отключён";
  if (c.online) {
    const since = c.session_start ? fmtDuration((st.value?.now ?? Date.now() / 1000) - c.session_start) : "";
    return since ? `в сети ${since}` : "в сети";
  }
  return c.last_seen ? `был ${fmtAgo(c.last_seen)}` : "ещё не подключался";
}

function hint(c: ServerClient): string {
  const mode = c.mode === "lan" ? "только дом" : "весь трафик";
  const route = c.route ? ` ${routeLabel(c.route)}` : "";
  return `${c.ip} · ${mode}${route} · ${presence(c)}`;
}
</script>

<template>
  <ServicePanel
    id="svc-server"
    v-model:open="open"
    title="Свой VPN-сервер"
    :summary="summary"
    :chip="chip"
    :tone="tone"
  >
    <p v-if="status.isClient" class="lead">
      Подключайте телефон или другой компьютер к этому устройству по AmneziaWG
      или VLESS-Reality — протоколам, которые маскируются от блокировок. Их трафик идёт по тем же
      правилам Detour, что и трафик этого компьютера. Сервер работает, пока
      Detour здесь подключён.
    </p>
    <p v-else class="lead">
      Подключайтесь к дому из любой сети по AmneziaWG или VLESS-Reality —
      протоколам, которые маскируются от блокировок. Клиенты получают маршруты
      роутера: заблокированное идёт через ваш VPN, остальное — напрямую,
      домашние устройства доступны.
    </p>

    <p v-if="loadError && !st" class="note bad">{{ loadError }}</p>

    <template v-if="st">
      <p v-if="st.note" class="note">{{ st.note }}</p>
      <p v-if="!anyReady && !canInstall" class="note warn">
        {{ st.reason || vl?.reason || "На этом роутере сервер недоступен." }}
      </p>

      <template v-else-if="!anyReady">
        <p class="note">
          Для сервера нужны пакеты из фида прошивки:
          <template v-if="st.can_install">AmneziaWG (модуль ядра, а если его нет — userspace-сборка из фида Detour)</template><template
            v-if="st.can_install && vl?.can_install"
          > и </template><template v-if="vl?.can_install">ip-full (для входа VLESS)</template>.
          Установка займёт около минуты.
        </p>
        <div class="actions">
          <UiButton variant="primary" :busy="busy === 'install'" @click="install">
            Установить
          </UiButton>
        </div>
        <pre v-if="installLog" class="log">{{ installLog }}</pre>
      </template>

      <template v-else>
        <SwitchToggle
          :model-value="st.enabled"
          label="Сервер включён"
          :hint="entriesHint"
          :busy="busy === 'server'"
          @update:model-value="toggleServer"
        />

        <div v-if="vl && st.configured" class="entries">
          <SwitchToggle
            :model-value="awgOn"
            label="AmneziaWG"
            :hint="awgReady ? `UDP ${st.port} · приложения AmneziaVPN, AmneziaWG` : st.can_install ? 'Модуль ядра не установлен' : st.reason || 'Недоступен на этом роутере'"
            :disabled="!awgReady"
            :busy="busy === 'awg'"
            @update:model-value="toggleEntry('awg', $event)"
          />
          <SwitchToggle
            :model-value="vlessOn"
            label="VLESS-Reality"
            :hint="vlessReady ? `TCP ${vl.port} · маскируется под ${vl.sni} · v2rayNG, Hiddify, Streisand` : vl.reason"
            :disabled="!vlessReady"
            :busy="busy === 'vless'"
            @update:model-value="toggleEntry('vless', $event)"
          />
          <div v-if="(!awgReady && st.can_install) || (!vlessReady && vl.can_install)" class="actions">
            <UiButton :busy="busy === 'install'" @click="install">Установить недостающее</UiButton>
          </div>
          <pre v-if="installLog" class="log">{{ installLog }}</pre>
        </div>

        <p v-if="st.enabled && !st.running && status.isClient && !st.engine_running" class="note warn">
          Detour не подключён — сервер поднимется вместе с подключением.
        </p>
        <p v-else-if="st.enabled && !st.running" class="note bad">
          Сервер включён, но не поднялся. Подробности — в журнале<template v-if="status.isClient">
          службы (awgsrv.log)</template><template v-else-if="st.platform === 'keenetic'"> /opt/var/log/detour-server.log</template><template v-else> /var/log/detour-server.log</template>.
        </p>
        <p v-if="status.isClient && st.wan_private && !st.endpoint" class="note warn">
          Компьютер в локальной сети ({{ st.wan_ip }}). Из этой же сети к нему
          подключатся сразу; из интернета — только если на роутере проброшен
          <template v-if="awgOn">UDP-порт {{ st.port }}</template><template v-if="awgOn && vlessOn"> и </template><template
            v-if="vlessOn && vl"
          >TCP-порт {{ vl.port }}</template> на {{ st.wan_ip }}, а в настройках сервера указан
          внешний адрес или домен роутера.
        </p>
        <p v-else-if="st.wan_private && !st.endpoint" class="note warn">
          Внешний адрес роутера {{ st.wan_ip }} — серый (провайдерский NAT): из
          интернета до сервера не достучаться. Нужен белый IP или проброс
          <template v-if="awgOn">порта {{ st.port }}/UDP</template><template v-if="awgOn && vlessOn"> и </template><template
            v-if="vlessOn && vl"
          >порта {{ vl.port }}/TCP</template> на стороне провайдера.
        </p>
        <p v-if="st.enabled" class="note faint">
          Клиенты подключаются к {{ st.endpoint_effective || "—" }}<template v-if="!vl">:{{ st.port }}</template><template
            v-if="!st.endpoint"
          > ({{ status.isClient ? "адрес компьютера в локальной сети" : "внешний адрес роутера; если он меняется — укажите домен в настройках" }})</template>.
        </p>

        <template v-if="st.configured">
          <h3 class="sub">Клиенты</h3>
          <p v-if="!clients.length" class="note">
            Добавьте первого клиента — телефон, ноутбук. У каждого свой ключ: его
            можно отключить или удалить, не трогая остальных.
          </p>

          <p v-if="clients.length && st.awg_traffic === false" class="note faint">
            Объём и скорость по клиентам здесь не видны: KeeneticOS ускоряет трафик встроенного
            WireGuard мимо всех счётчиков. Онлайн, адрес и время подключения — точные.
          </p>

          <div v-for="c in clients" :key="c.id" class="client" :class="{ live: c.online }">
            <SwitchToggle
              :model-value="c.enabled"
              :label="c.name"
              :hint="hint(c)"
              :busy="busy === `c:${c.id}`"
              @update:model-value="toggleClient(c, $event)"
            />
            <div v-if="c.online" class="speed">
              <template v-if="st.awg_traffic !== false">
                <span>↓ {{ fmtBitrate(speed[c.id]?.down ?? 0) }}</span>
                <span>↑ {{ fmtBitrate(speed[c.id]?.up ?? 0) }}</span>
              </template>
              <span v-if="c.remote" class="mono">{{ c.remote.replace(/:\d+$/, "") }}</span>
            </div>
            <dl v-if="st.awg_traffic !== false" class="traffic">
              <div v-if="c.online">
                <dt>сессия</dt>
                <dd>↓ {{ fmtBytes(c.session_tx) }} · ↑ {{ fmtBytes(c.session_rx) }}</dd>
              </div>
              <div>
                <dt>сегодня</dt>
                <dd>↓ {{ fmtBytes(c.day_tx) }} · ↑ {{ fmtBytes(c.day_rx) }}</dd>
              </div>
              <div>
                <dt>месяц</dt>
                <dd>↓ {{ fmtBytes(c.month_tx) }} · ↑ {{ fmtBytes(c.month_rx) }}</dd>
              </div>
              <div>
                <dt>всего</dt>
                <dd>↓ {{ fmtBytes(c.total_tx) }} · ↑ {{ fmtBytes(c.total_rx) }}</dd>
              </div>
            </dl>
            <label v-if="canRoute && c.mode !== 'lan'" class="route">
              <span>Маршрут</span>
              <select
                :value="c.route ?? ''"
                :disabled="busy === `c:${c.id}`"
                aria-label="Куда направить трафик клиента"
                @change="setRoute(c, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">По правилам роутера</option>
                <option value="direct">Мимо VPN — напрямую</option>
                <optgroup v-if="routeProfiles.length" label="Через профиль">
                  <option v-for="o in routeProfiles" :key="o.value" :value="o.value">{{ o.label }}</option>
                </optgroup>
                <optgroup v-if="routeChains.length" label="Через цепочку">
                  <option v-for="o in routeChains" :key="o.value" :value="o.value">{{ o.label }}</option>
                </optgroup>
                <option v-if="!routeKnown(c.route ?? '')" :value="c.route">
                  {{ (c.route ?? "").slice(4) }} (не найден — идёт через основной VPN)
                </option>
              </select>
            </label>
            <div class="row-actions">
              <UiButton @click="openSheet(c, vlessOn && !awgOn ? 'vless' : 'conf')">Подключение и QR</UiButton>
              <UiButton @click="openSheet(c, 'history')">История</UiButton>
              <UiButton
                :busy="busy === `c:${c.id}`"
                @click="setMode(c, c.mode === 'full' ? 'lan' : 'full')"
              >
                {{ c.mode === "full" ? "Только дом" : "Весь трафик" }}
              </UiButton>
              <template v-if="confirmDelete === c.id">
                <UiButton variant="danger" :busy="busy === `c:${c.id}`" @click="removeClient(c)">
                  Точно удалить
                </UiButton>
                <UiButton @click="confirmDelete = ''">Отмена</UiButton>
              </template>
              <UiButton v-else variant="danger" @click="confirmDelete = c.id">Удалить</UiButton>
            </div>
          </div>

          <div class="add">
            <FormField label="Новый клиент">
              <input
                v-model="newName"
                type="text"
                maxlength="48"
                placeholder="Телефон"
                @keydown.enter.prevent="addClient"
              />
            </FormField>
            <SegmentedControl v-model="newMode" label="Что пускать через сервер" :options="MODE_OPTIONS" />
            <div class="actions">
              <UiButton variant="primary" :busy="busy === 'add'" @click="addClient">
                Добавить клиента
              </UiButton>
            </div>
          </div>

          <details class="settings" :open="showSettings" @toggle="showSettings = ($event.target as HTMLDetailsElement).open">
            <summary>Настройки сервера</summary>
            <div class="settings-body">
              <FormField
                label="Адрес для клиентов"
                :hint="st.panel_domain ? `Пусто — внешний IP ${st.wan_ip}. Можно указать домен, например ${st.panel_domain}` : `Пусто — внешний IP ${st.wan_ip}`"
              >
                <input v-model="form.endpoint" type="text" placeholder="авто" />
              </FormField>
              <FormField v-if="awgReady" label="UDP-порт AmneziaWG">
                <input v-model="form.port" type="number" min="1024" max="65535" />
              </FormField>
              <template v-if="vl && vlessReady">
                <FormField label="TCP-порт VLESS" hint="443 прячется лучше всего, если на роутере он свободен">
                  <input v-model="form.vport" type="number" min="1" max="65535" />
                </FormField>
                <FormField
                  label="Сайт-маска VLESS-Reality"
                  hint="Зарубежный сайт с TLS 1.3, доступный из вашей сети; сервер выдаёт себя за него"
                >
                  <input v-model="form.sni" type="text" placeholder="www.microsoft.com" />
                </FormField>
              </template>
              <FormField label="Подсеть клиентов" hint="Частная /24, не пересекающаяся с домашней сетью">
                <input v-model="form.net" type="text" />
              </FormField>
              <FormField label="DNS для клиентов" :hint="`Пусто — роутер (${st.server_ip}); тогда работают списки доменов`">
                <input v-model="form.dns" type="text" placeholder="авто" />
              </FormField>
              <FormField label="MTU">
                <input v-model="form.mtu" type="number" min="1200" max="1500" />
              </FormField>
              <div class="actions">
                <UiButton variant="primary" :busy="busy === 'settings'" @click="saveSettings">
                  Сохранить
                </UiButton>
              </div>
              <p class="note faint">
                Смена адреса, порта, подсети, DNS, MTU или сайта-маски меняет
                конфиги и ссылки клиентов — после неё их нужно импортировать заново.
              </p>
              <div v-if="awgReady" class="actions">
                <template v-if="confirmRegen">
                  <UiButton variant="danger" :busy="busy === 'regen'" @click="regen">
                    Да, сменить — все переподключат
                  </UiButton>
                  <UiButton @click="confirmRegen = false">Отмена</UiButton>
                </template>
                <UiButton v-else @click="confirmRegen = true">Сменить параметры маскировки</UiButton>
              </div>
            </div>
          </details>
        </template>
      </template>
    </template>
  </ServicePanel>

  <ServerClientSheet
    :open="sheet.open"
    :client="sheetClient"
    :tab="sheet.tab"
    :awg="awgOn"
    :vless="vlessOn"
    :traffic="st?.awg_traffic"
    @close="sheet.open = false"
  />
</template>

<style scoped>
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
.log {
  font-family: var(--mono);
  font-size: 11.5px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 220px;
  overflow: auto;
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px;
}
.entries {
  display: grid;
  gap: 8px;
  border-left: 2px solid var(--line);
  padding-left: 12px;
  margin-left: 4px;
}
.sub {
  font-size: 13px;
  font-weight: 600;
  margin: 4px 0 0;
}
.client {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 11px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  background: var(--panel-2);
}
.client.live {
  border-color: color-mix(in srgb, var(--ok) 45%, var(--line));
}
.client :deep(.lbl) {
  font-weight: 600;
}
.client :deep(.text small) {
  font-family: var(--mono);
  overflow-wrap: anywhere;
}
.speed {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
  font-size: 13px;
  color: var(--ok);
  font-variant-numeric: tabular-nums;
}
.mono {
  font-family: var(--mono);
  font-size: 11.5px;
  color: var(--faint);
}
.traffic {
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 4px 14px;
  font-size: 12px;
}
.traffic div {
  display: flex;
  gap: 8px;
  min-width: 0;
}
.traffic dt {
  color: var(--faint);
  min-width: 52px;
}
.traffic dd {
  margin: 0;
  color: var(--dim);
  font-variant-numeric: tabular-nums;
}
.route {
  display: grid;
  gap: 5px;
  font-size: 12.5px;
  color: var(--dim);
  min-width: 0;
}
.route select {
  width: 100%;
  min-width: 0;
  text-overflow: ellipsis;
  font-size: 16px;
  min-height: 44px;
  padding: 0 10px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text);
  max-width: 100%;
}
.row-actions {
  display: flex;
  gap: 7px;
  flex-wrap: wrap;
}
.add {
  display: grid;
  gap: 10px;
  border: 1px dashed var(--line-2);
  border-radius: var(--radius-sm);
  padding: 11px 12px;
}
.settings summary {
  cursor: pointer;
  font-size: 13px;
  color: var(--dim);
  padding: 4px 0;
}
.settings-body {
  display: grid;
  gap: 10px;
  margin-top: 8px;
}
</style>
