<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import FlowBoard from "@/components/FlowBoard.vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import ProfilePicker from "@/components/ProfilePicker.vue";
import { diag, overview } from "@/api";
import type { BypassMode, TrafficLanes, UdpVpnMode } from "@/api";
import { useStatusStore } from "@/stores/status";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";
import { useCommandStore } from "@/stores/commands";
import { fmtBitrate, isSet } from "@/lib/format";

const status = useStatusStore();
const profiles = useProfilesStore();
const toast = useToastStore();
const commands = useCommandStore();

const pickerOpen = ref(false);
const busy = ref("");
const lanes = ref<(TrafficLanes & { warming?: boolean; span?: number; bytes?: Record<string, number> }) | null>(null);
const clients = ref(0);
let trafficTimer: number | undefined;

const sb = computed(() => status.data?.singbox);
const zp = computed(() => status.data?.zapret);

const profileLabel = computed(() => {
  const chain = status.activeChain;
  if (chain.length > 1) return chain.join(" → ");
  return status.activeProfile || "профиль не выбран";
});

const bypassLabel = computed(() => {
  const m = status.bypass?.mode ?? (zp.value?.running ? "zapret" : "off");
  return m === "off" ? "выключен" : m;
});

const uptime = computed(() => status.data?.system?.uptime ?? "");

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

const healthCounts = computed(() => {
  const rows = profiles.rows;
  const total = rows.length;
  const ok = rows.filter((r) => r.state === "ok").length;
  const slow = rows.filter((r) => r.state === "slow").length;
  const dead = rows.filter((r) => r.state === "dead").length;
  return { total, ok, slow, dead };
});

const bypassMode = computed({
  get: () => (status.bypass?.mode ?? "off") as BypassMode,
  set: (m: BypassMode) => void setBypass(m),
});

const udpMode = computed({
  get: () => (status.udp?.mode ?? "off") as UdpVpnMode,
  set: (m: UdpVpnMode) => void setUdp(m),
});

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

async function setBypass(mode: BypassMode) {
  busy.value = "bypass";
  try {
    await overview.bypassSet(mode);
    toast.ok(mode === "off" ? "Обход DPI выключен" : `Обход DPI: ${mode}`);
  } catch (e) {
    toast.fromError(e, "Не удалось переключить обход DPI");
  } finally {
    busy.value = "";
    await status.refreshExtras();
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
  if (c) clients.value = c.clients?.length ?? 0;
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
      <div class="udp">
        <p class="lbl">UDP через VPN</p>
        <SegmentedControl
          v-model="udpMode"
          label="UDP через VPN"
          :busy="busy === 'udp'"
          :options="[
            { value: 'off', label: 'Выкл' },
            { value: 'list', label: 'По списку' },
            { value: 'all', label: 'Весь UDP' },
          ]"
        />
        <p v-if="!status.udpVpnSupported" class="hint">
          Недоступно на этой платформе: нужен TPROXY.
        </p>
      </div>
    </TileCard>

    <TileCard title="Обход DPI">
      <SegmentedControl
        v-model="bypassMode"
        label="Стратегия обхода DPI"
        :busy="busy === 'bypass'"
        :options="[
          { value: 'off', label: 'Выкл' },
          { value: 'zapret', label: 'zapret' },
          {
            value: 'zapret2',
            label: 'zapret2',
            disabled: !status.zapret2Supported,
            hint: status.zapret2Supported ? undefined : 'Нужен NFQUEUE — на этой платформе нет',
          },
        ]"
      />
      <p class="meta">
        <span>{{ zp?.domains ?? 0 }} доменов</span>
        <span v-if="zp?.ipset_count">{{ zp.ipset_count }} адресов</span>
        <span v-if="isSet(zp?.port)">порт {{ zp?.port }}</span>
        <span v-else-if="!zp?.running">не запущен</span>
      </p>
    </TileCard>

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
    </TileCard>

    <TileCard title="Версии">
      <p class="meta col">
        <span>Панель {{ status.data?.version ?? "—" }}</span>
        <span>sing-box {{ status.data?.binaries?.singbox_version ?? "—" }}</span>
        <span>tpws {{ status.data?.binaries?.tpws_version ?? "—" }}</span>
      </p>
      <template #actions>
        <UiButton @click="$router.push('/journal')">Обновления</UiButton>
      </template>
    </TileCard>
  </div>

  <ProfilePicker :open="pickerOpen" @close="pickerOpen = false" />
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
