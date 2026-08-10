<script setup lang="ts">
/* Выпуск сертификата. Роутер отвечает сразу «начал» и уходит работать в фон:
   получить письмо от удостоверяющего центра, подтвердить домен и переложить
   файлы занимает от десяти секунд до минуты. Поэтому здесь не ожидание ответа,
   а опрос состояния с показом того, что происходит. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import FormField from "@/components/services/FormField.vue";
import { poll, services } from "@/api";

/** Состояние выпуска так, как его отдаёт роутер. */
interface CertState {
  domain?: string;
  target?: string;
  expiry?: string;
  last_result?: string;
  last_error?: string;
}

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
}>();

const emit = defineEmits<{ close: []; done: [] }>();

const domain = ref("");
const email = ref("");
const busy = ref(false);
const progress = ref("");
const err = ref("");
const result = ref<CertState | null>(null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    domain.value = props.domain;
    email.value = props.email;
    err.value = "";
    progress.value = "";
    result.value = null;
  },
);

const wanNote = computed(() => {
  if (!props.wanIp) return "";
  if (/^(10\.|127\.|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.)/.test(props.wanIp)) {
    return `Внешний адрес роутера — ${props.wanIp}, он приватный: роутер стоит за чужим NAT, и удостоверяющий центр до него не достучится.`;
  }
  return `Внешний адрес роутера: ${props.wanIp}`;
});

const http80Note = computed(() => {
  if (!props.http80 || props.http80 === "none") {
    return "На 80-м порту роутера сейчас никто не отвечает — подтвердить владение доменом не выйдет, пока порт 80 не открыт снаружи.";
  }
  return "";
});

/* Порядок подсказок про 80-й порт важен: «никто не отвечает» — жёсткая
   блокировка выпуска, и говорить про проброс поверх неё нечего. Поэтому
   keenetic-подсказка показывается только когда порт вообще отвечает. */
const keenPort80Note = computed(() => {
  if (!props.keenetic || http80Note.value) return "";
  const target = props.panelPort ? `на порт панели (${props.panelPort})` : "на порт панели";
  return `На этом роутере 80-й порт занят его собственным веб-интерфейсом. Чтобы удостоверяющий центр попал именно в панель, проброс порта 80 из интернета должен вести ${target}, а не в веб-интерфейс роутера.`;
});

function valid(): string {
  const d = domain.value.trim();
  const m = email.value.trim();
  if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/i.test(d)) {
    return "Домен указан неверно — ожидается что-то вроде panel.example.com";
  }
  if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(m)) {
    return "Почта указана неверно — на неё придёт предупреждение об истечении срока";
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
    await services.certIssue(domain.value.trim(), email.value.trim());
    progress.value = "Подтверждаю домен — это занимает до минуты";

    /* Пока роутер не отметил, что начал, в состоянии лежит результат прошлого
       выпуска. Принять его за свой — значит соврать об успехе, поэтому первые
       секунды старый ответ игнорируется. */
    let started = false;
    const t0 = Date.now();
    const final = (await poll(() => services.certStatus(), {
      intervalMs: 3000,
      timeoutMs: 300_000,
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
          progress.value = "Домен подтверждается…";
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
        защищённому соединению. Для этого домен уже должен указывать на роутер, а
        80-й порт — быть доступен из интернета: именно так удостоверяющий центр
        проверяет, что домен ваш.
      </p>

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

      <FormField label="Почта" hint="Туда придёт предупреждение, если срок истекает">
        <input
          v-model="email"
          type="email"
          placeholder="you@example.com"
          autocomplete="off"
          spellcheck="false"
          :disabled="busy"
        />
      </FormField>

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
}
</style>
