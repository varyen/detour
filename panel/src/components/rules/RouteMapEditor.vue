<script setup lang="ts">
/* Маршруты «домен → конкретный VPN». Файл на роутере — секционный текст:
     // === route:PROFILEID ===
     // meta: strict=1 via_chain=0
     example.com
   Разбор и сборка повторяют формат бэкенда буква в букву: любая отсебятина
   здесь тихо ломает маршрутизацию. Строки вне секций бэкендом не читаются —
   поэтому при пересборке они не сохраняются, о чём сказано в подсказке. */
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import UiButton from "@/components/UiButton.vue";
import { countEntries, entriesLabel } from "./entries";

interface RouteRule {
  id: string;
  strict: boolean;
  viaChain: boolean;
  text: string;
}

interface RouteTargetOption {
  id: string;
  name: string;
  /* Цепочка сама задаёт порядок хопов — для неё «через активную цепочку» не применимо. */
  isChain: boolean;
}

const props = defineProps<{
  open: boolean;
  text: string;
  targets: RouteTargetOption[];
  loading?: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{ close: []; save: [string] }>();

const rules = ref<RouteRule[]>([]);

const HEADER = /^\s*\/\/\s*===\s*route:\s*([A-Za-z0-9_-]+)\s*===\s*$/;
/* Только НАША служебная строка, а не любая заметка человека: иначе комментарий
   «// meta: не забыть про почту» съедался бы при сохранении. */
const META = /^\s*\/\/\s*meta:\s*((?:strict|via_chain)\s*=.*?)\s*$/;

function parse(text: string): RouteRule[] {
  const out: RouteRule[] = [];
  const byId = new Map<string, { rule: RouteRule; lines: string[] }>();
  let current: { rule: RouteRule; lines: string[] } | null = null;

  for (const line of String(text ?? "").replace(/\r/g, "").split("\n")) {
    const header = line.match(HEADER);
    if (header) {
      const id = header[1];
      const seen = byId.get(id);
      if (seen) {
        current = seen;
      } else {
        const entry = {
          rule: { id, strict: true, viaChain: false, text: "" },
          lines: [] as string[],
        };
        byId.set(id, entry);
        out.push(entry.rule);
        current = entry;
      }
      continue;
    }
    if (!current) continue;
    const meta = line.match(META);
    if (meta) {
      for (const part of meta[1].split(/\s+/)) {
        const [key, value] = part.split("=");
        if (value === undefined) continue;
        const on = /^(1|true|yes|on)$/i.test(value);
        if (key === "strict") current.rule.strict = on;
        if (key === "via_chain") current.rule.viaChain = on;
      }
      continue;
    }
    current.lines.push(line);
  }

  for (const { rule, lines } of byId.values()) {
    rule.text = lines.join("\n").replace(/^\n+|\n+$/g, "");
  }
  return out;
}

function serialize(list: RouteRule[]): string {
  return list
    .map((rule) => {
      const id = rule.id.replace(/[^A-Za-z0-9_-]/g, "");
      const body = rule.text.replace(/\r/g, "").trim();
      if (!id || !body) return "";
      return [
        `// === route:${id} ===`,
        `// meta: strict=${rule.strict ? 1 : 0} via_chain=${rule.viaChain ? 1 : 0}`,
        body,
      ].join("\n");
    })
    .filter(Boolean)
    .join("\n\n");
}

watch(
  () => [props.open, props.text] as const,
  ([open]) => {
    if (open) rules.value = parse(props.text);
  },
  { immediate: true },
);

function nameOf(id: string): string {
  return props.targets.find((t) => t.id === id)?.name ?? id;
}

function isChain(id: string): boolean {
  return props.targets.find((t) => t.id === id)?.isChain === true;
}

/* В одном блоке — один профиль: выбор, уже занятый другим блоком, не предлагаем. */
function optionsFor(index: number): RouteTargetOption[] {
  const taken = new Set(
    rules.value.filter((_, i) => i !== index).map((r) => r.id).filter(Boolean),
  );
  return props.targets.filter((t) => !taken.has(t.id));
}

const profileOptions = computed(() => props.targets.filter((t) => !t.isChain));
const chainOptions = computed(() => props.targets.filter((t) => t.isChain));

function add() {
  rules.value = [...rules.value, { id: "", strict: true, viaChain: false, text: "" }];
}

function remove(index: number) {
  rules.value = rules.value.filter((_, i) => i !== index);
}

const ready = computed(() => rules.value.filter((r) => r.id && r.text.trim()).length);
</script>

<template>
  <DrawerSheet :open="open" title="Отдельные маршруты" wide @close="emit('close')">
    <template #sticky>
      <p class="hint">
        <span>Эти сайты пойдут не через основное подключение, а через выбранный
          для них VPN или прокси.</span>
        <b class="num">{{ loading ? "загружаю…" : `${ready} из ${rules.length}` }}</b>
      </p>
    </template>

    <p v-if="loading" class="note">Загружаю маршруты…</p>
    <p v-else-if="!rules.length" class="empty">
      Маршрутов пока нет. Добавьте блок и укажите, какие сайты должны ходить
      через отдельное подключение.
    </p>

    <div v-for="(rule, i) in rules" :key="i" class="card">
      <div class="row">
        <select
          v-model="rule.id"
          class="sel"
          aria-label="Куда направить эти сайты"
        >
          <option value="">Выберите подключение…</option>
          <optgroup v-if="profileOptions.length" label="Профили">
            <option
              v-for="t in optionsFor(i).filter((o) => !o.isChain)"
              :key="t.id"
              :value="t.id"
            >
              {{ t.name }}
            </option>
          </optgroup>
          <optgroup v-if="chainOptions.length" label="Цепочки">
            <option
              v-for="t in optionsFor(i).filter((o) => o.isChain)"
              :key="t.id"
              :value="t.id"
            >
              {{ t.name }}
            </option>
          </optgroup>
          <option
            v-if="rule.id && !targets.some((t) => t.id === rule.id)"
            :value="rule.id"
          >
            {{ rule.id }} (профиль не найден)
          </option>
        </select>
        <button class="del" type="button" @click="remove(i)">Убрать</button>
      </div>

      <label class="chk">
        <input v-model="rule.strict" type="checkbox" />
        <span>
          Только этим путём: если выбранное подключение не работает, такой трафик
          не пойдёт ни в основной VPN, ни напрямую — соединение просто оборвётся.
        </span>
      </label>

      <label class="chk" :class="{ off: isChain(rule.id) }">
        <input v-model="rule.viaChain" type="checkbox" :disabled="isChain(rule.id)" />
        <span v-if="isChain(rule.id)">
          Выбрана цепочка — порядок промежуточных узлов задан ею самой.
        </span>
        <span v-else>
          Сначала через текущее подключение, затем через выбранное здесь.
        </span>
      </label>

      <textarea
        v-model="rule.text"
        class="ta mono"
        spellcheck="false"
        autocapitalize="off"
        autocomplete="off"
        autocorrect="off"
        :aria-label="`Сайты для ${nameOf(rule.id) || 'маршрута'}`"
        placeholder="// заметка к блоку&#10;example.com&#10;*.example.com&#10;203.0.113.0/24"
      ></textarea>
      <p class="cnt">{{ entriesLabel(countEntries(rule.text)) }}</p>
    </div>

    <UiButton class="add" @click="add">Добавить маршрут</UiButton>

    <p class="note">
      Сохранение перестраивает конфигурацию и перезапускает подключение — это
      может занять до минуты. Блоки без выбранного подключения или без записей
      не сохраняются.
    </p>

    <template #footer>
      <UiButton
        variant="primary"
        :busy="busy"
        :disabled="loading || busy"
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
.sel {
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
.chk {
  display: flex;
  gap: 9px;
  align-items: flex-start;
  font-size: 13px;
  color: var(--dim);
  cursor: pointer;
  /* Цель нажатия — вся строка вместе с текстом. */
  min-height: 44px;
  padding: 2px 0;
}
.chk.off {
  opacity: 0.6;
}
.chk input {
  width: 18px;
  height: 18px;
  margin-top: 2px;
  flex: none;
  accent-color: var(--accent);
}
.ta {
  width: 100%;
  min-height: 132px;
  resize: vertical;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--ink);
  padding: 9px 11px;
  font-size: 16px;
  line-height: 1.45;
  outline: none;
}
.ta:focus {
  border-color: var(--accent);
}
.cnt {
  font-size: 12px;
  color: var(--faint);
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
