<script setup lang="ts">
/* Плитка «Каналы в интернет». Появляется ТОЛЬКО когда аплинков больше одного:
   при единственном канале его скорость и так есть строкой в плитке «Система», и
   отдельная плитка была бы шумом.

   Зачем вообще: с двумя провайдерами «WAN 1000 Мбит/с» в «Системе» перестаёт
   быть ответом на вопрос «что у меня с интернетом» — оно описывает лишь тот
   канал, который сейчас держит маршрут по умолчанию. Резерв, который тихо
   отвалился, из той строки не виден вообще, а именно он и есть смысл второго
   канала. */
import { computed, onMounted, ref, watch } from "vue";
import TileCard from "@/components/TileCard.vue";
import SegmentedControl from "@/components/SegmentedControl.vue";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { services } from "@/api";
import type { WanUplink, WanpinMode, WanpinStatus } from "@/api";

const status = useStatusStore();
const toast = useToastStore();

const wan = computed(() => status.data?.wan_link);
const uplinks = computed<WanUplink[]>(() => wan.value?.uplinks ?? []);

/* Показываем при двух и более. `multi` бэкенд считает сам, но полагаться только
   на него нельзя: старая версия detour-wan-link этого поля не отдаёт вовсе, а
   панель обновляется отдельно от роутера.
   Объявлено здесь, а не ниже к остальным computed: watch() ниже разыменовывает
   `show` прямо во время setup, и объявление после него попало бы в TDZ. */
const show = computed(() => uplinks.value.length > 1);

/* Привязка запрашивается отдельно от status: она нужна только здесь и меняется
   лишь когда пользователь сам её трогает. */
const pin = ref<WanpinStatus | null>(null);
const busy = ref("");

async function loadPin() {
  try {
    pin.value = await services.wanpinStatus();
  } catch {
    pin.value = null;
  }
}

onMounted(() => {
  if (show.value) void loadPin();
});
/* Плитка появляется только при 2+ каналах, а первый status может прийти позже
   монтирования — иначе привязка не загрузится вовсе. */
watch(show, (on) => {
  if (on && !pin.value) void loadPin();
});

/* Каналы, которым вообще есть что привязывать: поднятые и со шлюзом. Отключённый
   резерв в выбор публичного канала попадать не должен — у него нет адреса. */
const pins = computed(() => pin.value?.pins ?? []);

const publicId = computed({
  get: () => pin.value?.public ?? "",
  set: (id: string) => void setPublic(id),
});

const mode = computed({
  get: () => (pin.value?.mode ?? "auto") as WanpinMode,
  set: (m: WanpinMode) => void setMode(m),
});

async function setPublic(id: string) {
  if (!id || id === pin.value?.public) return;
  busy.value = "public";
  try {
    pin.value = await services.wanpinSetPublic(id);
    toast.ok("Публичный канал изменён");
  } catch (e) {
    toast.fromError(e, "Не удалось сменить публичный канал");
  } finally {
    busy.value = "";
  }
}

async function setMode(m: WanpinMode) {
  busy.value = "mode";
  try {
    pin.value = await services.wanpinSetMode(m);
    toast.ok("Режим привязки применён");
  } catch (e) {
    toast.fromError(e, "Не удалось применить режим");
  } finally {
    busy.value = "";
  }
}

function labelFor(id: string): string {
  return uplinks.value.find((u) => u.id === id)?.label || id;
}

/* Ни одного живого канала — это отдельное состояние, и оно важнее любых
   подробностей по каждому. */
const allDown = computed(() => show.value && uplinks.value.every((u) => !u.up));

/* Резерв есть, только если живых каналов больше одного: один живой при трёх
   настроенных — это не «есть резерв», это «резерва не осталось».
   Канал с мёртвым маршрутом резервом не считается: адрес у него есть, транзита
   нет — засчитать его значит пообещать подстраховку, которой не существует. */
const spares = computed(
  () => uplinks.value.filter((u) => u.up && !u.active && !u.route_dead).length,
);

type Kind = "ok" | "warn" | "off";

/* Различие carrier/up тут не педантизм, а разные причины и разные действия:
   нет carrier — смотри провод, есть carrier без адреса — смотри провайдера. */
function describe(u: WanUplink): { kind: Kind; state: string } {
  if (!u.up) {
    if (u.carrier === 1) return { kind: "warn", state: "линк есть, адреса нет" };
    return { kind: "off", state: "нет линка" };
  }
  /* Раньше проверки `up` было достаточно, но адрес от провайдера ещё не значит
     интернет: канал может отвечать на ARP и молчать дальше первого хопа. */
  if (u.route_dead) return { kind: "warn", state: "адрес есть, интернета нет" };
  if (u.degraded) return { kind: "warn", state: `просел до ${u.speed_mbps} Мбит/с` };
  return { kind: "ok", state: u.active ? "в работе" : "в резерве" };
}

/* Вторая строка — только то, что реально известно. Пустые поля не превращаем в
   прочерки: строка из четырёх «—» ничего не сообщает, а место занимает. */
function facts(u: WanUplink): string[] {
  const out: string[] = [u.iface];
  if (u.ip) out.push(u.ip);
  if (u.speed_mbps) out.push(`${u.speed_mbps} Мбит/с`);
  if (u.weight) out.push(`вес ${u.weight}`);
  return out;
}
</script>

<template>
  <TileCard v-if="show" title="Каналы в интернет">
    <p v-if="allDown" class="state">
      <i class="dot bad" aria-hidden="true"></i>
      <span>Ни один канал не поднят</span>
    </p>

    <ul class="links">
      <li v-for="u in uplinks" :key="u.id">
        <p class="state">
          <i class="dot" :class="describe(u).kind" aria-hidden="true"></i>
          <span class="name">{{ u.label }}</span>
          <span class="dim">{{ describe(u).state }}</span>
          <span v-if="pin?.public === u.id" class="chip">публичный</span>
        </p>
        <p class="meta">
          <span v-for="f in facts(u)" :key="f">{{ f }}</span>
        </p>
      </li>
    </ul>

    <p v-if="!allDown && !spares" class="hint">
      Резервного канала сейчас нет — работает только один.
    </p>

    <!-- Выбор появляется только когда есть из чего выбирать: у отключённого
         резерва нет адреса, публичным каналом он быть не может. -->
    <SegmentedControl
      v-if="pins.length > 1"
      v-model="publicId"
      label="Публичный канал"
      :busy="busy === 'public'"
      :options="pins.map((p) => ({ value: p.id, label: labelFor(p.id) }))"
    />
    <p v-if="pins.length > 1" class="hint">
      Адрес этого канала показывают проброс портов, HTTPS-панель и выпуск
      сертификата.
    </p>

    <SegmentedControl
      v-model="mode"
      label="Отвечать тем же каналом"
      :busy="busy === 'mode'"
      :options="[
        { value: 'auto', label: 'Авто' },
        { value: 'on', label: 'Всегда' },
        { value: 'off', label: 'Выкл' },
      ]"
    />
    <p class="hint">
      <template v-if="mode === 'off'">
        Выключено: ответ пойдёт маршрутом по умолчанию, и при балансировке
        проброс портов будет работать через раз.
      </template>
      <template v-else-if="pin?.installed">
        Ответ на входящее соединение уходит тем каналом, которым пришёл запрос.
      </template>
      <template v-else>
        Включится само, когда поднимется второй канал — с одним привязывать
        нечего.
      </template>
    </p>

    <p v-if="pin?.rp_filter_strict" class="warnline">
      Строгий rp_filter на {{ pin.rp_filter_strict }} — входящие пакеты на
      неосновном канале будут молча теряться.
    </p>
  </TileCard>
</template>

<style scoped>
.links {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.links li + li {
  border-top: 1px solid var(--line);
  padding-top: 10px;
}
.state {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 13.5px;
}
.name {
  font-weight: 500;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--faint);
  flex: 0 0 auto;
}
.dot.ok {
  background: var(--ok);
}
.dot.warn {
  background: var(--warn);
}
.dot.bad {
  background: var(--bad);
}
.dim {
  color: var(--dim);
  font-size: 12px;
}
.meta {
  font-size: 13px;
  color: var(--dim);
  display: flex;
  gap: 4px 14px;
  flex-wrap: wrap;
  margin-top: 2px;
  padding-left: 15px;
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
.chip {
  font-size: 11px;
  color: var(--accent);
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 1px 7px;
}
.warnline {
  font-size: 12px;
  color: var(--warn);
}
</style>
