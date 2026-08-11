<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import FlowBoard from "@/components/FlowBoard.vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import ProfilePicker from "@/components/ProfilePicker.vue";
import BypassTile from "@/components/overview/BypassTile.vue";
import UplinksTile from "@/components/overview/UplinksTile.vue";
import { diag, overview } from "@/api";
import type { LanClient, TrafficLanes, UdpVpnMode } from "@/api";
import DrawerSheet from "@/components/DrawerSheet.vue";
import { useStatusStore } from "@/stores/status";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";
import { useCommandStore } from "@/stores/commands";
import { fmtAgo, fmtBitrate, fmtSpeedKbps, isSet } from "@/lib/format";

const status = useStatusStore();
const profiles = useProfilesStore();
const toast = useToastStore();
const commands = useCommandStore();

const pickerOpen = ref(false);
const busy = ref("");
const lanes = ref<(TrafficLanes & { warming?: boolean; span?: number; bytes?: Record<string, number> }) | null>(null);
const clients = ref(0);
/* Список, а не только счётчик: «16 устройств» не отвечает на вопрос «что это за
   шестнадцать» — а он возникает первым, когда трафик идёт не туда. */
const clientList = ref<LanClient[]>([]);
const clientsOpen = ref(false);
let trafficTimer: number | undefined;

const sb = computed(() => status.data?.singbox);
const zp = computed(() => status.data?.zapret);

const profileLabel = computed(() => {
  const chain = status.activeChain;
  if (chain.length > 1) return chain.join(" → ");
  return status.activeProfile || "профиль не выбран";
});

/* На схеме потока показываем РАБОТАЮЩИЙ движок, а не выбранный: при zapret2
   служба tpws штатно остановлена, и подпись по status.zapret врала «выключен».
   Выбранный-но-остановленный — отдельный случай, его тоже надо называть. */
const bypassLabel = computed(() => {
  const running = status.bypassRunning;
  if (running !== "none") return running;
  const m = status.bypass?.mode ?? "off";
  return m === "off" ? "выключен" : `${m} (остановлен)`;
});

const uptime = computed(() => status.data?.system?.uptime ?? "");
const sys = computed(() => status.data?.system);
const wan = computed(() => status.data?.wan_link);

/* CPU бэкенд отдаёт строкой и на первом опросе честно ставит «?» — дельту
   /proc/stat не с чем сравнивать. Прочерк тут правильнее нуля. */
const cpuText = computed(() => {
  const c = sys.value?.cpu;
  return c && c !== "?" ? `${c} %` : "—";
});

/* MPTCP на этом ядре сломан: при включённом sing-box не может принимать
   соединения, они виснут в SYN_RECEIVED. Значение должно быть 0 всегда, поэтому
   любое другое — это не «информация», а поломка, которую надо показать. */
const mptcpBroken = computed(() => {
  const m = sys.value?.mptcp;
  return m !== undefined && String(m) !== "0";
});

/* Предупреждение о деградации WAN-порта прячется до тех пор, пока диагноз не
   изменится: ключ — сам текст диагноза. Скрыть навсегда нельзя намеренно, иначе
   новая проблема (уже другая) молча унаследует старое «не показывать». */
const WAN_DISMISS_KEY = "detour:wan-dismissed";
const wanDismissed = ref(localStorage.getItem(WAN_DISMISS_KEY) ?? "");

/* Плоский degraded описывает только тот канал, который держит маршрут по
   умолчанию. С двумя провайдерами просевший РЕЗЕРВ через него не виден вовсе —
   а узнать про него надо заранее, а не в момент, когда он понадобился. Поэтому
   сначала ищем просадку по всему массиву и только потом откатываемся на плоские
   поля (их отдаёт и старый detour-wan-link, панель обновляется отдельно). */
const wanDegradedText = computed(() => {
  const w = wan.value;
  if (!w) return "";
  const bad = w.uplinks?.find((u) => u.degraded);
  if (bad) return bad.diagnosis || `Канал «${bad.label}» работает медленнее, чем может`;
  if (w.degraded) return w.diagnosis || "WAN-порт работает медленнее, чем может";
  return "";
});

const wanAlert = computed(() => {
  const text = wanDegradedText.value;
  if (!text) return null;
  return wanDismissed.value === text ? null : { text, advice: wan.value?.advice };
});

function dismissWan() {
  const t = wanDegradedText.value;
  if (!t) return;
  wanDismissed.value = t;
  localStorage.setItem(WAN_DISMISS_KEY, t);
}

/* При нескольких каналах строка «WAN … Мбит/с» в «Системе» описывала бы только
   один из них рядом с плиткой, где расписаны все — уводим её, чтобы не было двух
   разных ответов на один вопрос. */
const showSysWan = computed(
  () =>
    wan.value?.supported !== false &&
    !!wan.value?.speed_mbps &&
    (wan.value?.uplinks?.length ?? 0) < 2,
);

/* В режиме all-except список доменов не используется: в туннель идёт всё,
   кроме белого списка. Показывать там «0 доменов» — прямая дезинформация. */
const allExcept = computed(() => sb.value?.routing_mode === "all-except");

const scopeText = computed(() => {
  if (sb.value?.allvpn) return "весь TCP в туннеле, кроме белого списка";
  if (allExcept.value) return "в туннеле всё, кроме белого списка";
  return `в туннеле только список — ${sb.value?.domains ?? 0} доменов`;
});

const vpnScopeShort = computed(() =>
  allExcept.value ? "всё, кроме белого списка" : `${sb.value?.domains ?? 0} доменов`,
);

/* Проверка и замер скорости активного профиля. Данные приходят тем же
   health_status, что и для списка, но на «Обзоре» их не было вовсе — про рабочий
   ли сейчас VPN и с какой скоростью приходилось идти в другой раздел. */
const ACTIVE_STALE_S = 180;

const activeHealth = computed(() => {
  const id = status.activeProfile;
  if (!id || status.activeChain.length > 1) return null;
  const h = profiles.health[id];
  if (!h || h.ok === undefined) return null;
  const age = h.ts ? Math.max(0, Math.floor(Date.now() / 1000) - h.ts) : 0;
  return {
    ok: h.ok,
    speed: fmtSpeedKbps(h.dl),
    when: fmtAgo(h.ts),
    /* Для активного профиля проверка идёт раз в 30–60 с. Если результат старше
       трёх минут, он уже ничего не говорит о «сейчас» — и молча показывать его
       как текущий нельзя. */
    stale: age > ACTIVE_STALE_S,
  };
});

/* Плашка живёт сутки: событие важно ровно как объяснение «почему активен не тот
   профиль», а через день это уже история. */
const SWITCH_SHOW_S = 86_400;

const autoSwitch = computed(() => {
  const sw = profiles.healthSwitch;
  if (!sw?.to || !sw.ts) return null;
  if (Math.floor(Date.now() / 1000) - sw.ts > SWITCH_SHOW_S) return null;
  const nameOf = (id?: string) =>
    (id && profiles.rows.find((r) => r.id === id)?.name) || id || "?";
  return { from: nameOf(sw.from), to: nameOf(sw.to), when: fmtAgo(sw.ts) };
});

const healthCounts = computed(() => {
  const rows = profiles.rows;
  const total = rows.length;
  const ok = rows.filter((r) => r.state === "ok").length;
  const slow = rows.filter((r) => r.state === "slow").length;
  const dead = rows.filter((r) => r.state === "dead").length;
  return { total, ok, slow, dead };
});

const udpMode = computed({
  get: () => (status.udp?.mode ?? "off") as UdpVpnMode,
  set: (m: UdpVpnMode) => void setUdp(m),
});

/* Заворачивать UDP в туннель умеет только OpenWrt: на Keenetic нет TPROXY.
   Раньше сегменты оставались активными, клик уходил в CGI и возвращал ошибку. */
const udpUnsupportedHint = computed(() =>
  status.udpVpnSupported ? undefined : "Нужен TPROXY — на этой платформе его нет",
);

const allvpn = computed({
  get: () => sb.value?.allvpn === true,
  set: (v: boolean) => void setAllvpn(v),
});

async function setAllvpn(on: boolean) {
  busy.value = "allvpn";
  try {
    if (on) await overview.allvpnOn();
    else await overview.allvpnOff();
    toast.ok(on ? "Весь TCP идёт в туннель" : "В туннель идёт только список");
  } catch (e) {
    toast.fromError(e, "Не удалось переключить режим");
  } finally {
    busy.value = "";
    void status.refresh(true);
  }
}

async function setUdp(mode: UdpVpnMode) {
  busy.value = "udp";
  try {
    await overview.udpVpnSet(mode);
    toast.ok(`UDP через VPN: ${mode === "off" ? "выключено" : mode}`);
  } catch (e) {
    toast.fromError(e, "Не удалось изменить режим UDP");
  } finally {
    busy.value = "";
    await status.refreshExtras();
  }
}

async function restart() {
  busy.value = "restart";
  try {
    await diag.singboxRestart();
    toast.ok("sing-box перезапущен");
  } catch (e) {
    toast.fromError(e, "Перезапуск не удался");
  } finally {
    busy.value = "";
    void status.refresh(true);
  }
}

async function checkAll() {
  try {
    await diag.healthCheckAll();
    toast.info("Проверка запущена — результаты появятся по мере готовности");
  } catch (e) {
    toast.fromError(e, "Не удалось запустить проверку");
  }
}

/* Счётчики отдают дельту с прошлого чтения, поэтому первый ответ всегда
   пустой («прогрев»), а дальше опрашиваем ровным шагом — иначе доли скачут
   вместе со случайной длиной интервала. */
async function loadTraffic() {
  try {
    const t = await overview.traffic();
    lanes.value = t && t.supported !== false ? t : null;
  } catch {
    lanes.value = null;
  }
}

const hasShares = computed(() => !!lanes.value && lanes.value.warming !== true);

const speedText = computed(() => {
  const b = lanes.value?.bytes?.total ?? 0;
  const span = lanes.value?.span ?? 0;
  if (!b || !span) return "";
  return fmtBitrate(b / span);
});

async function loadExtras() {
  const c = await overview.lanClients().catch(() => null);
  if (c) {
    clientList.value = c.clients ?? [];
    clients.value = clientList.value.length;
  }
  await loadTraffic();
  trafficTimer = window.setInterval(() => {
    if (document.visibilityState === "visible") void loadTraffic();
  }, 10_000);
}

let unregister: (() => void) | undefined;

onMounted(async () => {
  unregister = commands.register([
    {
      id: "ov:switch",
      title: "Сменить VPN-профиль",
      group: "подключение",
      keywords: "профиль страна переключить",
      run: () => {
        pickerOpen.value = true;
      },
    },
    {
      id: "ov:restart",
      title: "Перезапустить sing-box",
      group: "подключение",
      run: () => void restart(),
    },
    {
      id: "ov:checkall",
      title: "Проверить все профили",
      group: "подключение",
      keywords: "здоровье health",
      run: () => void checkAll(),
    },
  ]);
  await profiles.load();
  void profiles.loadProbes();
  void loadExtras();
});

onBeforeUnmount(() => {
  unregister?.();
  if (trafficTimer) window.clearInterval(trafficTimer);
});
</script>

<template>
  <!-- Деградация WAN-порта: гигабитный линк, поднявшийся на 100 Мбит/с, объясняет
       «интернет медленный» лучше любых замеров скорости, поэтому это баннер, а не
       строчка в плитке. -->
  <div v-if="wanAlert" class="wanwarn">
    <div>
      <b>{{ wanAlert.text }}</b>
      <p v-if="wanAlert.advice">{{ wanAlert.advice }}</p>
    </div>
    <UiButton @click="dismissWan">Скрыть</UiButton>
  </div>

  <!-- Роутер мог сменить профиль сам, пока панель была закрыта. Без этой строки
       активным оказывается «не тот» профиль без всяких объяснений. -->
  <div v-if="autoSwitch" class="wanwarn">
    <div>
      <b>Авто-переключение: {{ autoSwitch.from }} → {{ autoSwitch.to }}</b>
      <p>
        Прежний профиль перестал проходить проверку{{
          autoSwitch.when ? ` · ${autoSwitch.when}` : ""
        }}.
      </p>
    </div>
  </div>

  <div v-if="mptcpBroken" class="wanwarn bad">
    <div>
      <b>MPTCP включён (net.mptcp.enabled = {{ sys?.mptcp }})</b>
      <p>
        На этом ядре MPTCP сломан: sing-box не сможет принимать соединения, они
        зависнут. Значение должно быть 0.
      </p>
    </div>
  </div>

  <FlowBoard
    :direct="lanes?.direct ?? 0"
    :vpn="lanes?.vpn ?? 0"
    :bypass="lanes?.bypass ?? 0"
    :exact="lanes?.exact ?? false"
    :has-shares="hasShares"
    :speed="speedText"
    :clients="clients"
    :profile-label="profileLabel"
    :bypass-label="bypassLabel"
    :vpn-scope="vpnScopeShort"
    :bypass-scope="`${zp?.domains ?? 0} доменов`"
    :connections="sb?.ipset_count"
    :external-ip="sb?.external_ip"
    :vpn-up="status.singboxRunning"
  />

  <div class="tiles">
    <TileCard title="Активное подключение" span>
      <p class="big">
        {{ profileLabel }}
        <small v-if="sb?.active_type">{{ sb.active_type }}</small>
      </p>
      <p class="meta">
        <span v-if="status.singboxRunning">Работает{{ uptime ? ` · ${uptime}` : "" }}</span>
        <span v-else class="bad">sing-box остановлен</span>
        <span v-if="isSet(sb?.pid)">PID {{ sb?.pid }}<template v-if="isSet(sb?.port)"> · порт {{ sb?.port }}</template></span>
        <span v-if="sb?.external_ip" class="mono">{{ sb.external_ip }}</span>
      </p>
      <p v-if="activeHealth" class="meta">
        <span :class="activeHealth.ok ? 'ok' : 'bad'">
          {{ activeHealth.ok ? "проверка проходит" : "проверка не проходит" }}
        </span>
        <span v-if="activeHealth.speed">↓ {{ activeHealth.speed }}</span>
        <span v-if="activeHealth.stale" class="warn">
          данные устарели{{ activeHealth.when ? ` · ${activeHealth.when}` : "" }}
        </span>
        <span v-else-if="activeHealth.when">{{ activeHealth.when }}</span>
      </p>
      <template #actions>
        <UiButton variant="primary" @click="pickerOpen = true">Сменить VPN</UiButton>
        <UiButton :busy="busy === 'restart'" @click="restart">Перезапустить</UiButton>
        <UiButton @click="checkAll">Проверить все</UiButton>
      </template>
    </TileCard>

    <TileCard title="Область действия">
      <SwitchToggle
        v-model="allvpn"
        label="Все через VPN"
        :busy="busy === 'allvpn'"
        :hint="scopeText"
      />
      <!-- Оба списка, от которых зависит эта настройка, правятся в «Правилах»:
           без ссылок карточка сообщает про «белый список» и не говорит, где он. -->
      <p class="hint links">
        <RouterLink :to="{ path: '/rules', query: { focus: 'whitelist' } }">
          Белый список
        </RouterLink>
        <RouterLink v-if="!allvpn && !allExcept" :to="{ path: '/rules', query: { focus: 'domains' } }">
          Сайты через VPN
        </RouterLink>
        <RouterLink :to="{ path: '/rules', query: { focus: 'mode' } }">Режим туннеля</RouterLink>
      </p>
      <div class="udp">
        <p class="lbl">UDP через VPN</p>
        <SegmentedControl
          v-model="udpMode"
          label="UDP через VPN"
          :busy="busy === 'udp'"
          :options="[
            { value: 'off', label: 'Выкл', disabled: !status.udpVpnSupported },
            {
              value: 'list',
              label: 'По списку',
              disabled: !status.udpVpnSupported,
              hint: udpUnsupportedHint,
            },
            {
              value: 'all',
              label: 'Весь UDP',
              disabled: !status.udpVpnSupported,
              hint: udpUnsupportedHint,
            },
          ]"
        />
        <p v-if="!status.udpVpnSupported" class="hint">
          Недоступно на этой платформе: нужен TPROXY.
        </p>
        <!-- Режим здесь, а список — в «Правилах». Без этой ссылки режим «по
             списку» оказывался тупиком: включить можно, а чем наполнять — нет. -->
        <p v-else class="hint">
          <template v-if="udpMode === 'list'">
            <RouterLink :to="{ path: '/rules', query: { focus: 'udp' } }">
              Список адресов — в «Правилах»
            </RouterLink>
          </template>
          <template v-else-if="udpMode === 'all'">
            Весь UDP, кроме белого списка и DNS. QUIC пойдёт в туннель.
          </template>
          <template v-else>Игры и голос по UDP идут напрямую.</template>
        </p>
      </div>
    </TileCard>

    <BypassTile />

    <TileCard title="Здоровье профилей">
      <p class="big num">
        {{ healthCounts.ok }}<small>из {{ healthCounts.total }} проходят проверку</small>
      </p>
      <div class="bar" aria-hidden="true">
        <i
          :style="{
            width: healthCounts.total
              ? `${(healthCounts.ok / healthCounts.total) * 100}%`
              : '0%',
          }"
        ></i>
      </div>
      <p class="meta">
        <span v-if="healthCounts.slow">{{ healthCounts.slow }} медленных</span>
        <span v-if="healthCounts.dead">{{ healthCounts.dead }} не отвечают</span>
      </p>
      <template #actions>
        <UiButton @click="$router.push('/profiles')">Список профилей</UiButton>
        <UiButton @click="$router.push({ path: '/journal', query: { focus: 'health' } })">
          Настроить проверку
        </UiButton>
      </template>
    </TileCard>

    <UplinksTile />

    <TileCard title="Система">
      <p class="meta col">
        <span>
          Процессор {{ cpuText
          }}<template v-if="sys?.cpu_cores"> · {{ sys.cpu_cores }} ядра</template>
        </span>
        <span>Память {{ sys?.memory ?? "—" }}</span>
        <span>Свободно на диске {{ sys?.disk_free ?? "—" }}</span>
        <!-- Скорость WAN показываем всегда, а не только при деградации: иначе
             скрытый баннер означал бы, что состояние линка вообще негде увидеть.
             При нескольких каналах это берёт на себя плитка «Каналы в интернет». -->
        <span v-if="showSysWan">
          WAN {{ wan?.speed_mbps }} Мбит/с<template v-if="wan?.duplex">
            · {{ wan.duplex }}</template
          >
        </span>
      </p>
      <template #actions>
        <UiButton :disabled="!clients" @click="clientsOpen = true">
          Устройства в сети{{ clients ? ` · ${clients}` : "" }}
        </UiButton>
      </template>
    </TileCard>

    <TileCard title="Версии">
      <p class="meta col">
        <span>Панель {{ status.data?.version ?? "—" }}</span>
        <span>sing-box {{ status.data?.binaries?.singbox_version ?? "—" }}</span>
        <span>tpws {{ status.data?.binaries?.tpws_version ?? "—" }}</span>
      </p>
      <template #actions>
        <UiButton @click="$router.push({ path: '/journal', query: { focus: 'updates' } })">
          Обновления
        </UiButton>
      </template>
    </TileCard>
  </div>

  <ProfilePicker :open="pickerOpen" @close="pickerOpen = false" />

  <DrawerSheet
    :open="clientsOpen"
    title="Устройства в локальной сети"
    @close="clientsOpen = false"
  >
    <p class="hint">
      Из аренд DHCP и таблицы ARP. Имя показывает само устройство, поэтому оно
      бывает пустым или неузнаваемым.
    </p>
    <ul class="devs">
      <li v-for="c in clientList" :key="c.ip">
        <span class="mono">{{ c.ip }}</span>
        <b>{{ c.host || "без имени" }}</b>
        <span v-if="c.mac" class="mono faint">{{ c.mac }}</span>
      </li>
    </ul>
  </DrawerSheet>
</template>

<style scoped>
.tiles {
  display: grid;
  gap: 14px;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
}
@media (min-width: 1100px) {
  .tiles {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}
.big {
  font-size: 24px;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.2;
  overflow-wrap: anywhere;
}
.big small {
  font-size: 13px;
  font-weight: 500;
  color: var(--dim);
  letter-spacing: 0;
  margin-left: 8px;
}
.meta {
  font-size: 13px;
  color: var(--dim);
  display: flex;
  gap: 4px 14px;
  flex-wrap: wrap;
}
.meta.col {
  flex-direction: column;
  gap: 4px;
}
.meta .bad {
  color: var(--bad);
}
.meta .ok {
  color: var(--ok);
}
.meta .warn {
  color: var(--warn);
}
.udp {
  display: flex;
  flex-direction: column;
  gap: 7px;
}
.lbl {
  font-size: 14px;
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
.hint a {
  color: var(--accent);
}
.links {
  display: flex;
  gap: 4px 14px;
  flex-wrap: wrap;
}
.devs {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.devs li {
  display: grid;
  /* Адрес и MAC — фиксированной ширины, имя тянется: у него длина непредсказуема,
     а колонки должны читаться как столбцы, а не как рваный текст. */
  grid-template-columns: 118px minmax(0, 1fr);
  gap: 2px 12px;
  align-items: baseline;
  padding: 8px 0;
  border-bottom: 1px solid var(--line);
  font-size: 13px;
}
.devs b {
  overflow-wrap: anywhere;
}
.devs .faint {
  grid-column: 2;
  color: var(--faint);
  font-size: 11.5px;
}
.wanwarn {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
  background: color-mix(in srgb, var(--warn) 10%, transparent);
  border-radius: var(--radius-sm);
  padding: 11px 14px;
  margin-bottom: 14px;
  font-size: 13.5px;
}
.wanwarn.bad {
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
  background: color-mix(in srgb, var(--bad) 10%, transparent);
}
.wanwarn div {
  min-width: 0;
  flex: 1 1 260px;
}
.wanwarn p {
  color: var(--dim);
  font-size: 12.5px;
  margin-top: 3px;
}
.bar {
  height: 5px;
  border-radius: 3px;
  background: var(--panel-2);
  overflow: hidden;
  border: 1px solid var(--line);
}
.bar i {
  display: block;
  height: 100%;
  background: var(--accent);
  transition: width 0.5s ease;
}
</style>
