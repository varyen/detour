<script setup lang="ts">
/* Клиент VPN-сервера: подписка со всеми его протоколами, конфиг AmneziaWG
   (QR + файл), ссылка VLESS-Reality (QR + текст) и история подключений. Конфиг и ссылка — ключи клиента, поэтому
   запрашиваются только при открытии шторки и не задерживаются после закрытия. */
import { computed, ref, watch } from "vue";
import qrcode from "qrcode-generator";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { services } from "@/api";
import type { ServerClient, ServerSession } from "@/api";
import { fmtBytes } from "@/lib/format";
import { saveFile, savedWhere } from "@/lib/download";
import { useToastStore } from "@/stores/toast";

type Tab = "sub" | "conf" | "vless" | "history";

const props = defineProps<{
  open: boolean;
  client: ServerClient | null;
  tab: Tab;
  /** Какие входы сейчас включены на сервере — вкладки только для них. */
  awg: boolean;
  vless: boolean;
  /** Подписки включены и их веб-сервер работает. */
  sub?: boolean;
  /** Подписка отдаётся по HTTP — ключи видны по пути. */
  subPlain?: boolean;
  /** false — объём трафика клиента роутер не видит (встроенный WireGuard KeeneticOS). */
  traffic?: boolean;
}>();
const emit = defineEmits<{ close: [] }>();
const toast = useToastStore();

const view = ref<Tab>("conf");
const conf = ref("");
const link = ref("");
const tlsLink = ref("");
const wsLink = ref("");
const xhLink = ref("");
const subUrl = ref("");
const confirmReset = ref(false);
const loading = ref(false);
const error = ref("");
const sessions = ref<ServerSession[]>([]);

function qrOf(text: string): string {
  if (!text) return "";
  const qr = qrcode(0, "L");
  qr.addData(text, "Byte");
  qr.make();
  return qr.createSvgTag({ cellSize: 4, margin: 3, scalable: true });
}
const qrSvg = computed(() => qrOf(conf.value));
const linkQr = computed(() => qrOf(link.value));
const tlsQr = computed(() => qrOf(tlsLink.value));
const wsQr = computed(() => qrOf(wsLink.value));
const xhQr = computed(() => qrOf(xhLink.value));
const subQr = computed(() => qrOf(subUrl.value));

const tabs = computed(() => {
  const t: { value: Tab; label: string }[] = [];
  if (props.sub) t.push({ value: "sub", label: "Подписка" });
  if (props.awg && props.client?.awg !== false) t.push({ value: "conf", label: "AmneziaWG" });
  if (props.vless && props.client?.vless) t.push({ value: "vless", label: "VLESS" });
  t.push({ value: "history", label: "История" });
  return t;
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

async function loadLink() {
  if (!props.client) return;
  loading.value = true;
  error.value = "";
  try {
    const r = await services.serverClientLink(props.client.id);
    link.value = r.link;
    tlsLink.value = r.tls_link ?? "";
    wsLink.value = r.ws_link ?? "";
    xhLink.value = r.xhttp_link ?? "";
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Не удалось получить ссылку";
  } finally {
    loading.value = false;
  }
}

async function loadSub() {
  if (!props.client) return;
  loading.value = true;
  error.value = "";
  try {
    subUrl.value = (await services.serverClientSub(props.client.id)).url;
  } catch (e) {
    error.value = e instanceof Error ? e.message : "Не удалось получить ссылку подписки";
  } finally {
    loading.value = false;
  }
}

async function resetSub() {
  if (!props.client) return;
  confirmReset.value = false;
  try {
    await services.serverClientSubReset(props.client.id);
    toast.ok("Старая ссылка больше не работает — отдайте клиенту новую");
    await loadSub();
  } catch (e) {
    toast.fromError(e, "Не удалось сменить ссылку");
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
  if (view.value === "sub" && !subUrl.value) void loadSub();
  if (view.value === "conf" && !conf.value) void loadConf();
  if (view.value === "vless" && !link.value) void loadLink();
  if (view.value === "history") void loadSessions();
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      const avail = tabs.value.map((t) => t.value);
      view.value = avail.includes(props.tab) ? props.tab : avail[0];
      load();
    } else {
      conf.value = "";
      link.value = "";
      tlsLink.value = "";
      wsLink.value = "";
      xhLink.value = "";
      subUrl.value = "";
      confirmReset.value = false;
      sessions.value = [];
      error.value = "";
    }
  },
);
watch(view, load);

async function download() {
  try {
    const where = await saveFile(fileName.value, conf.value, "text/plain");
    if (where) toast.ok(savedWhere(where, ""));
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить файл");
  }
}

async function copy(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.ok(`${what} скопирован${what === "Ссылка" ? "а" : ""}`);
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
        :options="tabs"
      />

      <p v-if="error" class="note bad">{{ error }}</p>

      <template v-if="view === 'sub'">
        <p class="lead">
          Одна ссылка со всеми протоколами клиента — добавьте её как подписку
          в Clash Verge, FlClash, Hiddify, v2rayNG, v2rayN, Streisand или в
          Detour. Приложение само заберёт настройки и обновит их, если на
          сервере что-то поменяется. Ссылка даёт доступ в домашнюю сеть — не
          пересылайте её чужим.
        </p>
        <p class="note faint">
          AmneziaWG получат Clash-совместимые приложения (mihomo) и Detour;
          v2rayNG, v2rayN, Hiddify и sing-box получат VLESS — AmneziaWG они не
          умеют.
        </p>
        <p v-if="props.subPlain" class="note warn">
          Подписка идёт по HTTP: ключи клиента видны по пути. Выпустите
          сертификат панели («Сервисы → Сертификат») — подписки перейдут на HTTPS.
        </p>
        <p v-if="loading" class="note">Готовлю ссылку…</p>
        <template v-else-if="subUrl">
          <!-- SVG собирает qrcode-generator из наших же данных: только rect/path. -->
          <div class="qr" v-html="subQr"></div>
          <div class="actions">
            <UiButton variant="primary" @click="copy(subUrl, 'Ссылка')">Скопировать ссылку</UiButton>
            <template v-if="confirmReset">
              <UiButton variant="danger" @click="resetSub">Да, сменить</UiButton>
              <UiButton @click="confirmReset = false">Отмена</UiButton>
            </template>
            <UiButton v-else @click="confirmReset = true">Сменить ссылку</UiButton>
          </div>
          <p v-if="confirmReset" class="note">
            Старая ссылка перестанет работать — приложения с ней перестанут
            обновляться. Подключение по уже скачанным настройкам не прервётся.
          </p>
          <details class="raw">
            <summary>Показать ссылку</summary>
            <pre>{{ subUrl }}</pre>
          </details>
        </template>
      </template>

      <template v-else-if="view === 'conf'">
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
            <UiButton @click="copy(conf, 'Конфиг')">Скопировать</UiButton>
          </div>
          <details class="raw">
            <summary>Показать текст конфига</summary>
            <pre>{{ conf }}</pre>
          </details>
        </template>
      </template>

      <template v-else-if="view === 'vless'">
        <p class="lead">
          Отсканируйте код или вставьте ссылку в v2rayNG, Hiddify, v2rayN,
          Streisand, FoXray или в приложение Detour. VLESS-Reality идёт по TCP и
          выглядит как обычный HTTPS к сайту-маске — пригодится там, где UDP
          режут. Ссылка даёт доступ в домашнюю сеть — не пересылайте её чужим.
        </p>
        <p v-if="loading" class="note">Готовлю ссылку…</p>
        <template v-else-if="link">
          <!-- SVG собирает qrcode-generator из наших же данных: только rect/path. -->
          <div class="qr" v-html="linkQr"></div>
          <div class="actions">
            <UiButton variant="primary" @click="copy(link, 'Ссылка')">Скопировать ссылку</UiButton>
          </div>
          <details class="raw">
            <summary>Показать ссылку</summary>
            <pre>{{ link }}</pre>
          </details>
          <template v-if="tlsLink">
            <p class="lead">
              Вторая ссылка — VLESS поверх обычного TLS на настоящем сертификате.
              Пробуйте её, если с первой не подключается: часть сетей портит Reality.
            </p>
            <!-- SVG собирает qrcode-generator из наших же данных: только rect/path. -->
            <div class="qr" v-html="tlsQr"></div>
            <div class="actions">
              <UiButton @click="copy(tlsLink, 'Ссылка')">Скопировать ссылку TLS</UiButton>
            </div>
            <details class="raw">
              <summary>Показать ссылку TLS</summary>
              <pre>{{ tlsLink }}</pre>
            </details>
          </template>
          <template v-if="wsLink">
            <p class="lead">
              Ссылка WebSocket — туннель под видом обычного сайта. Для сетей, где не
              проходят ни Reality, ни TLS.
            </p>
            <div class="qr" v-html="wsQr"></div>
            <div class="actions">
              <UiButton @click="copy(wsLink, 'Ссылка')">Скопировать ссылку WebSocket</UiButton>
            </div>
            <details class="raw">
              <summary>Показать ссылку WebSocket</summary>
              <pre>{{ wsLink }}</pre>
            </details>
          </template>
          <template v-if="xhLink">
            <p class="lead">
              Ссылка xhttp — туннель обычными HTTP-запросами к сайту. Приложения на
              Xray и mihomo: Happ, v2rayNG, Hiddify, Detour.
            </p>
            <div class="qr" v-html="xhQr"></div>
            <div class="actions">
              <UiButton @click="copy(xhLink, 'Ссылка')">Скопировать ссылку xhttp</UiButton>
            </div>
            <details class="raw">
              <summary>Показать ссылку xhttp</summary>
              <pre>{{ xhLink }}</pre>
            </details>
          </template>
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
              <span v-if="props.traffic !== false">↓ {{ fmtBytes(s.tx) }} · ↑ {{ fmtBytes(s.rx) }}</span>
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
.note.warn {
  color: var(--warn);
  border-color: color-mix(in srgb, var(--warn) 45%, transparent);
}
.note.faint {
  color: var(--faint);
  border-color: transparent;
  padding: 0 2px;
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
