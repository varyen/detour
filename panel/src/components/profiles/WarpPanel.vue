<script setup lang="ts">
/* Cloudflare WARP последним хопом: клиент → ваш VPN → WARP → сайт. Нужен там,
   где сайт отказывает адресам дата-центров.

   Регистрация идёт ЧЕРЕЗ выбранный VPN: API Cloudflare из России на прямые
   запросы не отвечает. Она отсоединена (роутер отвечает «начал» и уходит
   работать), поэтому исход узнаём опросом warp_status. */
import { computed, onMounted, ref } from "vue";
import TileCard from "@/components/TileCard.vue";
import UiButton from "@/components/UiButton.vue";
import PField from "@/components/profiles/PField.vue";
import { poll, warp } from "@/api";
import type { WarpStatus } from "@/api";
import { useProfilesStore } from "@/stores/profiles";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";
import { fmtAgo } from "@/lib/format";

/** Хелпер отдаёт про профиль больше, чем объявлено в контракте панели. */
type WarpProfile = {
  id: string;
  name?: string;
  address?: string;
  account_type?: string;
  chain_id?: string;
  chain_name?: string;
};
type WarpLast = { state?: string; message?: string; error?: string; ts?: number };

const emit = defineEmits<{ supported: [boolean] }>();

const store = useProfilesStore();
const status = useStatusStore();
const toast = useToastStore();

const data = ref<WarpStatus | null>(null);
const busy = ref("");
const stage = ref("");
const via = ref("");

const supported = computed(() => !!data.value && data.value.supported !== false);
const profiles = computed(() => (data.value?.profiles ?? []) as WarpProfile[]);
const last = computed(() => (data.value?.last ?? {}) as WarpLast);

const viaLabel = computed(() => {
  if (via.value) return store.rows.find((r) => r.id === via.value)?.name ?? via.value;
  const chain = store.activeChain;
  if (!chain.length) return "активная цепочка не выбрана";
  return chain.map((id) => store.rows.find((r) => r.id === id)?.name ?? id).join(" → ");
});

async function load() {
  data.value = await warp.status().catch(() => null);
  emit("supported", supported.value);
}

async function register() {
  if (!store.activeChain.length && !via.value) {
    toast.error("Сначала выберите VPN, через который пойдёт регистрация");
    return;
  }
  busy.value = "reg";
  stage.value = "Отправляю запрос на роутер…";
  try {
    await warp.register(via.value);
    const done = await poll(() => warp.status(), {
      intervalMs: 3000,
      timeoutMs: 300_000,
      done: (s) => {
        const st = (s?.last ?? {}) as WarpLast;
        return st.state === "done" || st.state === "error";
      },
      onTick: (s) => {
        const st = (s?.last ?? {}) as WarpLast;
        stage.value = st.message || "Регистрирую устройство…";
        data.value = s;
      },
    });
    const st = ((done?.last ?? {}) as WarpLast) || {};
    if (st.state === "done") toast.ok(st.message || "WARP зарегистрирован");
    else toast.error(st.message || st.error || "Регистрация не удалась");
  } catch (e) {
    toast.fromError(e, "Регистрация не удалась");
  } finally {
    busy.value = "";
    stage.value = "";
    await load();
    await store.load(true);
    await store.loadChains();
    void status.refresh(true);
  }
}

async function verify(p: WarpProfile) {
  busy.value = `v:${p.id}`;
  stage.value = "Проверяю цепочку с WARP…";
  try {
    await warp.verify(p.id);
    const done = await poll(() => warp.status(), {
      intervalMs: 3000,
      timeoutMs: 240_000,
      done: (s) => {
        const st = (s?.last ?? {}) as WarpLast;
        return st.state === "done" || st.state === "error";
      },
      onTick: (s) => {
        const st = (s?.last ?? {}) as WarpLast;
        stage.value = st.message || "Проверяю…";
        data.value = s;
      },
    });
    const st = ((done?.last ?? {}) as WarpLast) || {};
    if (st.state === "done") toast.ok(st.message || "WARP работает");
    else toast.error(st.message || st.error || "Проверка не прошла");
  } catch (e) {
    toast.fromError(e, "Проверка не удалась");
  } finally {
    busy.value = "";
    stage.value = "";
    await load();
  }
}

async function remove(p: WarpProfile) {
  if (!window.confirm(`Удалить учётную запись WARP «${p.name || p.id}»?`)) return;
  busy.value = `d:${p.id}`;
  try {
    await warp.remove(p.id);
    toast.ok("Учётная запись WARP удалена");
    await load();
    await store.load(true);
    await store.loadChains();
  } catch (e) {
    toast.fromError(e, "Не удалось удалить");
  } finally {
    busy.value = "";
  }
}

onMounted(load);
</script>

<template>
  <TileCard v-if="supported" title="Cloudflare WARP">
    <p class="lead">
      WARP ставится последним хопом: клиент → ваш VPN → WARP → сайт. Выходной
      адрес перестаёт выглядеть адресом дата-центра — именно это обычно нужно
      сервисам, которые отказывают хостингам.
    </p>
    <p class="lead">
      Регистрация идёт через выбранный VPN: на прямые запросы из России API
      Cloudflare не отвечает. Роутер сам сгенерирует ключи, заведёт бесплатное
      устройство и соберёт готовую цепочку.
    </p>

    <p v-if="!profiles.length" class="empty">Учётных записей WARP пока нет.</p>
    <ul v-else class="wlist">
      <li v-for="p in profiles" :key="p.id">
        <div class="wt">
          <p class="wname">{{ p.name || p.id }}</p>
          <p class="meta">
            <span v-if="p.account_type">тариф {{ p.account_type }}</span>
            <span v-if="p.address" class="mono">{{ p.address }}</span>
            <span v-if="p.chain_name">в цепочке «{{ p.chain_name }}»</span>
          </p>
        </div>
        <div class="wact">
          <UiButton :busy="busy === `v:${p.id}`" :disabled="!!busy" @click="verify(p)">
            Проверить
          </UiButton>
          <UiButton variant="danger" :busy="busy === `d:${p.id}`" :disabled="!!busy" @click="remove(p)">
            Удалить
          </UiButton>
        </div>
      </li>
    </ul>

    <div class="reg">
      <PField label="Через какой VPN регистрировать" hint="Пусто — через текущую активную цепочку">
        <select v-model="via">
          <option value="">текущая: {{ viaLabel }}</option>
          <option v-for="r in store.rows" :key="r.id" :value="r.id">
            {{ r.name }}
          </option>
        </select>
      </PField>
    </div>

    <p v-if="stage" class="stage">{{ stage }}</p>
    <p v-else-if="last.state === 'error'" class="err">
      Прошлая попытка: {{ last.message || last.error }}
      <template v-if="last.ts"> ({{ fmtAgo(last.ts) }})</template>
    </p>

    <template #actions>
      <UiButton variant="primary" :busy="busy === 'reg'" :disabled="!!busy" @click="register">
        Зарегистрировать устройство
      </UiButton>
      <span class="hint">Занимает до пары минут — роутер поднимает временный туннель</span>
    </template>
  </TileCard>
</template>

<style scoped>
.lead,
.empty {
  font-size: 13px;
  color: var(--dim);
}
.empty {
  padding: 6px 0;
}
.wlist {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.wlist li {
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.wt {
  flex: 1 1 220px;
  min-width: 0;
}
.wname {
  font-size: 14.5px;
}
.meta {
  font-size: 12px;
  color: var(--faint);
  display: flex;
  gap: 4px 12px;
  flex-wrap: wrap;
}
.wact {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.reg {
  margin-top: 10px;
  max-width: 420px;
}
.stage {
  font-size: 13px;
  color: var(--accent);
}
.err {
  font-size: 12.5px;
  color: var(--bad);
  overflow-wrap: anywhere;
}
.hint {
  font-size: 12px;
  color: var(--faint);
}
</style>
