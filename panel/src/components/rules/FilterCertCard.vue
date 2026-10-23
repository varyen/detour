<script setup lang="ts">
/* Корневой сертификат «Фильтра HTTPS»: роутер забирает его у AdGuard сам (или
   берёт загруженный файл) и раздаёт устройствам страницей /cgi-bin/detour-ca
   без входа в панель. QR ведёт на её адрес в локальной сети — телефон
   открывает страницу там, где он есть, даже если панель открыта снаружи. */
import { computed, onMounted, ref } from "vue";
import qrcode from "qrcode-generator";
import UiButton from "@/components/UiButton.vue";
import { overview } from "@/api";
import type { FilterCaResponse } from "@/api";
import { useToastStore } from "@/stores/toast";

const toast = useToastStore();
const data = ref<FilterCaResponse | null>(null);
const busy = ref(false);
const fileInput = ref<HTMLInputElement | null>(null);

const ca = computed(() => data.value?.ca ?? { present: false });

/* Ссылка для устройств: адрес роутера в LAN; без него — тот же адрес, что у панели. */
const pageUrl = computed(() => data.value?.url || `${location.origin}/cgi-bin/detour-ca`);

const qrSvg = computed(() => {
  if (!ca.value.present) return "";
  const qr = qrcode(0, "M");
  qr.addData(pageUrl.value, "Byte");
  qr.make();
  return qr.createSvgTag({ cellSize: 4, margin: 3, scalable: true });
});

const name = computed(() => {
  const s = ca.value.subject ?? "";
  return /CN\s*=\s*([^,]+)/.exec(s)?.[1]?.trim() || s || "Сертификат фильтра";
});

const origin = computed(() =>
  ca.value.from === "adguard" ? `получен от AdGuard на ${ca.value.source}` : (ca.value.source ?? "загружен вручную"),
);

async function run(p: Promise<FilterCaResponse>, ok?: string) {
  busy.value = true;
  try {
    data.value = await p;
    if (ok) toast.ok(ok);
  } catch (e) {
    toast.fromError(e, "Не получилось");
  } finally {
    busy.value = false;
  }
}

onMounted(() => run(overview.filterCaGet()));

function fetchCa() {
  return run(overview.filterCaFetch(), "Сертификат получен от прокси");
}

function clearCa() {
  return run(overview.filterCaClear(), "Сертификат убран");
}

function toBase64(buf: ArrayBuffer): string {
  let s = "";
  const bytes = new Uint8Array(buf);
  for (let i = 0; i < bytes.length; i += 0x8000) {
    s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(s);
}

async function onFile(ev: Event) {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (!file) return;
  if (file.size > 64 * 1024) {
    toast.fromError(new Error("Файл больше 64 КБ — это не сертификат"), "Не получилось");
    return;
  }
  await run(overview.filterCaUpload(toBase64(await file.arrayBuffer()), file.name), "Сертификат загружен");
}

async function copyLink() {
  try {
    await navigator.clipboard.writeText(pageUrl.value);
    toast.ok("Ссылка скопирована");
  } catch {
    toast.fromError(new Error(pageUrl.value), "Скопируйте ссылку вручную");
  }
}
</script>

<template>
  <div class="card">
    <div class="head">
      <span class="label">Сертификат фильтра</span>
      <span v-if="ca.present" class="ok">есть</span>
      <span v-else class="miss">не получен</span>
    </div>

    <template v-if="ca.present">
      <p class="line">
        <b>{{ name }}</b>
        <span v-if="ca.not_after" class="dim"> · до {{ ca.not_after.split("-").reverse().join(".") }}</span>
      </p>
      <p class="dim">{{ origin }}</p>
      <p class="fp">SHA-256: {{ ca.sha256 }}</p>

      <div class="share">
        <!-- SVG собирает qrcode-generator из наших же данных: только rect/path. -->
        <div class="qr" v-html="qrSvg"></div>
        <div class="share-text">
          <p>
            Наведите камеру устройства на код или откройте на нём ссылку — страница
            сама покажет шаги для его системы и проверит, что всё работает.
          </p>
          <a class="link" :href="pageUrl" target="_blank" rel="noopener">{{ pageUrl }}</a>
          <div class="actions">
            <UiButton @click="copyLink">Скопировать ссылку</UiButton>
          </div>
        </div>
      </div>
    </template>
    <p v-else class="dim">
      Без сертификата устройства с фильтром видят ошибку на каждом сайте. AdGuard
      роутер спрашивает сам — нажмите «Забрать у прокси», когда AdGuard запущен. Для
      другого фильтра загрузите его корневой сертификат (.crt, .cer или .pem).
    </p>

    <div class="actions">
      <UiButton :busy="busy" :disabled="busy" @click="fetchCa">
        {{ ca.present ? "Забрать заново" : "Забрать у прокси" }}
      </UiButton>
      <UiButton :disabled="busy" @click="fileInput?.click()">Загрузить файл…</UiButton>
      <UiButton v-if="ca.present" :disabled="busy" @click="clearCa">Убрать</UiButton>
      <input
        ref="fileInput"
        class="hidden"
        type="file"
        accept=".crt,.cer,.pem,.der,application/x-x509-ca-cert,application/x-pem-file"
        @change="onFile"
      />
    </div>
  </div>
</template>

<style scoped>
.card {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 9px;
  margin-bottom: 12px;
  min-width: 0;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 10px;
}
.label {
  font-size: 13px;
  font-weight: 600;
  color: var(--ink);
}
.ok {
  font-size: 12.5px;
  color: var(--ok);
}
.miss {
  font-size: 12.5px;
  color: var(--bad);
}
.line {
  font-size: 13.5px;
}
.dim {
  font-size: 12px;
  color: var(--dim);
}
.fp {
  font-family: var(--mono, ui-monospace, monospace);
  font-size: 11.5px;
  color: var(--dim);
  word-break: break-all;
}
.share {
  display: flex;
  gap: 14px;
  align-items: flex-start;
  flex-wrap: wrap;
}
.share-text {
  flex: 1 1 220px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 13px;
}
.link {
  color: var(--accent);
  word-break: break-all;
  font-size: 13px;
}
/* QR всегда тёмным по белому: камеры плохо читают инверсию тёмной темы. */
.qr {
  background: #fff;
  border-radius: var(--radius-sm);
  padding: 6px;
  width: 168px;
  flex: none;
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
.hidden {
  display: none;
}
</style>
