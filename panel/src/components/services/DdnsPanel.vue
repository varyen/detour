<script setup lang="ts">
/* Бесплатное имя для внешнего адреса роутера и сертификат на него.

   Провайдеры подобраны так, чтобы сертификат выпускался без открытого 80-го
   порта: тот же ключ, которым роутер обновляет адрес, ставит и проверочную
   TXT-запись Let's Encrypt. Поэтому имя полезно даже за CGNAT — с галочкой «в
   домашней сети» оно ведёт прямо на роутер, и панель открывается по HTTPS
   с настоящим сертификатом. */
import { computed, onMounted, ref, watch } from "vue";
import ServicePanel from "@/components/services/ServicePanel.vue";
import FormField from "@/components/services/FormField.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { poll, services } from "@/api";
import type { DdnsProvider, DdnsStatus } from "@/api";
import { fmtAgo } from "@/lib/format";
import { useToastStore } from "@/stores/toast";

const open = defineModel<boolean>("open", { required: true });
const emit = defineEmits<{ cert: [] }>();
const props = defineProps<{ certEmail?: string }>();
const toast = useToastStore();

const st = ref<DdnsStatus | null>(null);
const busy = ref("");
const err = ref("");
const progress = ref("");

const provider = ref<DdnsProvider>("duckdns");
const name = ref("");
const token = ref("");
const lan = ref(true);
const wildcard = ref(false);
const email = ref("");

const PROVIDERS: Record<DdnsProvider, { label: string; suffix: string; site: string; how: string }> = {
  duckdns: {
    label: "DuckDNS",
    suffix: "duckdns.org",
    site: "https://www.duckdns.org/",
    how: "Войдите на duckdns.org (через GitHub или Google), заведите имя и скопируйте token со страницы.",
  },
  desec: {
    label: "deSEC",
    suffix: "dedyn.io",
    site: "https://desec.io/",
    how: "Зарегистрируйтесь на desec.io по почте, создайте имя вида <имя>.dedyn.io и скопируйте токен из «Token Management».",
  },
};

async function load() {
  st.value = await services.ddnsStatus();
  const s = st.value;
  if (!s) return;
  if (s.provider) provider.value = s.provider;
  const suf = `.${PROVIDERS[provider.value].suffix}`;
  name.value = s.domain && s.domain.endsWith(suf) ? s.domain.slice(0, -suf.length) : (s.domain ?? "");
  lan.value = s.lan !== false;
  wildcard.value = s.wildcard === true;
  token.value = "";
}

/* Свёрнутой области тоже нужна сводка — грузим сразу, при раскрытии освежаем. */
onMounted(() => void load());
watch(open, (o) => {
  if (o) {
    email.value = props.certEmail ?? "";
    void load();
  }
});

const providerOptions = computed(() =>
  (Object.keys(PROVIDERS) as DdnsProvider[]).map((p) => ({ value: p, label: PROVIDERS[p].label })),
);

const sameProvider = computed(() => st.value?.provider === provider.value);
const tokenStored = computed(() => sameProvider.value && st.value?.token_set === true);
const fullName = computed(() => {
  const n = name.value.trim().toLowerCase();
  if (!n) return "";
  return n.includes(".") ? n : `${n}.${PROVIDERS[provider.value].suffix}`;
});
const enabled = computed(() => st.value?.enabled === true);
const configured = computed(() => !!st.value?.domain && st.value?.token_set === true);

const summary = computed(() => {
  const s = st.value;
  if (!s?.domain) return "Своё имя вместо адреса — бесплатно, с сертификатом";
  if (!s.enabled) return `${s.domain} — выключено`;
  if (s.last_result === "error") return `${s.domain} — ${s.last_error || "ошибка"}`;
  return s.last_ip ? `${s.domain} → ${s.last_ip}` : s.domain;
});
const chip = computed(() => {
  const s = st.value;
  if (!s?.enabled) return s?.domain ? "выключено" : "";
  if (s.last_result === "error") return "ошибка";
  return s.last_result === "ok" ? "работает" : "";
});
const tone = computed<"ok" | "warn" | "bad" | undefined>(() => {
  const s = st.value;
  if (!s?.enabled) return undefined;
  if (s.last_result === "error") return "bad";
  return s.last_result === "ok" ? "ok" : undefined;
});

const certCovers = computed(() => !!st.value?.domain && st.value?.cert_domain === st.value?.domain);

async function save(enable = true) {
  err.value = "";
  if (!fullName.value) {
    err.value = "Укажите имя";
    return;
  }
  if (!token.value.trim() && !tokenStored.value) {
    err.value = "Укажите токен";
    return;
  }
  busy.value = "save";
  try {
    const r = await services.ddnsSet({
      provider: provider.value,
      domain: fullName.value,
      ...(token.value.trim() ? { token: token.value.trim() } : {}),
      lan: lan.value,
      wildcard: provider.value === "desec" && wildcard.value,
      enabled: enable,
    });
    toast.ok(r.ip ? `${fullName.value} → ${r.ip}` : "Сохранено");
  } catch (e) {
    err.value = e instanceof Error ? e.message : "Не удалось сохранить";
  } finally {
    busy.value = "";
    await load();
  }
}

async function updateNow() {
  err.value = "";
  busy.value = "update";
  try {
    const r = await services.ddnsUpdate();
    toast.ok(`Адрес отправлен: ${r.ip ?? ""}`);
  } catch (e) {
    err.value = e instanceof Error ? e.message : "Не удалось обновить";
  } finally {
    busy.value = "";
    await load();
  }
}

async function turnOff() {
  busy.value = "off";
  try {
    await services.ddnsOff();
  } catch (e) {
    toast.fromError(e, "Не удалось выключить");
  } finally {
    busy.value = "";
    await load();
  }
}

async function forget() {
  if (!confirm("Забыть имя и токен? Само имя у провайдера останется — удалить его можно на его сайте.")) return;
  busy.value = "delete";
  try {
    await services.ddnsDelete();
  } catch (e) {
    toast.fromError(e, "Не удалось удалить");
  } finally {
    busy.value = "";
    await load();
  }
}

async function issueCert() {
  err.value = "";
  const m = email.value.trim();
  if (m && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(m)) {
    err.value = "Почта указана неверно";
    return;
  }
  const other = st.value?.cert_domain;
  if (
    other &&
    other !== st.value?.domain &&
    !confirm(`Сейчас у роутера сертификат на ${other}. Новый на ${st.value?.domain} его заменит — продолжить?`)
  )
    return;
  busy.value = "cert";
  progress.value = "Ставлю проверочную запись и жду удостоверяющий центр — до нескольких минут";
  try {
    await services.ddnsCert(m || "-");
    /* Пока роутер не отметил начало, в состоянии лежит результат прошлого
       выпуска — первые секунды его не принимаем за свой. */
    let started = false;
    const t0 = Date.now();
    const final = (await poll(() => services.certStatus(), {
      intervalMs: 3000,
      timeoutMs: 600_000,
      done: (s) => {
        const r = (s as { last_result?: string } | null)?.last_result ?? "";
        if (r === "issuing") {
          started = true;
          return false;
        }
        if (!started && Date.now() - t0 < 20_000) return false;
        return r !== "";
      },
    })) as { last_result?: string; last_error?: string } | null;
    if (final?.last_result === "ok") {
      toast.ok(`Сертификат на ${st.value?.domain} выпущен`);
      emit("cert");
    } else {
      err.value = final?.last_error || "Сертификат выпустить не удалось — подробности в журнале";
    }
  } catch (e) {
    err.value = e instanceof Error ? e.message : "Не удалось выпустить сертификат";
  } finally {
    progress.value = "";
    busy.value = "";
    await load();
  }
}
</script>

<template>
  <ServicePanel
    id="svc-ddns"
    v-model:open="open"
    icon="dns"
    title="Бесплатное имя (DynDNS)"
    :summary="summary"
    :chip="chip"
    :tone="tone"
  >
    <p class="lead">
      Имя вида <b>myhome.duckdns.org</b> всегда указывает на внешний адрес роутера,
      даже когда провайдер его меняет. На это имя роутер сам получит сертификат
      Let's Encrypt — без открытого 80-го порта.
    </p>

    <p v-if="st && st.supported === false" class="note warn">
      На роутере нет detour-ddns — обновите Detour.
    </p>

    <template v-else-if="st">
      <p v-if="st.curl === false" class="note warn">На роутере нет curl — имя обновлять нечем.</p>
      <p v-if="st.enabled && st.cgnat" class="note warn">
        Роутер за чужим NAT: адрес канала {{ st.iface_ip }}, а снаружи виден
        {{ st.last_ip }}. Имя будет указывать на адрес провайдера, и снаружи до роутера
        по нему не достучаться. Сертификат всё равно выпустится, а в домашней сети имя
        заработает, если включить «в домашней сети».
      </p>
      <p v-if="st.last_result === 'error' && st.last_error" class="note bad">
        {{ st.last_error }}
      </p>
      <p v-else-if="st.enabled && st.last_result === 'ok'" class="note">
        {{ st.domain }} → {{ st.last_ip }}<template v-if="st.last_ts">, отправлено {{ fmtAgo(st.last_ts) }}</template>.
        <template v-if="st.via_vpn"> Напрямую провайдер не пустил к {{ st.provider === "desec" ? "deSEC" : "DuckDNS" }} — адрес ушёл через VPN.</template>
        Роутер проверяет адрес каждые 5 минут.
      </p>

      <div class="block">
        <span class="lbl">Провайдер</span>
        <SegmentedControl v-model="provider" label="Провайдер" :options="providerOptions" />
        <p class="hint">
          {{ PROVIDERS[provider].how }}
          <a :href="PROVIDERS[provider].site" target="_blank" rel="noopener">{{ PROVIDERS[provider].site }}</a>
        </p>
      </div>

      <FormField label="Имя" :hint="fullName ? `Полностью: ${fullName}` : undefined">
        <input
            v-model="name"
            type="text"
            :placeholder="`myhome — станет myhome.${PROVIDERS[provider].suffix}`"
            autocomplete="off"
            spellcheck="false"
            :disabled="!!busy"
          />
      </FormField>

      <FormField
        label="Токен"
        :hint="tokenStored ? 'Уже сохранён — оставьте пустым, чтобы не менять' : undefined"
      >
        <input
          v-model="token"
          type="password"
          :placeholder="tokenStored ? '••••••••' : ''"
          autocomplete="off"
          spellcheck="false"
          :disabled="!!busy"
        />
      </FormField>

      <SwitchToggle
        v-if="!st.box"
        v-model="lan"
        label="В домашней сети — прямо на роутер"
        :hint="`Устройства дома получат ${st.lan_ip || 'адрес роутера'} вместо внешнего: панель откроется по имени и с закрытым снаружи портом`"
        :disabled="!!busy"
      />
      <SwitchToggle
        v-if="provider === 'desec'"
        v-model="wildcard"
        label="Сертификат и на поддомены"
        :hint="`*.${fullName || 'имя.dedyn.io'} — для публикации сервисов по имени`"
        :disabled="!!busy"
      />
      <p v-else class="hint">
        DuckDNS держит одну проверочную запись на имя, поэтому сертификат — только на
        само имя, без поддоменов. Нужен wildcard — берите deSEC.
      </p>

      <div class="actions">
        <UiButton variant="primary" :busy="busy === 'save'" :disabled="!!busy && busy !== 'save'" @click="save(true)">
          {{ enabled ? "Сохранить" : "Включить" }}
        </UiButton>
        <UiButton v-if="enabled" :busy="busy === 'update'" :disabled="!!busy" @click="updateNow">
          Отправить адрес сейчас
        </UiButton>
        <UiButton v-if="enabled" :busy="busy === 'off'" :disabled="!!busy" @click="turnOff">Выключить</UiButton>
        <UiButton v-if="configured" :busy="busy === 'delete'" :disabled="!!busy" @click="forget">Забыть</UiButton>
      </div>

      <template v-if="configured">
        <div class="cert">
          <span class="lbl">Сертификат на {{ st.domain }}</span>
          <p v-if="certCovers" class="note ok">
            Уже выпущен — панель открывается по https://{{ st.domain }}. Продление само.
          </p>
          <FormField
            label="Почта для Let's Encrypt"
            hint="Туда придёт предупреждение, если продление не удастся. Можно оставить пустой, если сертификат уже выпускался"
          >
            <input
              v-model="email"
              type="email"
              placeholder="you@example.com"
              autocomplete="off"
              spellcheck="false"
              :disabled="!!busy"
            />
          </FormField>
          <div class="actions">
            <UiButton :busy="busy === 'cert'" :disabled="!!busy" @click="issueCert">
              {{ certCovers ? "Выпустить заново" : "Получить сертификат" }}
            </UiButton>
          </div>
          <p v-if="progress" class="note live">{{ progress }}</p>
        </div>
      </template>

      <p v-if="err" class="note bad">{{ err }}</p>
    </template>
  </ServicePanel>
</template>

<style scoped>
.lead {
  font-size: 13px;
  color: var(--dim);
}
.block,
.cert {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.cert {
  border-top: 1px solid var(--line);
  padding-top: 12px;
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
.hint a {
  color: var(--accent);
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
.actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
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
  }
}
</style>
