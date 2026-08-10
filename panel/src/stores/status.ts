import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { AuthError, overview } from "@/api";
import type { BypassStatus, StatusResponse, UdpVpnResponse } from "@/api";

/** Пока панель открыта, статус подтягивается сам — но не чаще, чем нужно. */
const POLL_MS = 8000;

export const useStatusStore = defineStore("status", () => {
  const data = ref<StatusResponse | null>(null);
  const bypass = ref<BypassStatus | null>(null);
  const udp = ref<UdpVpnResponse | null>(null);
  const loading = ref(false);
  const error = ref("");
  const lastLoaded = ref(0);

  let timer: number | undefined;

  const platform = computed(() => data.value?.platform ?? "openwrt");
  const isKeenetic = computed(() => platform.value === "keenetic");

  /* Возможности определяем по ответу бэкенда, а не по платформе: MT6000 —
     тоже openwrt, но без аппаратного офлоада, а проброс зависит от nginx и
     выпущенного сертификата. Единственное честное исключение — NFQUEUE. */
  const zapret2Supported = computed(
    () => !isKeenetic.value && data.value?.binaries?.nfqws2_supported !== false,
  );
  const udpVpnSupported = computed(() => udp.value?.supported !== false && !isKeenetic.value);

  const singboxRunning = computed(() => data.value?.singbox?.running === true);
  const activeProfile = computed(() => data.value?.singbox?.active_profile ?? "");
  const activeChain = computed(() => data.value?.singbox?.active_chain ?? []);

  /* Роутер отдаёт версию установленного пакета, сборка знает свою. Расхождение
     значит одно: браузер держит старую оболочку. uhttpd не шлёт Cache-Control,
     и без этой проверки человек после обновления панели видел бы прежний
     интерфейс, пока кэш сам не протухнет. Перезагружаемся ОДИН раз за вкладку
     и только предварительно перечитав index.html мимо кэша. */
  const STALE_FLAG = "detour:shell-reload";

  async function reloadIfStale(routerVersion?: string) {
    if (import.meta.env.DEV) return;
    if (!routerVersion || routerVersion === __PANEL_BUILD__) return;
    if (sessionStorage.getItem(STALE_FLAG)) return;
    sessionStorage.setItem(STALE_FLAG, routerVersion);
    try {
      await fetch(location.pathname, { cache: "reload" });
    } catch {
      /* Не достучались — перезагрузка всё равно не повредит. */
    }
    location.reload();
  }

  async function refresh(quiet = false) {
    if (!quiet) loading.value = true;
    try {
      data.value = await overview.status();
      error.value = "";
      lastLoaded.value = Date.now();
      void reloadIfStale(data.value?.version);
    } catch (e) {
      if (e instanceof AuthError) return;
      error.value = e instanceof Error ? e.message : "Не удалось получить статус";
    } finally {
      loading.value = false;
    }
  }

  /** Второстепенные состояния — отдельным заходом, чтобы не тормозить первый экран. */
  async function refreshExtras() {
    const [b, u] = await Promise.allSettled([
      overview.bypassStatus(),
      overview.udpVpnGet(),
    ]);
    if (b.status === "fulfilled" && b.value) bypass.value = b.value;
    if (u.status === "fulfilled") udp.value = u.value;
  }

  function startPolling() {
    stopPolling();
    timer = window.setInterval(() => {
      /* Во вкладке на фоне опрашивать роутер незачем. */
      if (document.visibilityState === "visible") void refresh(true);
    }, POLL_MS);
  }

  function stopPolling() {
    if (timer) window.clearInterval(timer);
    timer = undefined;
  }

  return {
    data,
    bypass,
    udp,
    loading,
    error,
    lastLoaded,
    platform,
    isKeenetic,
    zapret2Supported,
    udpVpnSupported,
    singboxRunning,
    activeProfile,
    activeChain,
    refresh,
    refreshExtras,
    startPolling,
    stopPolling,
  };
});
