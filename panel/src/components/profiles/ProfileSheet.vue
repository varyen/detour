<script setup lang="ts">
/* Форма профиля: создание и правка. Поля общие для всех протоколов сверху,
   дальше — то, что нужно конкретному типу. Ссылку (vless://, trojan://, ss://,
   hysteria2://…) можно вставить целиком: её разбирает parseShareLink, и дальше
   человек правит обычные поля, а не URI.

   Обратный путь тоже есть: кнопка рядом собирает из полей ссылку для
   стороннего клиента (у wireguard — .conf, ссылок для него не бывает) и кладёт
   её в буфер. Текст показываем всегда: без HTTPS буфер обмена браузеру
   недоступен, и выделить руками — единственный оставшийся способ.

   WireGuard и AmneziaWG заводятся из .conf (вставить текст или выбрать файл)
   и из ключа Amnezia vpn://. AmneziaWG на роутере работает через сайдкар
   mihomo — если его нет, форма предлагает поставить. */
import { computed, nextTick, ref, watch } from "vue";
import { awg as awgApi, poll, services } from "@/api";
import type { AwgStatus, EngineConfig } from "@/api";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import PField from "@/components/profiles/PField.vue";
import {
  NO_UTLS,
  PROTO_TYPES,
  SS_METHODS,
  buildShareLink,
  buildWireguardConf,
  emptyDraft,
  isWgType,
  parseShareLink,
  parseWgConf,
  slugify,
} from "@/components/profiles/uri";
import { confFromAmneziaKey } from "@/components/profiles/awg";
import type { ProfileDraft } from "@/components/profiles/uri";
import { copyText } from "@/lib/clipboard";

const props = defineProps<{
  open: boolean;
  /** null — создаём новый профиль. */
  draft: ProfileDraft | null;
  busy?: boolean;
  groups: string[];
}>();

const emit = defineEmits<{
  close: [];
  save: [payload: { draft: ProfileDraft; activate: boolean }];
}>();

const d = ref<ProfileDraft>(emptyDraft());
const link = ref("");
const linkNote = ref("");
const activateNow = ref(false);
const isNew = ref(true);
/** Собранная ссылка/конфиг — показываем после нажатия «Скопировать». */
const shareShown = ref(false);
const shareEl = ref<HTMLTextAreaElement | null>(null);
/** Идентификатор правили руками — больше не подставляем его из имени. */
const idTouched = ref(false);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    d.value = props.draft ? { ...props.draft } : emptyDraft();
    isNew.value = !props.draft;
    link.value = "";
    linkNote.value = "";
    shareShown.value = false;
    activateNow.value = false;
    idTouched.value = false;
  },
);

/* Пока идентификатор не правили руками, он идёт следом за именем — так профиль
   получает понятное имя файла. У существующего профиля id не трогаем вовсе:
   это имя файла на роутере, переименование сломало бы ссылки из цепочек. */
watch(
  () => d.value.name,
  (name) => {
    if (isNew.value && !idTouched.value) d.value.id = name.trim() ? slugify(name) : "";
  },
);

const type = computed(() => d.value.type);
const isV2ray = computed(() => ["vless", "vmess", "trojan"].includes(type.value));
const isProxy = computed(() => type.value === "socks" || type.value === "http");
const isWg = computed(() => isWgType(type.value));
const isAwg = computed(() => type.value === "amneziawg");

/* Состояние сайдкара mihomo: без него AWG-профиль сохранится, но работать не будет. */
const awgState = ref<AwgStatus | null>(null);
const awgInstalling = ref(false);
const awgNote = ref("");
const onRouter = computed(() => ["openwrt", "keenetic"].includes(awgState.value?.platform ?? ""));
const engineCfg = ref<EngineConfig | null>(null);
async function loadAwgState() {
  try {
    awgState.value = await awgApi.status();
  } catch {
    awgState.value = null;
  }
  if (onRouter.value) {
    try {
      engineCfg.value = await services.engineConfig();
    } catch {
      engineCfg.value = null;
    }
  }
}
watch(
  () => [props.open, isAwg.value] as const,
  ([open, awg]) => {
    if (open && awg && !awgState.value) void loadAwgState();
  },
  { immediate: true },
);
async function installMihomo() {
  awgInstalling.value = true;
  awgNote.value = "Ставлю mihomo из фида — это ~20 МБ загрузки, до пары минут…";
  try {
    await awgApi.install();
    const st = await poll(() => awgApi.status(), {
      done: (v) => !!v?.installed,
      intervalMs: 4000,
      timeoutMs: 300_000,
    });
    awgState.value = st;
    if (!st) throw new Error("роутер не ответил");
    awgNote.value = st.installed
      ? `mihomo ${st.version} установлен`
      : "Установка не завершилась — подробности в журнале обновлений";
  } catch (e) {
    awgNote.value = `Не удалось установить mihomo: ${(e as Error).message}`;
  } finally {
    awgInstalling.value = false;
  }
}
const needsUuid = computed(() => ["vless", "vmess", "tuic"].includes(type.value));
const needsPassword = computed(() =>
  ["trojan", "shadowsocks", "hysteria2", "tuic"].includes(type.value),
);
const showTls = computed(() => !isWg.value && type.value !== "socks");
const tlsForced = computed(() => type.value === "hysteria2" || type.value === "tuic");
const showFingerprint = computed(
  () => showTls.value && !NO_UTLS.includes(type.value) && (d.value.tls || tlsForced.value),
);

const title = computed(() => (isNew.value ? "Новый профиль" : `Профиль: ${d.value.name}`));

const VPN_KEY = /^vpn:\/\//i;

async function parseInput(text: string): Promise<ProfileDraft | null> {
  const t = text.trim();
  if (VPN_KEY.test(t)) {
    const conf = await confFromAmneziaKey(t);
    return conf ? parseWgConf(conf) : null;
  }
  if (/\[Interface\]/i.test(t)) return parseWgConf(t);
  return parseShareLink(t);
}

async function applyLink() {
  const parsed = await parseInput(link.value);
  if (!parsed) {
    linkNote.value = VPN_KEY.test(link.value.trim())
      ? "Не удалось разобрать ключ vpn:// — в нём нет конфигурации WireGuard/AmneziaWG. Выгрузите из клиента Amnezia файл .conf"
      : "Не понял ссылку. Поддерживаются vless://, trojan://, vmess://, ss://, hysteria2://, tuic://, socks5://, http://, конфиг .conf WireGuard/AmneziaWG и ключ vpn://";
    return;
  }
  /* Имя, группу и область маршрутизации, если они уже заданы, ссылка не
     перетирает: в ней этих сведений нет, а стереть их было бы обидно. */
  const keepName = d.value.name.trim();
  const keepGroup = d.value.group.trim();
  const keepMode = d.value.routingMode;
  const id = isNew.value ? parsed.id : d.value.id;
  d.value = { ...parsed, id, routingMode: keepMode };
  if (keepName) d.value.name = keepName;
  if (keepGroup) d.value.group = keepGroup;
  if (!d.value.name) d.value.name = parsed.server;
  if (isNew.value && !idTouched.value) d.value.id = slugify(d.value.name);
  linkNote.value = `Разобрано: ${parsed.type}, ${parsed.server}:${parsed.port}`;
  link.value = "";
}

const fileEl = ref<HTMLInputElement | null>(null);
async function pickFile(ev: Event) {
  const input = ev.target as HTMLInputElement;
  const f = input.files?.[0];
  if (!f) return;
  link.value = await f.text();
  input.value = "";
  await applyLink();
}
/** Многострочный .conf в однострочное поле не влезет — растим поле по тексту. */
const linkRows = computed(() => Math.min(10, Math.max(1, link.value.split("\n").length)));
function linkEnter(e: KeyboardEvent) {
  if (e.shiftKey || link.value.includes("\n")) return;
  e.preventDefault();
  void applyLink();
}

/* Новый AWG-профиль без mihomo роутер не примет: в цепочке он стал бы выходом
   в никуда. Правку профиля, который уже был AWG, роутер пропускает — и мы. */
const awgBlocked = computed(
  () =>
    isAwg.value &&
    onRouter.value &&
    !awgState.value?.installed &&
    props.draft?.type !== "amneziawg",
);

const canSave = computed(() => {
  if (!d.value.name.trim()) return false;
  if (awgBlocked.value) return false;
  if (isWg.value)
    return !!d.value.privateKey.trim() && !!d.value.peerPublicKey.trim() && !!d.value.server.trim();
  return !!d.value.server.trim();
});

function submit() {
  if (!canSave.value) return;
  emit("save", { draft: d.value, activate: activateNow.value });
}

/* Ссылку собираем из полей формы, а не отдаём сохранённый d.uri: профиль могли
   править после импорта, и старая ссылка увела бы человека на другой сервер. */
const shareText = computed(() =>
  (isWg.value ? buildWireguardConf(d.value) : buildShareLink(d.value)) || "",
);
const shareLabel = computed(() => (isWg.value ? "Скопировать конфиг" : "Скопировать ссылку"));

async function copyShare() {
  const text = shareText.value;
  if (!text) {
    linkNote.value = isWg.value
      ? "Не хватает ключей: конфиг wireguard собрать не из чего"
      : "Не хватает данных для ссылки: заполните адрес сервера и UUID/пароль";
    return;
  }
  shareShown.value = true;
  const ok = await copyText(text);
  linkNote.value = ok
    ? isWg.value
      ? "Конфиг скопирован — вставьте его в клиент WireGuard"
      : "Ссылка скопирована — вставьте её в сторонний клиент"
    : "Буфер обмена недоступен (панель открыта без HTTPS) — выделите текст ниже и скопируйте вручную";
  await nextTick();
  if (!ok) shareEl.value?.select();
}
</script>

<template>
  <DrawerSheet :open="open" :title="title" wide @close="emit('close')">
    <template #sticky>
      <div class="linkrow">
        <textarea
          v-model="link"
          :rows="linkRows"
          spellcheck="false"
          placeholder="Ссылка vless://…, .conf WireGuard/AmneziaWG или vpn://"
          aria-label="Ссылка или конфиг"
          @keydown.enter="linkEnter"
        ></textarea>
        <UiButton :disabled="!link.trim()" @click="applyLink">Разобрать</UiButton>
        <UiButton title="Загрузить .conf WireGuard/AmneziaWG" @click="fileEl?.click()">Файл…</UiButton>
        <input ref="fileEl" type="file" accept=".conf,.txt,text/plain" hidden @change="pickFile" />
        <UiButton :disabled="!shareText" :title="`${shareLabel} для стороннего клиента`" @click="copyShare">
          {{ shareLabel }}
        </UiButton>
      </div>
      <p v-if="linkNote" class="note">{{ linkNote }}</p>
      <textarea
        v-if="shareShown && shareText"
        ref="shareEl"
        class="share"
        :rows="isWg ? 8 : 2"
        :value="shareText"
        readonly
        spellcheck="false"
        aria-label="Ссылка для стороннего клиента"
        @focus="($event.target as HTMLTextAreaElement).select()"
      ></textarea>
    </template>

    <div class="grid">
      <PField label="Имя профиля">
        <input v-model="d.name" type="text" placeholder="Например, Example VPN" />
      </PField>

      <PField label="Группа" hint="Пусто — профиль попадёт в «Без группы»">
        <input v-model="d.group" type="text" list="profile-groups" placeholder="Например, Европа" />
        <datalist id="profile-groups">
          <option v-for="g in groups" :key="g" :value="g"></option>
        </datalist>
      </PField>

      <PField label="Идентификатор" hint="Имя файла на роутере: только латиница, цифры, дефис">
        <input
          v-model="d.id"
          type="text"
          :disabled="!isNew"
          spellcheck="false"
          @input="idTouched = true"
        />
      </PField>

      <PField label="Протокол">
        <select v-model="d.type">
          <option v-for="t in PROTO_TYPES" :key="t.value" :value="t.value">
            {{ t.label }}
          </option>
        </select>
      </PField>

      <PField :label="isWg ? 'Endpoint (адрес пира)' : 'Сервер'">
        <input v-model="d.server" type="text" spellcheck="false" placeholder="198.51.100.10" />
      </PField>

      <PField label="Порт">
        <input v-model="d.port" type="text" inputmode="numeric" placeholder="443" />
      </PField>

      <PField v-if="needsUuid" label="UUID" wide>
        <input v-model="d.uuid" type="text" spellcheck="false" placeholder="00000000-0000-0000-0000-000000000000" />
      </PField>

      <PField
        v-if="needsPassword"
        :label="type === 'tuic' ? 'Пароль (token)' : 'Пароль'"
        wide
      >
        <input v-model="d.password" type="text" spellcheck="false" />
      </PField>

      <PField v-if="type === 'shadowsocks'" label="Метод шифрования">
        <select v-model="d.method">
          <option v-for="m in SS_METHODS" :key="m" :value="m">{{ m }}</option>
        </select>
      </PField>

      <PField v-if="type === 'vless'" label="Flow" hint="Обычно xtls-rprx-vision или пусто">
        <input v-model="d.flow" type="text" spellcheck="false" placeholder="xtls-rprx-vision" />
      </PField>

      <PField v-if="type === 'vmess'" label="alterId">
        <input v-model="d.alterId" type="text" inputmode="numeric" placeholder="0" />
      </PField>

      <template v-if="type === 'hysteria2'">
        <PField label="Пароль обфускации" hint="Salamander, если сервер её требует">
          <input v-model="d.obfsPassword" type="text" spellcheck="false" />
        </PField>
        <PField label="Скорость вверх / вниз, Мбит/с" hint="Пусто — без ограничения">
          <div class="pair">
            <input v-model="d.upMbps" type="text" inputmode="numeric" placeholder="50" />
            <input v-model="d.downMbps" type="text" inputmode="numeric" placeholder="200" />
          </div>
        </PField>
      </template>

      <PField v-if="type === 'tuic'" label="Контроль перегрузки">
        <select v-model="d.congestion">
          <option value="">по умолчанию</option>
          <option value="bbr">bbr</option>
          <option value="cubic">cubic</option>
          <option value="new_reno">new_reno</option>
        </select>
      </PField>

      <template v-if="isProxy">
        <PField label="Пользователь" hint="Пусто — прокси без авторизации">
          <input v-model="d.username" type="text" spellcheck="false" />
        </PField>
        <PField label="Пароль">
          <input v-model="d.password" type="text" spellcheck="false" />
        </PField>
      </template>

      <template v-if="isWg">
        <PField label="Приватный ключ" wide>
          <input v-model="d.privateKey" type="text" spellcheck="false" placeholder="base64" />
        </PField>
        <PField label="Публичный ключ пира" wide>
          <input v-model="d.peerPublicKey" type="text" spellcheck="false" placeholder="base64" />
        </PField>
        <PField label="Preshared key" hint="Если сервер его использует">
          <input v-model="d.presharedKey" type="text" spellcheck="false" />
        </PField>
        <PField label="MTU">
          <input v-model="d.mtu" type="text" inputmode="numeric" placeholder="1420" />
        </PField>
        <PField label="Локальные адреса" hint="По одному на строку" wide>
          <textarea v-model="d.localAddress" rows="2" spellcheck="false" placeholder="10.0.0.2/32"></textarea>
        </PField>
        <PField label="Allowed IPs" hint="По одному на строку" wide>
          <textarea v-model="d.allowedIps" rows="2" spellcheck="false" placeholder="0.0.0.0/0"></textarea>
        </PField>
        <PField v-if="!isAwg" label="Reserved" hint="Три числа через запятую, если требует сервер">
          <input v-model="d.reserved" type="text" spellcheck="false" placeholder="0,0,0" />
        </PField>
        <template v-if="isAwg">
          <PField label="Persistent keepalive, с" hint="Пусто — выключен">
            <input v-model="d.keepalive" type="text" inputmode="numeric" placeholder="25" />
          </PField>
          <PField
            label="Параметры AmneziaWG"
            hint="Строки из [Interface] конфига Amnezia: Jc, Jmin, Jmax, S1–S4, H1–H4, I1–I5; для AWG 1.5 — J1–J3, Itime; для AWG 3 — HeaderProtectionKey и др."
            wide
          >
            <textarea
              v-model="d.awg"
              rows="8"
              spellcheck="false"
              class="mono"
              placeholder="Jc = 4&#10;Jmin = 40&#10;Jmax = 70&#10;S1 = 15&#10;S2 = 21&#10;H1 = 1234567&#10;H2 = 2345678&#10;H3 = 3456789&#10;H4 = 4567890"
            ></textarea>
          </PField>
          <div class="awgstate" role="status">
            <p v-if="awgState && !awgState.installed && awgState.platform === 'ios'" class="warn">
              На iOS AmneziaWG пока недоступен: система не даёт запустить mihomo рядом с туннелем.
            </p>
            <p v-else-if="awgState && !awgState.installed && !onRouter" class="warn">
              В этой сборке приложения нет mihomo, через который работает AmneziaWG, —
              переустановите Detour свежим установщиком.
            </p>
            <template v-else-if="awgState && !awgState.installed">
              <p class="warn">
                AmneziaWG на роутере работает через mihomo, а он не установлен
                (~57 МБ на диске). Пока его нет, профиль не сохранить. Проверьте, что
                на роутере хватит места.
              </p>
              <UiButton :busy="awgInstalling" @click="installMihomo">Установить mihomo</UiButton>
            </template>
            <p v-else-if="engineCfg?.mode === 'singbox'" class="warn">
              Выбран движок «sing-box» — в нём AmneziaWG недоступен. Смените режим в
              «Сервисы → Движок VPN» на гибрид или mihomo.
            </p>
            <p v-else-if="awgState" class="note">
              mihomo {{ awgState.version }}.
              <template v-if="engineCfg?.mode === 'mihomo'">Движок mihomo: профиль может стоять в любом звене цепочки.</template>
              <template v-else>Гибрид: AmneziaWG-профиль в цепочке может стоять только первым звеном.</template>
            </p>
            <p v-if="awgNote" class="note">{{ awgNote }}</p>
          </div>
        </template>
      </template>

      <template v-if="showTls">
        <PField v-if="!tlsForced" label="TLS">
          <select v-model="d.tls">
            <option :value="true">включён</option>
            <option :value="false">выключен</option>
          </select>
        </PField>
        <template v-if="d.tls || tlsForced">
          <PField label="SNI" hint="Имя, которое видит сервер в TLS">
            <input v-model="d.sni" type="text" spellcheck="false" placeholder="panel.example.com" />
          </PField>
          <PField label="ALPN" hint="Через запятую: h2, http/1.1">
            <input v-model="d.alpn" type="text" spellcheck="false" />
          </PField>
          <PField
            v-if="showFingerprint"
            label="Отпечаток uTLS"
            hint="chrome, firefox, safari — если сервер его ждёт"
          >
            <input v-model="d.fingerprint" type="text" spellcheck="false" placeholder="chrome" />
          </PField>
          <PField v-if="isV2ray" label="Reality: публичный ключ">
            <input v-model="d.realityKey" type="text" spellcheck="false" />
          </PField>
          <PField v-if="isV2ray" label="Reality: short id">
            <input v-model="d.realityShortId" type="text" spellcheck="false" />
          </PField>
          <PField label="Проверка сертификата">
            <select v-model="d.insecure">
              <option :value="false">обязательна</option>
              <option :value="true">не проверять</option>
            </select>
          </PField>
        </template>
      </template>

      <p v-if="NO_UTLS.includes(d.type)" class="warn" role="note">
        У {{ d.type }} отпечаток uTLS не задаётся: sing-box с ним отказывает в
        каждом соединении. Роутер вырезает это поле, даже если оно приехало из
        подписки.
      </p>

      <template v-if="isV2ray">
        <PField label="Транспорт">
          <select v-model="d.transport">
            <option value="">tcp (без обёртки)</option>
            <option value="ws">websocket</option>
            <option value="grpc">gRPC</option>
            <option value="http">http/2</option>
          </select>
        </PField>
        <PField v-if="d.transport === 'ws' || d.transport === 'http'" label="Путь">
          <input v-model="d.path" type="text" spellcheck="false" placeholder="/ws" />
        </PField>
        <PField v-if="d.transport === 'ws' || d.transport === 'http'" label="Host-заголовок">
          <input v-model="d.host" type="text" spellcheck="false" />
        </PField>
        <PField v-if="d.transport === 'grpc'" label="gRPC service name">
          <input v-model="d.serviceName" type="text" spellcheck="false" />
        </PField>
      </template>

      <PField v-if="type === 'http'" label="Путь HTTP-прокси" hint="Нужен редко">
        <input v-model="d.path" type="text" spellcheck="false" placeholder="/connect" />
      </PField>

      <PField
        label="Область маршрутизации"
        hint="Чем профиль отличается от общей настройки: список доменов или всё, кроме белого списка"
      >
        <select v-model="d.routingMode">
          <option value="">как в общих настройках</option>
          <option value="proxy-list">только список доменов</option>
          <option value="all-except">всё, кроме белого списка</option>
        </select>
      </PField>
    </div>

    <label class="now">
      <input v-model="activateNow" type="checkbox" />
      <span>Подключиться к этому профилю сразу после сохранения</span>
    </label>

    <template #footer>
      <UiButton variant="primary" :busy="busy" :disabled="!canSave" @click="submit">
        Сохранить
      </UiButton>
      <UiButton @click="emit('close')">Отмена</UiButton>
      <span v-if="awgBlocked" class="hint">Сначала установите mihomo</span>
      <span v-else-if="!canSave" class="hint">Заполните имя и адрес сервера</span>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.linkrow {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}
.linkrow textarea {
  flex: 1 1 260px;
  resize: vertical;
  font-family: inherit;
  line-height: 1.4;
  min-width: 0;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 9px 11px;
  font-size: 16px;
  min-height: 44px;
  outline: none;
}
.linkrow textarea:focus {
  border-color: var(--accent);
}
.note {
  font-size: 12px;
  color: var(--dim);
  margin-top: 6px;
  overflow-wrap: anywhere;
}
.share {
  width: 100%;
  margin-top: 6px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--ink);
  padding: 8px 10px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  line-height: 1.5;
  resize: vertical;
  outline: none;
}
.grid {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 13px;
}
.awgstate {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.awgstate .warn {
  width: 100%;
}
.pair {
  display: flex;
  gap: 8px;
}
.warn {
  grid-column: 1 / -1;
  font-size: 12px;
  color: var(--warn);
  border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
  background: color-mix(in srgb, var(--warn) 10%, transparent);
  border-radius: var(--radius-sm);
  padding: 8px 11px;
}
.now {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 16px;
  font-size: 14px;
  min-height: 44px;
}
.now input {
  width: 18px;
  height: 18px;
  accent-color: var(--accent);
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
</style>
