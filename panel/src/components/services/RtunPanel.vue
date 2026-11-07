<script setup lang="ts">
/* Встречный канал: роутер за CGNAT (мобильный интернет) сам держит туннель
   AmneziaWG к своему VPN-серверу Detour на VPS, а сервер публикует его порты.
   Здесь — конфиг клиента с сервера и перевод пришедших портов на устройства
   домашней сети. Порт на сервере и порт здесь совпадают. */
import { computed, onMounted, ref, watch } from "vue";
import ServicePanel from "@/components/services/ServicePanel.vue";
import UiButton from "@/components/UiButton.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { diag, overview, poll, services } from "@/api";
import type { ApplyLogResponse, PubProto, RtunMap, RtunStatus } from "@/api";
import { fmtBytes } from "@/lib/format";
import { useToastStore } from "@/stores/toast";

const open = defineModel<boolean>("open", { required: true });
const toast = useToastStore();

const st = ref<RtunStatus | null>(null);
const busy = ref("");
const installLog = ref("");
const confText = ref("");
const showImport = ref(false);
const fileInput = ref<HTMLInputElement | null>(null);

const mProto = ref<PubProto>("tcp");
const mPort = ref("");
const mIp = ref("");
const mDport = ref("");
const mNote = ref("");
const devices = ref<{ ip: string; name: string }[]>([]);

const PROTO_OPTIONS = [
  { value: "tcp" as PubProto, label: "TCP" },
  { value: "udp" as PubProto, label: "UDP" },
  { value: "both" as PubProto, label: "Оба" },
];
const protoName = (p: PubProto) => (p === "both" ? "TCP и UDP" : p.toUpperCase());

async function load() {
  st.value = await services.rtunStatus();
}
async function loadDevices() {
  try {
    const r = await overview.lanClients();
    devices.value = (r.clients ?? [])
      .filter((c) => c.ip)
      .map((c) => ({ ip: c.ip, name: c.host ?? "" }));
  } catch {
    devices.value = [];
  }
}

onMounted(() => void load());
watch(open, (o) => {
  if (o) {
    void load();
    void loadDevices();
  }
});

const online = computed(() => {
  const a = st.value?.handshake_ago ?? -1;
  return !!st.value?.up && a >= 0 && a < 180;
});
const summary = computed(() => {
  const s = st.value;
  if (!s || s.supported === false) return "Порты дома через свой сервер — для мобильного интернета";
  if (!s.configured) return "Порты дома через свой сервер — для мобильного интернета";
  if (!s.enabled) return `Выключен · сервер ${s.server}`;
  return online.value ? `Связь с ${s.server} есть · переводов: ${s.maps?.length ?? 0}` : `Нет связи с ${s.server}`;
});
const chip = computed(() => {
  const s = st.value;
  if (!s?.configured) return "";
  if (!s.enabled) return "выключен";
  return online.value ? "работает" : "нет связи";
});
const tone = computed<"ok" | "warn" | "bad" | undefined>(() => {
  const s = st.value;
  if (!s?.configured || !s.enabled) return undefined;
  return online.value ? "ok" : "bad";
});

async function install() {
  busy.value = "install";
  installLog.value = "";
  try {
    await services.rtunInstall();
    const r = await poll<ApplyLogResponse | null>(() => diag.applyLog(), {
      done: (v) => v?.done === true,
      intervalMs: 2000,
      timeoutMs: 300_000,
      onTick: (v) => {
        if (v && typeof v.log === "string") installLog.value = v.log;
      },
    });
    if (r?.done && Number(r.rc) === 0) toast.ok("AmneziaWG установлен");
    else toast.error("Установка не удалась — подробности в журнале ниже");
  } catch (e) {
    toast.fromError(e, "Не удалось установить AmneziaWG");
  } finally {
    busy.value = "";
    await load();
  }
}

async function pickFile(ev: Event) {
  const f = (ev.target as HTMLInputElement).files?.[0];
  if (!f) return;
  confText.value = await f.text();
  (ev.target as HTMLInputElement).value = "";
}

async function doImport() {
  if (!confText.value.includes("[Interface]")) {
    toast.error("Это не конфиг AmneziaWG — нужен файл .conf клиента с сервера");
    return;
  }
  busy.value = "import";
  try {
    await services.rtunImport(confText.value);
    toast.ok("Конфиг загружен, туннель поднят");
    confText.value = "";
    showImport.value = false;
  } catch (e) {
    toast.fromError(e, "Не удалось загрузить конфиг");
  } finally {
    busy.value = "";
    await load();
  }
}

async function toggle(on: boolean) {
  busy.value = "toggle";
  try {
    await services.rtunSet(on);
  } catch (e) {
    toast.fromError(e, "Не удалось переключить");
  } finally {
    busy.value = "";
    await load();
  }
}

async function addMap() {
  const port = Number(mPort.value);
  const dport = mDport.value.trim() ? Number(mDport.value) : port;
  if (!Number.isInteger(port) || port < 1 || port > 65535 || !Number.isInteger(dport) || dport < 1 || dport > 65535) {
    toast.error("Порты — числа от 1 до 65535");
    return;
  }
  if (!/^\d{1,3}(\.\d{1,3}){3}$/.test(mIp.value.trim())) {
    toast.error("Адрес устройства — IPv4 домашней сети");
    return;
  }
  busy.value = "add";
  try {
    await services.rtunMapAdd({ proto: mProto.value, port, ip: mIp.value.trim(), dport, note: mNote.value.trim() });
    toast.ok(`Порт ${port} ведёт на ${mIp.value.trim()}:${dport}`);
    mPort.value = "";
    mDport.value = "";
    mNote.value = "";
  } catch (e) {
    toast.fromError(e, "Не удалось добавить");
  } finally {
    busy.value = "";
    await load();
  }
}

async function toggleMap(m: RtunMap) {
  busy.value = m.id;
  try {
    await services.rtunMapSet(m.id, { enabled: !m.enabled });
  } catch (e) {
    toast.fromError(e, "Не удалось переключить");
  } finally {
    busy.value = "";
    await load();
  }
}

async function delMap(m: RtunMap) {
  if (!confirm(`Убрать перевод порта ${m.port}?`)) return;
  busy.value = m.id;
  try {
    await services.rtunMapDel(m.id);
  } catch (e) {
    toast.fromError(e, "Не удалось удалить");
  } finally {
    busy.value = "";
    await load();
  }
}

async function forget() {
  if (!confirm("Отключить встречный канал и забыть конфиг и переводы портов?")) return;
  busy.value = "forget";
  try {
    await services.rtunForget();
  } catch (e) {
    toast.fromError(e, "Не удалось");
  } finally {
    busy.value = "";
    await load();
  }
}

function deviceName(ip: string) {
  if (ip === st.value?.lan_ip) return "сам роутер";
  return devices.value.find((d) => d.ip === ip)?.name ?? "";
}
</script>

<template>
  <ServicePanel
    id="svc-rtun"
    v-model:open="open"
    icon="link"
    title="Встречный канал через свой сервер"
    :summary="summary"
    :chip="chip"
    :tone="tone"
  >
    <p class="lead">
      На мобильном интернете роутер стоит за NAT оператора, и снаружи до него не
      достучаться — проброс портов не работает. Встречный канал решает это: роутер
      сам подключается к вашему VPN-серверу Detour на VPS, а порт сервера ведёт на
      устройство дома.
    </p>

    <p v-if="st && st.supported === false" class="note warn">
      {{ st.reason === "keenetic" ? "На Keenetic встречный канал пока не поддерживается." : "Обновите Detour — в этой версии встречного канала нет." }}
    </p>

    <template v-else-if="st">
      <template v-if="st.reason === 'not_installed'">
        <p class="note">Нужен AmneziaWG — его поставит роутер (модуль ядра или userspace из фида Detour).</p>
        <div class="actions">
          <UiButton variant="primary" :busy="busy === 'install'" @click="install">Установить AmneziaWG</UiButton>
        </div>
        <pre v-if="installLog" class="log">{{ installLog }}</pre>
      </template>

      <template v-else>
        <ol v-if="!st.configured" class="steps">
          <li>На сервере: «Свой VPN-сервер» → добавьте клиента для этого роутера (режим «Только дом»).</li>
          <li>В его карточке «Подключение и QR» → «AmneziaWG» → скачайте конфиг и загрузите его сюда.</li>
          <li>Там же, на вкладке «Порты», опубликуйте порт — и переведите его здесь на устройство.</li>
        </ol>

        <template v-if="st.configured">
          <SwitchToggle
            :model-value="!!st.enabled"
            label="Канал включён"
            :hint="`Сервер ${st.server} · адрес в туннеле ${st.address}`"
            :busy="busy === 'toggle'"
            @update:model-value="toggle"
          />
          <p v-if="st.enabled && online" class="note ok">
            Связь есть: рукопожатие {{ st.handshake_ago }} с назад · ↓ {{ fmtBytes(st.rx ?? 0) }} · ↑ {{ fmtBytes(st.tx ?? 0) }}
          </p>
          <p v-else-if="st.enabled" class="note bad">
            Сервер не отвечает. Проверьте, что VPN-сервер на VPS включён, а клиент для
            этого роутера не выключен; роутер пробует переподключиться сам каждую минуту.
          </p>

          <span class="lbl">Перевод портов</span>
          <ul v-if="st.maps?.length" class="maps">
            <li v-for="m in st.maps" :key="m.id">
              <div class="m-main">
                <span class="mono">:{{ m.port }} → {{ m.ip }}:{{ m.dport }}</span>
                <span class="dim">{{ protoName(m.proto) }}{{ deviceName(m.ip) ? ` · ${deviceName(m.ip)}` : "" }}{{ m.note ? ` · ${m.note}` : "" }}</span>
              </div>
              <div class="actions">
                <UiButton :busy="busy === m.id" @click="toggleMap(m)">{{ m.enabled ? "Выключить" : "Включить" }}</UiButton>
                <UiButton variant="danger" :disabled="!!busy" @click="delMap(m)">Удалить</UiButton>
              </div>
            </li>
          </ul>
          <p v-else class="note">
            Переводов пока нет. Порт должен совпадать с опубликованным на сервере.
          </p>

          <div class="form">
            <SegmentedControl v-model="mProto" label="Протокол" :options="PROTO_OPTIONS" />
            <div class="row3">
              <input v-model="mPort" type="text" inputmode="numeric" placeholder="Порт сервера" aria-label="Порт сервера" />
              <input v-model="mIp" type="text" list="rtun-devices" placeholder="IP устройства" aria-label="IP устройства" />
              <input v-model="mDport" type="text" inputmode="numeric" placeholder="Порт устройства" aria-label="Порт устройства" />
            </div>
            <datalist id="rtun-devices">
              <option v-if="st.lan_ip" :value="st.lan_ip">сам роутер</option>
              <option v-for="d in devices" :key="d.ip" :value="d.ip">{{ d.name }}</option>
            </datalist>
            <input v-model="mNote" type="text" placeholder="Заметка (например, Home Assistant)" aria-label="Заметка" />
            <div class="actions">
              <UiButton variant="primary" :busy="busy === 'add'" @click="addMap">Добавить перевод</UiButton>
            </div>
          </div>
        </template>

        <div class="actions">
          <UiButton :variant="st.configured ? undefined : 'primary'" @click="showImport = !showImport">
            {{ st.configured ? "Заменить конфиг" : "Загрузить конфиг клиента" }}
          </UiButton>
          <UiButton v-if="st.configured" variant="danger" :busy="busy === 'forget'" @click="forget">Отключить и забыть</UiButton>
        </div>

        <div v-if="showImport" class="form">
          <textarea
            v-model="confText"
            rows="8"
            spellcheck="false"
            placeholder="[Interface]&#10;PrivateKey = …&#10;Address = …&#10;&#10;[Peer]&#10;…"
            aria-label="Конфиг AmneziaWG"
          ></textarea>
          <input ref="fileInput" type="file" accept=".conf,text/plain" hidden @change="pickFile" />
          <div class="actions">
            <UiButton @click="fileInput?.click()">Выбрать файл .conf</UiButton>
            <UiButton variant="primary" :busy="busy === 'import'" :disabled="!confText.trim()" @click="doImport">
              Загрузить
            </UiButton>
          </div>
        </div>
      </template>
    </template>
  </ServicePanel>
</template>

<style scoped>
.lead {
  font-size: 13px;
  color: var(--dim);
}
.lbl {
  font-size: 12.5px;
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
.steps {
  margin: 0;
  padding-left: 20px;
  font-size: 13px;
  color: var(--dim);
  display: grid;
  gap: 4px;
}
.actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.maps {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 8px;
}
.maps li {
  display: grid;
  gap: 8px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px 11px;
  min-width: 0;
}
.m-main {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  align-items: baseline;
  overflow-wrap: anywhere;
}
.mono {
  font-family: var(--mono);
  font-size: 12.5px;
}
.dim {
  font-size: 12px;
  color: var(--faint);
}
.form {
  display: grid;
  gap: 8px;
  min-width: 0;
}
.row3 {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr) minmax(0, 1fr);
  gap: 8px;
}
.row3 input,
.form > input,
textarea {
  min-width: 0;
  width: 100%;
}
textarea {
  font-family: var(--mono);
  font-size: 12px;
}
.log {
  font-family: var(--mono);
  font-size: 11.5px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 220px;
  overflow: auto;
  background: var(--panel-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px;
}
@media (max-width: 560px) {
  .row3 {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }
  .row3 input:nth-child(2) {
    grid-column: 1 / -1;
    grid-row: 1;
  }
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
