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
import { useToastStore } from "@/stores/toast";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ toggle: [] }>();

const store = useProfilesStore();
const toast = useToastStore();

const action = ref<TorrentAction | null>(null);
const st = ref<TorrentStatus | null>(null);
const mode = ref<TorrentActionMode>("block");
const via = ref("");
const busy = ref(false);

const MODES: { value: TorrentActionMode; label: string; hint: string }[] = [
  { value: "block", label: "Блокировать", hint: "Торренты не работают, пока активен такой профиль" },
  { value: "direct", label: "Напрямую", hint: "Торрент-трафик идёт мимо VPN, с вашего адреса" },
  { value: "via", label: "Через профиль", hint: "Через профиль, где торренты разрешены" },
];

const allowed = computed(() => store.rows.filter((r) => r.torrents === true));

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
  if (a.mode === "direct") return "Пускать напрямую, мимо VPN";
  if (a.mode === "via") {
    const name = store.rows.find((r) => r.id === a.via)?.name ?? a.via;
    return a.via_ok ? `Через «${name}»` : `Через «${name}» — но там торренты больше не разрешены, сейчас блок`;
  }
  return "Блокировать";
});

const dirty = computed(
  () => !!action.value && (mode.value !== action.value.mode || (mode.value === "via" && via.value !== action.value.via)),
);

async function save() {
  if (mode.value === "via" && !via.value) {
    toast.error("Выберите профиль, где торренты разрешены");
    return;
  }
  busy.value = true;
  try {
    action.value = await profilesApi.setTorrentAction(mode.value, mode.value === "via" ? via.value : "");
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
    title="Торренты на запрещающих профилях"
    :summary="summary"
    :open="open"
    @toggle="emit('toggle')"
  >
    <p class="hint">
      Большинство VPN-провайдеров запрещают торренты и банят за них аккаунт,
      поэтому на профиле без явного разрешения торренты по умолчанию
      блокируются. Вместо блока их можно увести: роутер узнаёт торрент-клиента по
      DHT, трекерам и uTP и следующие 30 минут пускает его соединения на
      нестандартные порты отдельно. Браузер того же компьютера (веб, почта,
      мессенджеры) остаётся на обычном маршруте.
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

    <p v-if="mode === 'direct'" class="warn">
      Провайдер увидит торрент-трафик с вашего домашнего адреса. При включённом
      «Всё через VPN» роутер всё равно блокирует — этот режим обещает, что
      напрямую не уходит ничего.
    </p>
    <p v-if="mode !== 'block'" class="hint">
      Зашифрованный обмен с пирами роутер не распознаёт по содержимому — его
      уводит именно признак «это торрент-клиент». Компьютер, где торрент только
      что качал, полчаса ходит на нестандартные порты тем же путём.
    </p>

    <p v-if="st?.enforcing && st.action && st.action !== 'block'" class="ok">
      Сейчас работает: торренты идут {{ st.action === "direct" ? "напрямую" : `через «${st.via_name || st.via}»` }}<template
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
