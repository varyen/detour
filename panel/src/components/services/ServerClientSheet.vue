<script setup lang="ts">
/* Клиент VPN-сервера: конфиг для приложения (QR + файл) и история подключений.
   Конфиг несёт приватный ключ клиента, поэтому запрашивается только при
   открытии шторки и в памяти страницы не задерживается после закрытия. */
import { computed, ref, watch } from "vue";
import qrcode from "qrcode-generator";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { services } from "@/api";
import type { ServerClient, ServerSession } from "@/api";
import { fmtBytes } from "@/lib/format";
import { useToastStore } from "@/stores/toast";

type Tab = "conf" | "history";

const props = defineProps<{ open: boolean; client: ServerClient | null; tab: Tab }>();
const emit = defineEmits<{ close: [] }>();
const toast = useToastStore();

const view = ref<Tab>("conf");
const conf = ref("");
const loading = ref(false);
const error = ref("");
const sessions = ref<ServerSession[]>([]);

const qrSvg = computed(() => {
  if (!conf.value) return "";
  const qr = qrcode(0, "L");
  qr.addData(conf.value, "Byte");
  qr.make();
  return qr.createSvgTag({ cellSize: 4, margin: 3, scalable: true });
});

/* Имя туннеля в WireGuard/AmneziaWG берётся из имени файла: только
   [A-Za-z0-9_=+.-] и не длиннее 15 символов, иначе импорт отказывает. */
const fileName = computed(() => {
  const ascii = (props.client?.name || "")
    .replace(/[^A-Za-z0-9_=+.-]+/g, "_")
    .replace(/^_+|_+$/g, "");
  const octet = props.client?.ip.split(".").pop() ?? "";
  const base = (ascii.length >= 2 ? ascii : `detour-${octet}`).slice(0, 15);
  return `${base}.conf`;
});

async function loadConf() {
  if (!props.client) return;
  loading.value = true;
  error.value = "";
  try {
    conf.value = (await services.serverClientConf(props.client.id)).conf;
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Не удалось получить конфиг";
  } finally {
    loading.value = false;
  }
}

async function loadSessions() {
  if (!props.client) return;
  loading.value = true;
  error.value = "";
  try {
    sessions.value = (await services.serverSessions(props.client.id)).sessions;
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Не удалось получить историю";
  } finally {
    loading.value = false;
  }
}

function load() {
  if (view.value === "conf" && !conf.value) void loadConf();
  if (view.value === "history") void loadSessions();
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      view.value = props.tab;
      load();
    } else {
      conf.value = "";
      sessions.value = [];
      error.value = "";
    }
  },
);
watch(view, load);

function download() {
  const url = URL.createObjectURL(new Blob([conf.value], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = fileName.value;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

async function copy() {
  try {
    await navigator.clipboard.writeText(conf.value);
    toast.ok("Конфиг скопирован");
  } catch {
    toast.error("Браузер не дал скопировать — скачайте файл");
  }
}

function fmtTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function fmtDuration(sec: number): string {
  if (sec < 60) return "меньше минуты";
  const m = Math.round(sec / 60);
  if (m < 60) return `${m} мин`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} ч ${m % 60} мин`;
  return `${Math.floor(h / 24)} д ${h % 24} ч`;
}

function host(remote: string): string {
  return remote.replace(/:\d+$/, "");
}
</script>

<template>
  <DrawerSheet :open="open" :title="client ? client.name : 'Клиент'" @close="emit('close')">
    <div class="wrap">
      <SegmentedControl
        v-model="view"
        label="Раздел"
        :options="[
          { value: 'conf', label: 'Подключение' },
          { value: 'history', label: 'История' },
        ]"
      />

      <p v-if="error" class="note bad">{{ error }}</p>

      <template v-if="view === 'conf'">
        <p class="lead">
          Отсканируйте код в приложении AmneziaVPN или AmneziaWG или импортируйте
          файл. Конфиг даёт доступ в домашнюю сеть — не пересылайте его чужим.
        </p>
        <p v-if="loading" class="note">Готовлю конфиг…</p>
        <template v-else-if="conf">
          <!-- SVG собирает qrcode-generator из наших же данных: только rect/path. -->
          <div class="qr" v-html="qrSvg"></div>
          <div class="actions">
            <UiButton variant="primary" @click="download">Скачать {{ fileName }}</UiButton>
            <UiButton @click="copy">Скопировать</UiButton>
          </div>
          <details class="raw">
            <summary>Показать текст конфига</summary>
            <pre>{{ conf }}</pre>
          </details>
        </template>
      </template>

      <template v-else>
        <p v-if="loading" class="note">Загружаю…</p>
        <p v-else-if="!sessions.length" class="note">
          Завершённых подключений пока нет. Сессия попадает сюда, когда клиент
          отключился (через три минуты без связи).
        </p>
        <ul v-else class="sess">
          <li v-for="s in sessions" :key="`${s.start}-${s.end}`">
            <div class="s-head">
              <span>{{ fmtTime(s.start) }}</span>
              <span class="dim">{{ fmtDuration(Math.max(0, s.end - s.start)) }}</span>
            </div>
            <div class="s-meta">
              <span>↓ {{ fmtBytes(s.tx) }} · ↑ {{ fmtBytes(s.rx) }}</span>
              <span v-if="s.remote" class="mono">{{ host(s.remote) }}</span>
            </div>
          </li>
        </ul>
      </template>
    </div>
  </DrawerSheet>
</template>

<style scoped>
.wrap {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
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
}
.note.bad {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
/* QR всегда тёмным по белому: камеры плохо читают инверсию тёмной темы. */
.qr {
  background: #fff;
  border-radius: var(--radius-sm);
  padding: 8px;
  width: min(100%, 320px);
  align-self: center;
}
.qr :deep(svg) {
  display: block;
  width: 100%;
  height: auto;
}
.actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.raw summary {
  cursor: pointer;
  font-size: 12.5px;
  color: var(--dim);
}
.raw pre {
  margin-top: 8px;
  font-family: var(--mono);
  font-size: 11.5px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px;
}
.sess {
  list-style: none;
  margin: 0;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
}
.sess li {
  padding: 9px 11px;
  display: grid;
  gap: 3px;
  font-size: 12.5px;
}
.sess li + li {
  border-top: 1px solid var(--line);
}
.s-head,
.s-meta {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}
.s-meta {
  color: var(--dim);
  font-variant-numeric: tabular-nums;
}
.dim {
  color: var(--faint);
}
.mono {
  font-family: var(--mono);
  font-size: 11.5px;
}
</style>
