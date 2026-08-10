import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { chains, diag, profiles as profilesApi } from "@/api";
import type {
  Chain,
  HealthResult,
  HealthState,
  PingResult,
  ProfileSummary,
} from "@/api";

export interface ProfileRow extends ProfileSummary {
  ping?: PingResult;
  health?: HealthResult;
  /** Сведённое состояние для точки в списке. */
  state: HealthState;
  isActive: boolean;
}

/** Медленным считаем то, что отвечает, но еле-еле — это видно и без графиков. */
const SLOW_MS = 400;

export const useProfilesStore = defineStore("profiles", () => {
  const items = ref<ProfileSummary[]>([]);
  const chainList = ref<Chain[]>([]);
  const active = ref("");
  const activeChain = ref<string[]>([]);
  const ping = ref<Record<string, PingResult>>({});
  const health = ref<Record<string, HealthResult>>({});
  /* Подписи целей проверки приходят тем же ответом, что и результаты: без них
     health.delays[] — безымянный список чисел. Держим их здесь, чтобы экраны не
     запрашивали health_status повторно ради одних заголовков. */
  const healthTargets = ref<{ label: string; url: string }[]>([]);
  /* Последнее авто-переключение приходит тем же ответом. Роутер мог сменить
     профиль сам, пока панель была закрыта, — без этой отметки человек видит
     чужой активный профиль и не знает, почему он сменился. */
  const healthSwitch = ref<{ from?: string; to?: string; ts?: number } | null>(null);
  const loading = ref(false);
  const switching = ref("");

  function stateOf(id: string): HealthState {
    const h = health.value[id];
    if (h && h.ok === false) return "dead";
    const p = ping.value[id];
    if (!p || p.ok === undefined) return h?.ok ? "ok" : "unknown";
    if (!p.ok) return "dead";
    if (h?.ok === false) return "dead";
    return (p.rtt ?? 0) > SLOW_MS ? "slow" : "ok";
  }

  const rows = computed<ProfileRow[]>(() =>
    items.value
      .map((p) => ({
        ...p,
        ping: ping.value[p.id],
        health: health.value[p.id],
        state: stateOf(p.id),
        isActive: p.id === active.value,
      }))
      /* Алфавит — единственный порядок, который не меняется под руками, пока
         роутер переопрашивает пинги. */
      .sort((a, b) =>
        (a.group ?? "").localeCompare(b.group ?? "", "ru") ||
        a.name.localeCompare(b.name, "ru", { numeric: true }),
      ),
  );

  const byGroup = computed(() => {
    const map = new Map<string, ProfileRow[]>();
    for (const r of rows.value) {
      const g = r.group || "Без группы";
      if (!map.has(g)) map.set(g, []);
      map.get(g)!.push(r);
    }
    return map;
  });

  const activeRow = computed(() => rows.value.find((r) => r.isActive));

  async function load(force = false) {
    if (loading.value) return;
    if (items.value.length && !force) return;
    loading.value = true;
    try {
      const r = await profilesApi.list();
      items.value = r.profiles ?? [];
      active.value = r.active ?? "";
      activeChain.value = r.active_chain ?? [];
    } finally {
      loading.value = false;
    }
  }

  /** Пинги и проверки — отдельно и молча: без них список уже полезен. */
  async function loadProbes() {
    const [p, h] = await Promise.allSettled([diag.pingStatus(), diag.healthStatus()]);
    if (p.status === "fulfilled") ping.value = p.value.results ?? {};
    if (h.status === "fulfilled") {
      health.value = h.value.results ?? {};
      if (h.value.urls?.length) healthTargets.value = h.value.urls;
      healthSwitch.value = h.value.switch ?? null;
    }
  }

  async function loadChains() {
    const r = await chains.list();
    chainList.value = r.chains ?? [];
  }

  async function activate(id: string) {
    switching.value = id;
    try {
      await profilesApi.activate(id);
      active.value = id;
      activeChain.value = [id];
    } finally {
      switching.value = "";
    }
  }

  async function activateChain(ids: string[]) {
    switching.value = ids.join(",");
    try {
      await chains.activate(ids);
      activeChain.value = ids;
      active.value = ids[ids.length - 1] ?? "";
    } finally {
      switching.value = "";
    }
  }

  return {
    items,
    chainList,
    active,
    activeChain,
    ping,
    health,
    healthTargets,
    healthSwitch,
    loading,
    switching,
    rows,
    byGroup,
    activeRow,
    load,
    loadProbes,
    loadChains,
    activate,
    activateChain,
  };
});
