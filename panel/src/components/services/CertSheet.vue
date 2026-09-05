<script setup lang="ts">
/* Выпуск сертификата. Роутер отвечает сразу «начал» и уходит работать в фон:
   получить письмо от удостоверяющего центра, подтвердить домен и переложить
   файлы занимает от десяти секунд до минуты. Поэтому здесь не ожидание ответа,
   а опрос состояния с показом того, что происходит.

   Способов подтверждения два, и выбор между ними — не вкус, а разные
   возможности: через 80-й порт выпускается ровно одно имя и только если порт
   доступен снаружи; через DNS-API — wildcard на всю зону (Let's Encrypt выдаёт
   его исключительно так) и вообще без открытого порта. Wildcard — то, что делает
   публикацию сервисов по имени бесплатной: новое имя не требует перевыпуска. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import FormField from "@/components/services/FormField.vue";
import { poll, services } from "@/api";
import type { CertDnsStatus } from "@/api";

/** Состояние выпуска так, как его отдаёт роутер. */
interface CertState {
  domain?: string;
  target?: string;
  expiry?: string;
  last_result?: string;
  last_error?: string;
}

type Method = "http-01" | "dns-01";

const props = defineProps<{
  open: boolean;
  /** Домен и почта из прошлого выпуска — их удобно предложить снова. */
  domain: string;
  email: string;
  /** Что роутер знает про своё окружение: кто слушает 80-й порт и т. п. */
  http80: string;
  wanIp: string;
  acmePresent: boolean;
  /** На Keenetic 80-й порт занят своим вебом — подтверждение требует проброса. */
  keenetic: boolean;
  /** Порт панели с роутера: цель того самого проброса. */
  panelPort?: number;
  /** Чем подтверждался нынешний сертификат и какие имена в него вошли. */
  challenge?: string;
  altDomains?: string;
}>();

const emit = defineEmits<{ close: []; done: [] }>();

const domain = ref("");
const email = ref("");
const method = ref<Method>("http-01");
const alts = ref("");
const busy = ref(false);
const progress = ref("");
const err = ref("");
const result = ref<CertState | null>(null);

/* --- состояние DNS-провайдера --- */
const dns = ref<CertDnsStatus | null>(null);
const provider = ref("");
const dnsValues = ref<Record<string, string>>({});
const dnsBusy = ref(false);
const dnsNote = ref("");
const dnsErr = ref("");

/** Человеческие подписи к полям плагинов acme.sh — сами имена машинные. */
const VAR_LABELS: Record<string, { label: string; hint?: string; secret?: boolean }> = {
  PDNS_Url: { label: "Адрес API PowerDNS", hint: "например https://pdns-api.example.com" },
  PDNS_ServerId: { label: "Идентификатор сервера", hint: "обычно localhost" },
  PDNS_Token: { label: "Ключ API (X-API-Key)", secret: true },
  PDNS_Ttl: { label: "TTL записи, секунд", hint: "60 — чтобы проверка не ждала лишнего" },
  GCORE_Key: { label: "Ключ API Gcore", secret: true },
};
const PROVIDER_LABELS: Record<string, string> = {
  pdns: "Свой PowerDNS",
  gcore: "Gcore DNS",
};

function labelFor(name: string) {
  return VAR_LABELS[name]?.label ?? name;
}
function hintFor(name: string) {
  return VAR_LABELS[name]?.hint;
}
function isSecret(name: string) {
  return VAR_LABELS[name]?.secret === true;
}

async function loadDns() {
  dns.value = await services.certDnsStatus();
  provider.value = dns.value?.provider || dns.value?.providers?.[0] || "";
  syncVars();
}

/* Значения секретов роутер отдаёт звёздочками — их нельзя показывать как
   введённые, иначе «сохранить» отправит обратно строку из звёздочек. */
function syncVars() {
  const out: Record<string, string> = {};
  for (const v of dns.value?.vars ?? []) {
    out[v.name] = isSecret(v.name) ? "" : (v.value ?? "");
  }
  dnsValues.value = out;
}

/* Обязательное поле пустое и на роутере ничего не лежит — выпуск не начнётся. */
const dnsIncomplete = computed(() =>
  dnsVars.value.some(
    (v) =>
      v.required &&
      !dnsValues.value[v.name] &&
      !storedFor.value.find((x) => x.name === v.name)?.value,
  ),
);

const storedSecret = computed(() => {
  const v = storedFor.value.find((x) => isSecret(x.name));
  return !!v?.value;
});

watch(provider, (p, old) => {
  if (!p || p === old || !old) return;
  /* Набор полей берётся из schema, поэтому форма перерисовывается сразу; ранее
     введённые значения относились к другому провайдеру и не переносятся. */
  dnsValues.value = {};
  dnsErr.value = "";
  dnsNote.value = "";
});

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    domain.value = props.domain;
    email.value = props.email;
    alts.value = props.altDomains ?? "";
    method.value = props.challenge === "dns-01" ? "dns-01" : "http-01";
    err.value = "";
    progress.value = "";
    dnsErr.value = "";
    dnsNote.value = "";
    result.value = null;
    await loadDns();
  },
);

const methodOptions = computed(() => [
  { value: "http-01" as Method, label: "Через порт 80" },
  { value: "dns-01" as Method, label: "Через DNS-API" },
]);

const methodHint = computed(() =>
  method.value === "dns-01"
    ? "Роутер сам добавит проверочную запись в вашу DNS-зону. Так можно получить wildcard (*.h.example.com) и открытый 80-й порт не нужен вовсе."
    : "Удостоверяющий центр обратится к роутеру по 80-му порту. Просто, но требует, чтобы порт был доступен снаружи, и выпускает ровно одно имя.",
);

/* Поля выбранного провайдера. Значения (и «уже сохранён» у секретов) есть только
   у того провайдера, что сейчас записан на роутере. */
const dnsVars = computed(() => {
  const schema = dns.value?.schema?.[provider.value];
  if (schema?.length) return schema;
  return provider.value === dns.value?.provider ? (dns.value?.vars ?? []) : [];
});
const storedFor = computed(() =>
  provider.value === dns.value?.provider ? (dns.value?.vars ?? []) : [],
);
const dnsReady = computed(() => dns.value?.ready === true);
const pluginMissing = computed(
  () => !!dns.value?.provider && dns.value?.plugin_present === false,
);

const altList = computed(() =>
  alts.value
    .split(/[\s,]+/)
    .map((v) => v.trim())
    .filter(Boolean),
);

/* Подсказка про wildcard строится от самого домена: для panel.example.com это
   «*.h.example.com» — отдельная ветка под роутер, не задевающая основную зону. */
const altSuggestion = computed(() => {
  const d = domain.value.trim();
  const dot = d.indexOf(".");
  if (dot < 0) return "";
  return `*.h.${d.slice(dot + 1)}`;
});

const wanNote = computed(() => {
  if (!props.wanIp) return "";
  if (/^(10\.|127\.|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.)/.test(props.wanIp)) {
    return `Внешний адрес роутера — ${props.wanIp}, он приватный: роутер стоит за чужим NAT, и удостоверяющий центр до него не достучится.`;
  }
  return `Внешний адрес роутера: ${props.wanIp}`;
});

const http80Note = computed(() => {
  if (method.value === "dns-01") return "";
  if (!props.http80 || props.http80 === "none") {
    return "На 80-м порту роутера сейчас никто не отвечает — подтвердить владение доменом не выйдет, пока порт 80 не открыт снаружи. Через DNS-API это ограничение снимается.";
  }
  return "";
});

/* Порядок подсказок про 80-й порт важен: «никто не отвечает» — жёсткая
   блокировка выпуска, и говорить про проброс поверх неё нечего. Поэтому
   keenetic-подсказка показывается только когда порт вообще отвечает. */
const keenPort80Note = computed(() => {
  if (method.value === "dns-01") return "";
  if (!props.keenetic || http80Note.value) return "";
  const target = props.panelPort ? `на порт панели (${props.panelPort})` : "на порт панели";
  return `На этом роутере 80-й порт занят его собственным веб-интерфейсом. Чтобы удостоверяющий центр попал именно в панель, проброс порта 80 из интернета должен вести ${target}, а не в веб-интерфейс роутера.`;
});

async function saveDns() {
  dnsErr.value = "";
  dnsNote.value = "";
  const values: Record<string, string> = {};
  for (const [k, v] of Object.entries(dnsValues.value)) {
    if (v !== "") values[k] = v;
  }
  if (!Object.keys(values).length) {
    dnsErr.value = "Заполните поля провайдера";
    return;
  }
  dnsBusy.value = true;
  try {
    await services.certDnsSet(provider.value, values);
    await loadDns();
    dnsNote.value = "Настройки сохранены";
  } catch (e) {
    dnsErr.value = e instanceof Error ? e.message : "Не удалось сохранить";
  } finally {
    dnsBusy.value = false;
  }
}

async function checkDns() {
  dnsErr.value = "";
  dnsNote.value = "";
  dnsBusy.value = true;
  try {
    const r = await services.certDnsCheck();
    dnsNote.value = `Ключ принят${r.zones ? `, провайдер отдал зон: ${r.zones}` : ""}`;
  } catch (e) {
    dnsErr.value = e instanceof Error ? e.message : "Проверка не удалась";
  } finally {
    dnsBusy.value = false;
  }
}

function valid(): string {
  const d = domain.value.trim();
  const m = email.value.trim();
  if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/i.test(d)) {
    return "Домен указан неверно — ожидается что-то вроде panel.example.com";
  }
  /* Почта не обязательна: у уже зарегистрированной учётной записи ACME контакта
     может не быть вовсе, и придумывать его за человека неправильно. */
  if (m && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(m)) {
    return "Почта указана неверно — на неё придёт предупреждение об истечении срока";
  }
  if (method.value === "dns-01") {
    if (dnsIncomplete.value || !dnsReady.value || provider.value !== dns.value?.provider)
      return "Сначала заполните и сохраните доступ к DNS-провайдеру";
    for (const a of altList.value) {
      const bare = a.startsWith("*.") ? a.slice(2) : a;
      if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/i.test(bare)) {
        return `Дополнительное имя «${a}» указано неверно`;
      }
    }
  } else if (altList.value.length) {
    return "Дополнительные имена возможны только при подтверждении через DNS-API";
  }
  return "";
}

async function issue() {
  const bad = valid();
  if (bad) {
    err.value = bad;
    return;
  }
  err.value = "";
  result.value = null;
  busy.value = true;
  progress.value = "Отправляю запрос…";
  try {
    /* «-» — «оставить учётную запись как есть»: роутер понимает это как
       отсутствие адреса и не трогает зарегистрированный контакт. */
    const mail = email.value.trim() || "-";
    if (method.value === "dns-01") {
      await services.certIssueDns(domain.value.trim(), mail, altList.value);
      progress.value = "Добавляю запись в DNS и жду её распространения — до нескольких минут";
    } else {
      await services.certIssue(domain.value.trim(), mail);
      progress.value = "Подтверждаю домен — это занимает до минуты";
    }

    /* Пока роутер не отметил, что начал, в состоянии лежит результат прошлого
       выпуска. Принять его за свой — значит соврать об успехе, поэтому первые
       секунды старый ответ игнорируется. */
    let started = false;
    const t0 = Date.now();
    const final = (await poll(() => services.certStatus(), {
      intervalMs: 3000,
      timeoutMs: 600_000,
      done: (s) => {
        const r = (s as CertState | null)?.last_result ?? "";
        if (r === "issuing") {
          started = true;
          return false;
        }
        if (!started && Date.now() - t0 < 20_000) return false;
        return r !== "";
      },
      onTick: (s) => {
        if ((s as CertState | null)?.last_result === "issuing") {
          progress.value =
            method.value === "dns-01"
              ? "Проверочная запись опубликована, ждём удостоверяющий центр…"
              : "Домен подтверждается…";
        }
      },
    })) as CertState | null;

    result.value = final;
    progress.value = "";
    if (final?.last_result === "ok") {
      emit("done");
    } else {
      err.value =
        final?.last_error || "Роутер не смог выпустить сертификат — подробности в журнале";
    }
  } catch (e) {
    progress.value = "";
    err.value = e instanceof Error ? e.message : "Не удалось выпустить сертификат";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <DrawerSheet :open="open" title="Свой домен и защищённое соединение" @close="emit('close')">
    <div class="form">
      <p class="lead">
        Роутер получит бесплатный сертификат Let's Encrypt и станет открываться по
        защищённому соединению. Домен уже должен указывать на роутер; чем подтвердить
        владение — выберите ниже.
      </p>

      <div class="block">
        <span class="lbl">Как подтверждать владение</span>
        <SegmentedControl
          v-model="method"
          label="Как подтверждать владение"
          :options="methodOptions"
        />
        <p class="hint">{{ methodHint }}</p>
      </div>

      <FormField label="Домен" hint="Тот, что указывает на этот роутер">
        <input
          v-model="domain"
          type="text"
          placeholder="panel.example.com"
          autocomplete="off"
          spellcheck="false"
          :disabled="busy"
        />
      </FormField>

      <FormField
        label="Почта"
        hint="Туда придёт предупреждение об истечении срока. Можно оставить пустой"
      >
        <input
          v-model="email"
          type="email"
          placeholder="you@example.com"
          autocomplete="off"
          spellcheck="false"
          :disabled="busy"
        />
      </FormField>

      <template v-if="method === 'dns-01'">
        <div class="block">
          <span class="lbl">DNS-провайдер</span>
          <select v-model="provider" :disabled="busy || dnsBusy">
            <option v-for="p in dns?.providers ?? []" :key="p" :value="p">
              {{ PROVIDER_LABELS[p] ?? p }}
            </option>
          </select>
          <p v-if="pluginMissing" class="note warn">
            В acme.sh на роутере нет плагина {{ dns?.plugin }} — обновите acme.sh.
          </p>
        </div>

        <FormField
          v-for="v in dnsVars"
          :key="v.name"
          :label="labelFor(v.name)"
          :hint="
            isSecret(v.name) && storedSecret
              ? 'Уже сохранён — оставьте пустым, чтобы не менять'
              : hintFor(v.name)
          "
        >
          <input
            v-model="dnsValues[v.name]"
            :type="isSecret(v.name) ? 'password' : 'text'"
            :placeholder="isSecret(v.name) && storedSecret ? '••••••••' : ''"
            autocomplete="off"
            spellcheck="false"
            :disabled="busy || dnsBusy"
          />
        </FormField>

        <div class="row">
          <UiButton :busy="dnsBusy" :disabled="busy" @click="saveDns">Сохранить доступ</UiButton>
          <UiButton :disabled="busy || dnsBusy || !dnsReady" @click="checkDns">
            Проверить ключ
          </UiButton>
        </div>
        <p v-if="dnsNote" class="note ok">{{ dnsNote }}</p>
        <p v-if="dnsErr" class="note bad">{{ dnsErr }}</p>

        <FormField
          label="Дополнительные имена"
          :hint="
            altSuggestion
              ? `Через пробел. Wildcard тоже можно: ${altSuggestion} — тогда любое имя под ним заработает без перевыпуска`
              : 'Через пробел; допускается wildcard вида *.h.example.com'
          "
        >
          <input
            v-model="alts"
            type="text"
            :placeholder="altSuggestion"
            autocomplete="off"
            spellcheck="false"
            :disabled="busy"
          />
        </FormField>
      </template>

      <p v-if="!acmePresent" class="note warn">
        Средство выпуска на роутере ещё не установлено — оно поставится само при
        первом выпуске, если у роутера есть интернет.
      </p>
      <p v-if="http80Note" class="note warn">{{ http80Note }}</p>
      <p v-if="keenPort80Note" class="note warn">{{ keenPort80Note }}</p>
      <p v-if="wanNote" class="note">{{ wanNote }}</p>

      <p v-if="progress" class="note live">{{ progress }}</p>
      <p v-if="err" class="note bad">{{ err }}</p>
      <p v-if="result?.last_result === 'ok'" class="note ok">
        Сертификат выпущен{{ result.expiry ? `, действует до ${result.expiry}` : "" }}.
        Откройте панель по адресу https://{{ result.domain || domain }} — старый адрес
        тоже продолжит работать.
      </p>
    </div>

    <template #footer>
      <UiButton variant="primary" :busy="busy" @click="issue">
        {{ busy ? "Выпускаю…" : "Выпустить" }}
      </UiButton>
      <UiButton :disabled="busy" @click="emit('close')">Закрыть</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}
.lead {
  font-size: 13px;
  color: var(--dim);
}
.block {
  display: flex;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
}
.lbl {
  font-size: 12.5px;
  color: var(--dim);
}
.hint {
  font-size: 12px;
  color: var(--faint);
  overflow-wrap: anywhere;
}
.row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
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
.note.ok {
  color: var(--ok);
  border-color: color-mix(in srgb, var(--ok) 45%, transparent);
}
.note.live {
  color: var(--accent);
  border-color: var(--accent);
}
@media (max-width: 860px) {
  :deep(.btn) {
    min-height: 44px;
  }
  :deep(.seg) {
    width: 100%;
  }
  :deep(.seg button) {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 44px;
    white-space: normal;
  }
}
</style>
