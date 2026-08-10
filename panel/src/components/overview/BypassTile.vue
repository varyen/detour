<script setup lang="ts">
/* Плитка «Обход DPI». Отдельным компонентом, потому что движок — это не один
   переключатель: у него выбранный режим, реально работающий процесс (это разные
   вещи), автозапуск, стратегия и метрики очереди.

   Ключевая правка против первой версии: состояние берётся из bypass_status, а не
   из status.zapret. status.zapret — это служба tpws, и режим zapret2 её штатно
   останавливает, поэтому при работающем nfqws2 плитка писала «не запущен». */
import { computed, ref } from "vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import { overview } from "@/api";
import type { BypassMode } from "@/api";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { isSet } from "@/lib/format";

/* Дублирует NFQWS_STRATEGY_DEFAULT из router_files/detour-bypass:57 — бэкенд не
   отдаёт дефолт отдельным полем, а кнопка «По умолчанию» без него невозможна.
   Так же было и в старой панели (index.html:18201). */
const NFQWS2_DEFAULT_STRATEGY =
  "--filter-tcp=443 --filter-l7=tls --payload=tls_client_hello --lua-desync=tcpseg:pos=0,midsld:ip_id=rnd:repeats=2";

const status = useStatusStore();
const toast = useToastStore();

const busy = ref("");
const strategyOpen = ref(false);
const strategy = ref("");
const strategyErr = ref("");

const zp = computed(() => status.data?.zapret);
const bp = computed(() => status.bypass);

const mode = computed({
  get: () => (bp.value?.mode ?? "off") as BypassMode,
  set: (m: BypassMode) => void setMode(m),
});

const running = computed(() => status.bypassRunning);
/** Выбран движок, но процесса нет — это состояние нужно уметь исправить кнопкой. */
const stopped = computed(() => mode.value !== "off" && running.value === "none");

const autostart = computed({
  get: () => status.bypassAutostart,
  set: (on: boolean) => void setAutostart(on),
});

const stateText = computed(() => {
  if (running.value !== "none") return `работает: ${running.value}`;
  if (stopped.value) return `выбран ${mode.value}, остановлен`;
  return "выключен";
});

const stateKind = computed(() => {
  if (running.value !== "none") return "ok";
  return stopped.value ? "warn" : "off";
});

const zapret2Hint = computed(() => {
  if (status.zapret2Supported) return undefined;
  if (status.nfqws2Missing) return "Пакет nfqws2 не установлен";
  return "Нужен NFQUEUE — на этой платформе его нет";
});

async function run(tag: string, fn: () => Promise<unknown>, ok: string, fail: string) {
  busy.value = tag;
  try {
    await fn();
    toast.ok(ok);
  } catch (e) {
    toast.fromError(e, fail);
  } finally {
    busy.value = "";
    await status.refreshExtras();
    void status.refresh(true);
  }
}

function setMode(m: BypassMode) {
  return run(
    "mode",
    () => overview.bypassSet(m),
    m === "off" ? "Обход DPI выключен" : `Обход DPI: ${m}`,
    "Не удалось переключить обход DPI",
  );
}

/* Старт и рестарт — это один и тот же bypass_set с уже выбранным режимом:
   detour-bypass переставляет правила и поднимает процесс заново. */
function start() {
  return run("start", () => overview.bypassSet(mode.value), `${mode.value} запущен`, "Не удалось запустить");
}

function restart() {
  return run("restart", () => overview.bypassSet(mode.value), `${mode.value} перезапущен`, "Не удалось перезапустить");
}

function stop() {
  return run("stop", () => overview.bypassStop(), "Обход DPI остановлен", "Не удалось остановить");
}

function setAutostart(on: boolean) {
  return run(
    "autostart",
    () => overview.bypassAutostart(on),
    on ? "Движок будет подниматься при старте роутера" : "Автозапуск выключен",
    "Не удалось изменить автозапуск",
  );
}

async function openStrategy() {
  strategyErr.value = "";
  /* GET отдаёт пустоту, если своя стратегия ещё не сохранена. Подставляем
     действующую из bypass_status — она и есть дефолт бэкенда. */
  const stored = await overview.bypassStrategyGet().catch(() => "");
  strategy.value = (stored || bp.value?.strategy || NFQWS2_DEFAULT_STRATEGY).trim();
  strategyOpen.value = true;
}

async function saveStrategy() {
  const line = strategy.value.trim().replace(/\s*\n[\s\S]*$/, "");
  /* Ту же проверку делает CGI (detour-api:2923), но получить отказ уже после
     отправки — хуже, чем узнать сразу: без --lua-desync стратегия ничего не
     обходит. */
  if (!line.includes("--lua-desync=")) {
    strategyErr.value = "Стратегия должна содержать --lua-desync=…";
    return;
  }
  busy.value = "strategy";
  try {
    await overview.bypassStrategySet(line);
    strategyOpen.value = false;
    toast.ok(
      mode.value === "zapret2" ? "Стратегия сохранена, zapret2 перезапущен" : "Стратегия сохранена",
    );
  } catch (e) {
    strategyErr.value = e instanceof Error ? e.message : "Не удалось сохранить стратегию";
  } finally {
    busy.value = "";
    await status.refreshExtras();
  }
}
</script>

<template>
  <TileCard title="Обход DPI">
    <SegmentedControl
      v-model="mode"
      label="Движок обхода DPI"
      :busy="busy === 'mode'"
      :options="[
        { value: 'off', label: 'Выкл' },
        { value: 'zapret', label: 'zapret' },
        {
          value: 'zapret2',
          label: 'zapret2',
          disabled: !status.zapret2Supported,
          hint: zapret2Hint,
        },
      ]"
    />

    <p class="state">
      <i class="dot" :class="stateKind" aria-hidden="true"></i>
      <span>{{ stateText }}</span>
      <span v-if="running === 'zapret2' && status.data?.binaries?.nfqws2_version" class="mono dim">
        nfqws2 {{ status.data.binaries.nfqws2_version }}
      </span>
      <span v-else-if="running === 'zapret' && status.data?.binaries?.tpws_version" class="mono dim">
        tpws {{ status.data.binaries.tpws_version }}
      </span>
    </p>

    <p class="meta">
      <span>{{ zp?.domains ?? 0 }} доменов</span>
      <span v-if="zp?.ipset_count">{{ zp.ipset_count }} адресов</span>
      <template v-if="running === 'zapret2'">
        <span v-if="isSet(bp?.qnum)">очередь {{ bp?.qnum }}</span>
        <span v-if="isSet(bp?.queued)">{{ bp?.queued }} пакетов отдано</span>
      </template>
      <span v-else-if="running === 'zapret' && isSet(zp?.port)">порт {{ zp?.port }}</span>
    </p>

    <p v-if="status.nfqws2Missing" class="hint">
      zapret2 требует пакет nfqws2 —
      <RouterLink :to="{ path: '/journal', query: { focus: 'updates' } }">
        поставить в «Журнале»
      </RouterLink>.
    </p>
    <!-- Какие сайты вообще идут в обход — правится в «Правилах», а видно здесь. -->
    <p class="hint">
      <RouterLink :to="{ path: '/rules', query: { focus: 'zapret' } }">
        Список сайтов для обхода
      </RouterLink>
    </p>

    <SwitchToggle
      v-model="autostart"
      label="Автозапуск"
      :busy="busy === 'autostart'"
      :disabled="mode === 'off'"
      :hint="
        mode === 'off'
          ? 'Нечего запускать: движок выключен'
          : autostart
            ? 'Поднимется сам после перезагрузки роутера'
            : 'После перезагрузки роутера останется выключенным'
      "
    />

    <template #actions>
      <UiButton v-if="stopped" variant="primary" :busy="busy === 'start'" @click="start">
        Старт
      </UiButton>
      <template v-if="running !== 'none'">
        <UiButton :busy="busy === 'restart'" @click="restart">Перезапустить</UiButton>
        <UiButton :busy="busy === 'stop'" @click="stop">Стоп</UiButton>
      </template>
      <UiButton v-if="mode === 'zapret2'" @click="openStrategy">Стратегия</UiButton>
      <UiButton
        v-else-if="mode === 'zapret'"
        @click="$router.push({ path: '/journal', query: { focus: 'services' } })"
      >
        Аргументы tpws
      </UiButton>
    </template>
  </TileCard>

  <DrawerSheet
    :open="strategyOpen"
    title="Стратегия nfqws2"
    wide
    @close="strategyOpen = false"
  >
    <p class="note">
      Одна строка аргументов nfqws2. Обязателен <code>--lua-desync=…</code> — без
      него движок поднимется, но обходить ничего не будет. Сохранение при активном
      zapret2 сразу перезапускает движок.
    </p>
    <textarea v-model="strategy" rows="6" spellcheck="false" aria-label="Стратегия nfqws2"></textarea>
    <p v-if="strategyErr" class="err">{{ strategyErr }}</p>
    <template #footer>
      <UiButton variant="primary" :busy="busy === 'strategy'" @click="saveStrategy">
        Сохранить
      </UiButton>
      <UiButton @click="strategy = NFQWS2_DEFAULT_STRATEGY">По умолчанию</UiButton>
      <UiButton @click="strategyOpen = false">Отмена</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.state {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 13.5px;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--faint);
  flex: 0 0 auto;
}
.dot.ok {
  background: var(--ok);
}
.dot.warn {
  background: var(--warn);
}
.dim {
  color: var(--dim);
  font-size: 12px;
}
.meta {
  font-size: 13px;
  color: var(--dim);
  display: flex;
  gap: 4px 14px;
  flex-wrap: wrap;
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
.hint a {
  color: var(--accent);
}
.note {
  font-size: 13px;
  color: var(--dim);
  margin-bottom: 10px;
}
.note code {
  font-family: var(--mono);
  font-size: 12px;
  color: var(--ink);
}
textarea {
  width: 100%;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.5;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 10px 12px;
  resize: vertical;
}
.err {
  color: var(--bad);
  font-size: 12.5px;
  margin-top: 8px;
}
</style>
