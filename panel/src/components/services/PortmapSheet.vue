<script setup lang="ts">
/* Форма публикации домашнего сервиса наружу. Правка и создание — одна форма:
   различие только в том, откуда взяты начальные значения и можно ли менять
   идентификатор (он не меняется никогда — по нему бэкенд ищет строку). */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import FormField from "@/components/services/FormField.vue";
import { services } from "@/api";
import type { LanClient } from "@/api";

type Mode = "https" | "dnat";

/** Строка проброса так, как её реально отдаёт роутер. */
interface PortmapRow {
  id: string;
  name?: string;
  enabled: boolean;
  mode: Mode;
  listen_port: number;
  proto: string;
  target_ip: string;
  target_port: number;
  scheme?: string;
  src?: string;
  auth_user?: string;
  auth?: boolean;
  listening?: boolean;
}

const props = defineProps<{
  open: boolean;
  /** null — создаём новый. */
  entry: PortmapRow | null;
  /** Занятые идентификаторы: новый должен быть уникальным. */
  usedIds: string[];
  httpsSupported: boolean;
  httpsReason: string;
  dnatSupported: boolean;
  dnatReason: string;
  authSupported: boolean;
  authReason: string;
  /** Домен сертификата панели — по нему сервис откроется снаружи. */
  domain: string;
  clients: LanClient[];
}>();

const emit = defineEmits<{ close: []; saved: [] }>();

const name = ref("");
const mode = ref<Mode>("https");
const listenPort = ref("");
const proto = ref("tcp");
const targetIp = ref("");
const targetPort = ref("");
const scheme = ref("http");
const src = ref("any");
const authUser = ref("");
const authPass = ref("");
const busy = ref(false);
const err = ref("");

const isNew = computed(() => !props.entry);

const title = computed(() =>
  isNew.value ? "Новый доступ снаружи" : `Правка: ${props.entry?.name || props.entry?.id}`,
);

/* Режим по умолчанию — тот, который на этом роутере вообще возможен. */
function defaultMode(): Mode {
  if (props.httpsSupported) return "https";
  return "dnat";
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    err.value = "";
    authPass.value = "";
    const e = props.entry;
    name.value = e?.name ?? "";
    mode.value = e?.mode ?? defaultMode();
    listenPort.value = e ? String(e.listen_port) : "";
    proto.value = e?.proto || "tcp";
    targetIp.value = e?.target_ip ?? "";
    targetPort.value = e ? String(e.target_port) : "";
    scheme.value = e?.scheme || "http";
    src.value = e?.src || "any";
    authUser.value = e?.auth_user ?? "";
  },
);

const modeOptions = computed(() => [
  {
    value: "https" as Mode,
    label: "Через HTTPS",
    disabled: !props.httpsSupported,
    hint: props.httpsSupported ? undefined : props.httpsReason,
  },
  {
    value: "dnat" as Mode,
    label: "Проброс порта",
    disabled: !props.dnatSupported,
    hint: props.dnatSupported ? undefined : props.dnatReason,
  },
]);

const modeHint = computed(() =>
  mode.value === "https"
    ? `Роутер сам принимает защищённое соединение${
        props.domain ? ` на ${props.domain}` : ""
      } и передаёт запрос устройству. Подходит для веб-сервисов; пароль на вход тоже возможен.`
    : "Порт роутера отдаётся устройству как есть — для игр, SSH, удалённого рабочего стола. Шифрования роутер не добавляет.",
);

const canUseAuth = computed(() => mode.value === "https" && props.authSupported);

/* Список устройств — это подсказка, а не источник истины: адрес можно вписать
   и руками, тогда в списке просто ничего не выбрано. */
const clientPick = computed({
  get: () => (props.clients.some((c) => c.ip === targetIp.value) ? targetIp.value : ""),
  set: (v: string) => {
    if (v) targetIp.value = v;
  },
});

/* Идентификатор строки — только латиница, цифры, дефис. Из русского названия
   он не получится, поэтому для новых записей просто берём свободный номер. */
function makeId(): string {
  const slug = name.value
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 24);
  const used = new Set(props.usedIds);
  if (slug && !used.has(slug)) return slug;
  for (let i = 1; i < 999; i++) {
    const cand = `svc-${i}`;
    if (!used.has(cand)) return cand;
  }
  return `svc-${Date.now()}`;
}

function portOk(v: string): boolean {
  const n = Number(v);
  return Number.isInteger(n) && n >= 1 && n <= 65535;
}

async function save() {
  err.value = "";
  if (!portOk(listenPort.value)) {
    err.value = "Внешний порт должен быть числом от 1 до 65535";
    return;
  }
  if (!portOk(targetPort.value)) {
    err.value = "Порт устройства должен быть числом от 1 до 65535";
    return;
  }
  if (!/^\d{1,3}(\.\d{1,3}){3}$/.test(targetIp.value.trim())) {
    err.value = "Укажите адрес устройства в локальной сети, например 192.168.8.100";
    return;
  }
  if (canUseAuth.value && authUser.value.trim() && !authPass.value && !props.entry?.auth) {
    err.value = "Задайте пароль или очистите логин";
    return;
  }

  const id = props.entry?.id ?? makeId();
  busy.value = true;
  try {
    await services.portmapSave({
      id,
      name: name.value.trim(),
      enabled: props.entry ? props.entry.enabled : true,
      mode: mode.value,
      listen_port: Number(listenPort.value),
      proto: mode.value === "https" ? "tcp" : proto.value,
      target_ip: targetIp.value.trim(),
      target_port: Number(targetPort.value),
      scheme: mode.value === "https" ? scheme.value : "http",
      src: src.value,
    });

    /* Пароль хранится отдельным действием — роутер никогда не отдаёт хеш
       обратно, поэтому трогаем его, только если человек что-то ввёл. */
    if (canUseAuth.value) {
      const user = authUser.value.trim();
      const had = props.entry?.auth === true;
      if (!user && had) {
        await services.portmapAuth(id, "", "");
      } else if (user && authPass.value) {
        await services.portmapAuth(id, user, authPass.value);
      }
    }
    emit("saved");
    emit("close");
  } catch (e) {
    err.value = e instanceof Error ? e.message : "Не удалось сохранить";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <DrawerSheet :open="open" :title="title" @close="emit('close')">
    <div class="form">
      <FormField label="Название">
        <input
          v-model="name"
          type="text"
          placeholder="Домашний сервер"
          autocomplete="off"
        />
      </FormField>

      <div class="block">
        <span class="lbl">Как открывать</span>
        <SegmentedControl v-model="mode" label="Как открывать" :options="modeOptions" />
        <p class="hint">{{ modeHint }}</p>
        <p v-if="!httpsSupported" class="hint warn">
          Через HTTPS сейчас нельзя: {{ httpsReason || "нет подходящего веб-сервера" }}
        </p>
        <p v-if="!dnatSupported" class="hint warn">
          Проброс порта сейчас нельзя: {{ dnatReason || "недоступно на этом роутере" }}
        </p>
      </div>

      <div class="grid">
        <FormField label="Порт снаружи" hint="По нему сервис будет виден в интернете">
          <input
            v-model="listenPort"
            type="number"
            inputmode="numeric"
            min="1"
            max="65535"
            placeholder="8443"
          />
        </FormField>

        <FormField v-if="mode === 'dnat'" label="Протокол">
          <select v-model="proto">
            <option value="tcp">TCP</option>
            <option value="udp">UDP</option>
            <option value="tcp udp">TCP и UDP</option>
          </select>
        </FormField>
      </div>

      <FormField v-if="clients.length" label="Устройство в сети">
        <select v-model="clientPick">
          <option value="">Выбрать из списка…</option>
          <option v-for="c in clients" :key="c.ip" :value="c.ip">
            {{ c.host ? `${c.host} — ${c.ip}` : c.ip }}
          </option>
        </select>
      </FormField>

      <div class="grid">
        <FormField label="Адрес устройства">
          <input
            v-model="targetIp"
            type="text"
            inputmode="decimal"
            placeholder="192.168.8.100"
            autocomplete="off"
            spellcheck="false"
          />
        </FormField>
        <FormField label="Порт устройства">
          <input
            v-model="targetPort"
            type="number"
            inputmode="numeric"
            min="1"
            max="65535"
            placeholder="5000"
          />
        </FormField>
      </div>

      <div class="grid">
        <FormField v-if="mode === 'https'" label="Устройство отвечает по">
          <select v-model="scheme">
            <option value="http">HTTP</option>
            <option value="https">HTTPS (самоподписанный подойдёт)</option>
          </select>
        </FormField>
        <FormField label="Кого пускать">
          <select v-model="src">
            <option value="any">Кого угодно из интернета</option>
            <option value="lan">Только свою локальную сеть</option>
          </select>
        </FormField>
      </div>

      <div v-if="mode === 'https'" class="block">
        <span class="lbl">Пароль на вход</span>
        <p v-if="!authSupported" class="hint warn">
          {{ authReason || "На этом роутере пароль на вход недоступен" }}
        </p>
        <template v-else>
          <div class="grid">
            <FormField label="Логин" hint="Пусто — входить сможет кто угодно">
              <input
                v-model="authUser"
                type="text"
                placeholder="без пароля"
                autocomplete="off"
              />
            </FormField>
            <FormField
              label="Пароль"
              :hint="entry?.auth ? 'Пусто — оставить прежний' : undefined"
            >
              <input v-model="authPass" type="password" autocomplete="new-password" />
            </FormField>
          </div>
          <p class="hint">
            Пароль спрашивает сам роутер — до устройства запрос без него не дойдёт.
          </p>
        </template>
      </div>

      <p v-if="err" class="err">{{ err }}</p>
    </div>

    <template #footer>
      <UiButton variant="primary" :busy="busy" @click="save">Сохранить</UiButton>
      <UiButton :disabled="busy" @click="emit('close')">Отмена</UiButton>
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
.grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  min-width: 0;
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
.hint.warn {
  color: var(--warn);
}
@media (max-width: 860px) {
  /* Палец, а не курсор: кнопки и сегменты режима — не меньше 44 px. */
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
.err {
  font-size: 13px;
  color: var(--bad);
  border: 1px solid color-mix(in srgb, var(--bad) 45%, transparent);
  background: color-mix(in srgb, var(--bad) 10%, transparent);
  border-radius: var(--radius-sm);
  padding: 9px 11px;
  overflow-wrap: anywhere;
}
</style>
