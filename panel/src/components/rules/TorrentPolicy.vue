<script setup lang="ts">
/* Что делать с торрентами, когда активный профиль их запрещает: блокировать
   (как раньше), пускать напрямую или через профиль, где они разрешены.
   Сами разрешения ставятся на профилях («Профили» → торренты); здесь только
   судьба запрещённого. */
import { computed, ref, watch } from "vue";
import RuleSection from "@/components/rules/RuleSection.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import UiButton from "@/components/UiButton.vue";
import { profiles as profilesApi } from "@/api";
import type { TorrentAction, TorrentActionMode, TorrentStatus } from "@/api";
import { useProfilesStore } from "@/stores/profiles";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { uplinkName } from "@/lib/uplink";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ toggle: [] }>();

const store = useProfilesStore();
const status = useStatusStore();
const toast = useToastStore();

const action = ref<TorrentAction | null>(null);
const st = ref<TorrentStatus | null>(null);
const mode = ref<TorrentActionMode>("block");
const via = ref("");
const uplink = ref("");
const busy = ref(false);

const MODES: { value: TorrentActionMode; label: string; hint: string }[] = [
  { value: "block", label: "Блокировать", hint: "Торренты не работают, пока активен такой профиль" },
  { value: "direct", label: "Напрямую", hint: "Торрент-трафик идёт мимо VPN, с вашего адреса" },
  { value: "via", label: "Через профиль", hint: "Через профиль, где торренты разрешены" },
];

const allowed = computed(() => store.rows.filter((r) => r.torrents === true));

/* Канал в интернет — только у роутера и только когда каналов больше одного. */
const uplinks = computed(() => action.value?.uplinks ?? []);
const showUplink = computed(() => !status.isClient && uplinks.value.length >= 2);
function uplinkLabel(id: string): string {
  return id ? `«${uplinkName(id)}»` : "";
}

async function load() {
  const [a, s] = await Promise.all([
    profilesApi.torrentAction().catch(() => null),
    profilesApi.torrentStatus().catch(() => null),
  ]);
  action.value = a;
  st.value = s;
  if (a) {
    mode.value = a.mode;
    via.value = a.via;
    uplink.value = a.uplink ?? "";
  }
  if (!store.rows.length) void store.load();
}

watch(
  () => props.open,
  (o) => {
    if (o) void load();
  },
);
void load();

const summary = computed(() => {
  const a = action.value;
  if (!a) return "Читаю настройку…";
  const ch = a.uplink ? `, канал ${uplinkLabel(a.uplink)}` : "";
  if (a.mode === "direct") return `Пускать напрямую, мимо VPN${ch}`;
  if (a.mode === "via") {
    const name = store.rows.find((r) => r.id === a.via)?.name ?? a.via;
    return a.via_ok ? `Через «${name}»${ch}` : `Через «${name}» — но там торренты больше не разрешены, сейчас блок`;
  }
  return "Блокировать";
});

const dirty = computed(
  () =>
    !!action.value &&
    (mode.value !== action.value.mode ||
      (mode.value === "via" && via.value !== action.value.via) ||
      (mode.value !== "block" && uplink.value !== (action.value.uplink ?? ""))),
);

const uplinkDead = computed(() => {
  const u = uplinks.value.find((x) => x.id === uplink.value);
  return !!u && !u.usable;
});

async function save() {
  if (mode.value === "via" && !via.value) {
    toast.error("Выберите профиль, где торренты разрешены");
    return;
  }
  busy.value = true;
  try {
    action.value = await profilesApi.setTorrentAction(
      mode.value,
      mode.value === "via" ? via.value : "",
      mode.value !== "block" && showUplink.value ? uplink.value : "",
    );
    toast.ok("Сохранено");
    st.value = await profilesApi.torrentStatus().catch(() => null);
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <RuleSection
    id="rule-torrents"
    icon="torrent"
    title="Торренты на запрещающих профилях"
    :summary="summary"
    :open="open"
    @toggle="emit('toggle')"
  >
    <p class="hint">
      Большинство VPN-провайдеров запрещают торренты и банят за них аккаунт,
      поэтому на профиле без явного разрешения торренты по умолчанию
      блокируются.
      <template v-if="status.isClient">
        Вместо блока их можно увести: весь трафик торрент-программ (qBittorrent,
        Transmission, µTorrent, Deluge и другие) идёт напрямую или через выбранный
        профиль, остальные программы — обычным маршрутом.
      </template>
      <template v-else>
        Вместо блока их можно увести: роутер узнаёт торрент-клиента по DHT,
        трекерам и uTP и следующие 30 минут пускает его соединения на
        нестандартные порты отдельно. Браузер того же компьютера (веб, почта,
        мессенджеры) остаётся на обычном маршруте.
      </template>
    </p>

    <div class="scroll-x">
      <SegmentedControl v-model="mode" label="Что делать с торрентами" :options="MODES" :busy="busy" />
    </div>

    <template v-if="mode === 'via'">
      <p v-if="!allowed.length" class="warn">
        Нет ни одного профиля с разрешёнными торрентами. Разрешите их нужному
        профилю в «Профилях», затем вернитесь сюда.
      </p>
      <label v-else class="pick">
        <span>Профиль для торрентов</span>
        <select v-model="via">
          <option value="" disabled>— выберите —</option>
          <option v-for="r in allowed" :key="r.id" :value="r.id">{{ r.name }}</option>
        </select>
      </label>
    </template>

    <template v-if="mode !== 'block' && showUplink">
      <label class="pick">
        <span>Канал в интернет</span>
        <select v-model="uplink">
          <option value="">Как у роутера</option>
          <option v-for="u in uplinks" :key="u.id" :value="u.id">
            {{ uplinkName(u.id) }} · {{ u.iface }}{{ u.ip ? ` · ${u.ip}` : "" }}{{ u.usable ? "" : " · не работает" }}
          </option>
        </select>
      </label>
      <p v-if="uplink" class="hint">
        <template v-if="mode === 'direct'">
          Новые соединения торрент-клиента выходят в интернет через {{ uplinkLabel(uplink) }} — с его адресом.
        </template>
        <template v-else>
          Туннель профиля для торрентов набирается через {{ uplinkLabel(uplink) }}; основной VPN остаётся на своём канале.
        </template>
        Если канал упадёт — каналом роутера. Входящие соединения от пиров на канал с серым адресом
        (CGNAT) не приходят: качать и раздавать можно, но только тем пирам, к которым подключаетесь вы.
      </p>
      <p v-if="uplink && uplinkDead" class="warn">Канал сейчас не работает — торренты идут каналом роутера.</p>
      <p v-if="uplink && action?.uplink_pinned === false" class="warn">
        Правила каналов на роутере сейчас не стоят (второй канал не обнаружен или
        «Отвечать тем же каналом» выключено) — выбор вступит в силу, когда они появятся.
      </p>
    </template>

    <p v-if="mode === 'direct'" class="warn">
      Провайдер увидит торрент-трафик с вашего домашнего адреса. При включённом
      «Всё через VPN» роутер всё равно блокирует — этот режим обещает, что
      напрямую не уходит ничего.
    </p>
    <p v-if="mode !== 'block' && !status.isClient" class="hint">
      Зашифрованный обмен с пирами роутер не распознаёт по содержимому — его
      уводит именно признак «это торрент-клиент». Компьютер, где торрент только
      что качал, полчаса ходит на нестандартные порты тем же путём.
    </p>

    <p v-if="st?.enforcing && st.action && st.action !== 'block'" class="ok">
      Сейчас работает: торренты идут {{ st.action === "direct" ? "напрямую" : `через «${st.via_name || st.via}»` }}{{
        st.uplink ? `, канал ${uplinkLabel(st.uplink)}` : ""
      }}<template
        v-if="st.clients?.length"
      >, торрент-клиенты: {{ st.clients.join(", ") }}</template>.
    </p>

    <div class="row">
      <UiButton variant="primary" :disabled="!dirty" :busy="busy" @click="save">Сохранить</UiButton>
      <UiButton :disabled="busy" @click="load">Перечитать</UiButton>
    </div>
  </RuleSection>
</template>

<style scoped>
.hint {
  font-size: 13px;
  color: var(--dim);
}
.warn {
  font-size: 12.5px;
  color: var(--warn);
}
.ok {
  font-size: 12.5px;
  color: var(--ok);
}
.row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.pick {
  display: grid;
  gap: 5px;
  font-size: 12.5px;
  color: var(--dim);
}
.pick select {
  font-size: 16px;
  min-height: 44px;
  padding: 0 10px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  color: var(--text);
  max-width: 100%;
}
</style>
