<script setup lang="ts">
/* Правила устройств: «мимо VPN» или «через свой VPN/цепочку». Файл на роутере —
   строки «mac|mode|target|name» (mode: direct | vpn). Устройство узнаётся по MAC:
   адрес от DHCP меняется, MAC — нет. «Отдельные маршруты» роутера главнее: их
   сайты идут своим путём и с устройств из этого списка. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import type { LanClient } from "@/api";

interface DeviceRule {
  mac: string;
  /* "direct" — мимо VPN, иначе id профиля или цепочки. */
  route: string;
  name: string;
}

interface TargetOption {
  id: string;
  name: string;
  isChain: boolean;
}

const props = defineProps<{
  open: boolean;
  text: string;
  targets: TargetOption[];
  clients: LanClient[];
  vpnSupported: boolean;
  loading?: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{ close: []; save: [string] }>();

const rules = ref<DeviceRule[]>([]);
const MAC = /^([0-9a-f]{2}:){5}[0-9a-f]{2}$/;

function normMac(v: string): string {
  return v.trim().toLowerCase().replace(/-/g, ":");
}

function parse(text: string): DeviceRule[] {
  const out: DeviceRule[] = [];
  for (const line of String(text ?? "").replace(/\r/g, "").split("\n")) {
    const [mac = "", mode = "", target = "", name = ""] = line.split("|");
    if (!MAC.test(normMac(mac))) continue;
    if (mode === "direct") out.push({ mac: normMac(mac), route: "direct", name });
    else if (mode === "vpn" && target) out.push({ mac: normMac(mac), route: target, name });
  }
  return out;
}

function serialize(list: DeviceRule[]): string {
  return list
    .filter((r) => MAC.test(normMac(r.mac)) && r.route)
    .map((r) => {
      const name = r.name.replace(/[|\n\r\t]/g, " ").trim();
      return r.route === "direct"
        ? `${normMac(r.mac)}|direct||${name}`
        : `${normMac(r.mac)}|vpn|${r.route}|${name}`;
    })
    .join("\n");
}

watch(
  () => [props.open, props.text] as const,
  ([open]) => {
    if (open) rules.value = parse(props.text);
  },
  { immediate: true },
);

/* Устройства в сети, у которых известен MAC, — чтобы не вводить его руками. */
const known = computed(() =>
  props.clients
    .filter((c) => c.mac && MAC.test(normMac(c.mac)))
    .map((c) => ({ mac: normMac(c.mac!), label: `${c.host || c.ip} · ${c.ip}`, host: c.host || c.ip })),
);

function pick(rule: DeviceRule, mac: string) {
  rule.mac = mac;
  const c = known.value.find((k) => k.mac === mac);
  if (c && !rule.name) rule.name = c.host;
}

const profileOptions = computed(() => props.targets.filter((t) => !t.isChain));
const chainOptions = computed(() => props.targets.filter((t) => t.isChain));

function add() {
  rules.value = [...rules.value, { mac: "", route: "direct", name: "" }];
}

function remove(index: number) {
  rules.value = rules.value.filter((_, i) => i !== index);
}

function macBad(rule: DeviceRule): boolean {
  return rule.mac.trim() !== "" && !MAC.test(normMac(rule.mac));
}

const dup = computed(() => {
  const seen = new Set<string>();
  const out = new Set<string>();
  for (const r of rules.value) {
    const m = normMac(r.mac);
    if (!m) continue;
    if (seen.has(m)) out.add(m);
    seen.add(m);
  }
  return out;
});

const ready = computed(() => rules.value.filter((r) => MAC.test(normMac(r.mac))).length);
</script>

<template>
  <DrawerSheet :open="open" title="Устройства" wide @close="emit('close')">
    <template #sticky>
      <p class="hint">
        <span>Отдельное правило для устройства в сети: весь его трафик мимо VPN
          или через выбранный VPN или цепочку. «Отдельные маршруты» главнее —
          их сайты идут своим путём и с этих устройств.</span>
        <b class="num">{{ loading ? "загружаю…" : `${ready} из ${rules.length}` }}</b>
      </p>
    </template>

    <p v-if="loading" class="note">Загружаю правила…</p>
    <p v-else-if="!rules.length" class="empty">
      Правил пока нет. Например, игровой компьютер можно пустить мимо VPN: часть
      игровых серверов выгоняет игроков с VPN-адресов.
    </p>

    <div v-for="(rule, i) in rules" :key="i" class="card">
      <div class="row">
        <select
          class="sel"
          :value="known.some((k) => k.mac === normMac(rule.mac)) ? normMac(rule.mac) : ''"
          aria-label="Устройство в сети"
          @change="pick(rule, ($event.target as HTMLSelectElement).value)"
        >
          <option value="">{{ known.length ? "Выберите устройство…" : "Устройств в сети не видно" }}</option>
          <option v-for="k in known" :key="k.mac" :value="k.mac">{{ k.label }}</option>
        </select>
        <button class="del" type="button" @click="remove(i)">Убрать</button>
      </div>

      <div class="row">
        <input
          v-model="rule.mac"
          class="inp mono"
          :class="{ bad: macBad(rule) || dup.has(normMac(rule.mac)) }"
          spellcheck="false"
          autocapitalize="off"
          autocomplete="off"
          placeholder="MAC, например aa:bb:cc:dd:ee:ff"
          aria-label="MAC-адрес"
        />
        <input
          v-model="rule.name"
          class="inp"
          placeholder="Название (для себя)"
          aria-label="Название устройства"
        />
      </div>
      <p v-if="macBad(rule)" class="err">MAC-адрес — шесть пар шестнадцатеричных цифр через двоеточие.</p>
      <p v-else-if="dup.has(normMac(rule.mac))" class="err">Это устройство уже есть в списке — сработает первое правило.</p>

      <select v-model="rule.route" class="sel full" aria-label="Куда направить трафик устройства">
        <option value="direct">Мимо VPN — напрямую</option>
        <optgroup v-if="profileOptions.length" label="Через профиль">
          <option v-for="t in profileOptions" :key="t.id" :value="t.id" :disabled="!vpnSupported">
            {{ t.name }}
          </option>
        </optgroup>
        <optgroup v-if="chainOptions.length" label="Через цепочку">
          <option v-for="t in chainOptions" :key="t.id" :value="t.id" :disabled="!vpnSupported">
            {{ t.name }}
          </option>
        </optgroup>
        <option
          v-if="rule.route !== 'direct' && !targets.some((t) => t.id === rule.route)"
          :value="rule.route"
        >
          {{ rule.route }} (не найден — пойдёт через основной VPN)
        </option>
      </select>
    </div>

    <UiButton class="add" @click="add">Добавить устройство</UiButton>

    <p v-if="!vpnSupported" class="note">
      «Через профиль или цепочку» работает только в обычном режиме sing-box — в
      режиме «отдельный процесс на маршрут» доступно только «мимо VPN».
    </p>
    <p class="note">
      Сохранение перестраивает конфигурацию и перезапускает подключение. Для
      «мимо VPN» у устройства открывается и IPv6. Правило работает для устройств в
      локальной сети; клиенты VPN-сервера роутера (WireGuard) MAC не передают.
    </p>

    <template #footer>
      <UiButton
        variant="primary"
        :busy="busy"
        :disabled="loading || busy || rules.some(macBad)"
        @click="emit('save', serialize(rules))"
      >
        Сохранить и применить
      </UiButton>
      <UiButton :disabled="busy" @click="emit('close')">Отмена</UiButton>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.hint {
  font-size: 12.5px;
  color: var(--dim);
  display: flex;
  gap: 10px;
  align-items: baseline;
  flex-wrap: wrap;
}
.hint b {
  margin-left: auto;
  color: var(--ink);
  font-weight: 600;
  white-space: nowrap;
}
.empty {
  border: 1px dashed var(--line-2);
  border-radius: var(--radius-sm);
  padding: 14px;
  color: var(--dim);
  font-size: 13.5px;
}
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
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.sel,
.inp {
  flex: 1 1 200px;
  min-width: 0;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--ink);
  padding: 9px 10px;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  min-height: 44px;
}
.sel.full {
  flex: none;
  width: 100%;
}
.inp:focus,
.sel:focus {
  border-color: var(--accent);
  outline: none;
}
.inp.bad {
  border-color: var(--bad);
}
.err {
  font-size: 12px;
  color: var(--bad);
}
.del {
  border: 1px solid var(--line-2);
  background: transparent;
  color: var(--bad);
  border-radius: var(--radius-sm);
  padding: 8px 12px;
  font-size: 13px;
  min-height: 44px;
}
.del:hover {
  background: color-mix(in srgb, var(--bad) 12%, transparent);
  border-color: var(--bad);
}
.add {
  width: 100%;
  justify-content: center;
}
.note {
  margin-top: 12px;
  font-size: 12px;
  color: var(--faint);
}
@media (max-width: 700px) {
  :deep(.btn) {
    min-height: 44px;
  }
}
</style>
