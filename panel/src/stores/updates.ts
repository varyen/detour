import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { diag } from "@/api";
import type { UpdateChannelState, UpdatesOverview } from "@/api";

/* Каналы обновления одни и те же на всех экранах, поэтому и названия, и
   состояние живут здесь: «Обзор» показывает, что обновиться есть чем, а
   «Журнал» — чем именно и кнопками. Раньше сводку читал только «Журнал», и
   главная страница молчала о вышедшей версии вовсе. */
export type UpdateChannel = "panel" | "singbox" | "tpws" | "nfqws2";

export const CHANNEL_TITLE: Record<UpdateChannel, string> = {
  panel: "Панель",
  singbox: "sing-box",
  tpws: "tpws (обход DPI)",
  nfqws2: "nfqws2 (zapret2)",
};

export const CHANNELS: UpdateChannel[] = ["panel", "singbox", "tpws", "nfqws2"];

export interface HotChannel {
  key: UpdateChannel;
  title: string;
  current: string;
  available: string;
}

/** Версия, которую нельзя показывать: обновлятор пишет так «не знаю». */
export function realVersion(v?: string): string {
  const s = String(v ?? "").trim();
  return s && s !== "?" && s !== "n/a" ? s : "";
}

/**
 * Ответ на ручную проверку — это сырой state-файл обновлятора, а не сводка:
 * у фид-каналов версии лежат в current/available, вердикта update_available
 * там нет вообще, а об ошибке говорит status/message. Без приведения к виду
 * updates_overview строка после проверки теряла и доступную версию, и зелёный
 * чип «Есть обновление».
 */
export function normalizeChannel(
  ch: UpdateChannel,
  r: UpdateChannelState,
): UpdateChannelState {
  const out: UpdateChannelState = { ...r };
  const cur = realVersion(r.current_version ?? r.current);
  const avail = realVersion(r.available_version ?? r.available);
  if (cur) out.current_version = cur;
  if (avail) out.available_version = avail;
  if (out.update_available === undefined) {
    /* Панель сообщает вердикт статусом, фид-каналы — расхождением версий
       (тот же критерий, что у feed_upd в CGI). */
    out.update_available =
      ch === "panel" ? r.status === "update_available" : !!cur && !!avail && cur !== avail;
  }
  if (!out.error && r.status === "error") out.error = r.message || "проверка не удалась";
  return out;
}

export const useUpdatesStore = defineStore("updates", () => {
  const data = ref<UpdatesOverview | null>(null);
  const loading = ref(false);
  /** Когда сводку в последний раз забирали С РОУТЕРА (не когда роутер ходил в GitHub). */
  const loadedAt = ref(0);

  /* Сводка меняется раз в шесть часов (cron), поэтому чаще минуты её тянуть
     незачем: на «Обзор» заходят и по десять раз подряд. */
  const FRESH_MS = 60_000;

  const hot = computed<HotChannel[]>(() =>
    CHANNELS.filter((c) => data.value?.[c]?.update_available === true).map((c) => {
      const st = data.value?.[c] ?? {};
      return {
        key: c,
        title: CHANNEL_TITLE[c],
        current: realVersion(st.current_version ?? st.current),
        available: realVersion(st.available_version ?? st.available),
      };
    }),
  );

  const available = computed(() => hot.value.length > 0);
  /** Обновление самой панели — единственное, ради которого стоит звать по имени. */
  const panelHot = computed(() => hot.value.find((h) => h.key === "panel") ?? null);

  async function load(force = false) {
    if (!force && data.value && Date.now() - loadedAt.value < FRESH_MS) return;
    if (loading.value) return;
    loading.value = true;
    try {
      const o = await diag.updatesOverview();
      if (o) {
        data.value = o;
        loadedAt.value = Date.now();
      }
    } catch {
      /* Сводка — не то, ради чего стоит показывать ошибку на главной: канал
         обновлений мог просто не отвечать. Экран «Журнал» скажет подробнее. */
    } finally {
      loading.value = false;
    }
  }

  /** Ручная проверка одного канала в «Журнале» должна двигать и главную. */
  function setChannel(ch: UpdateChannel, state: UpdateChannelState) {
    data.value = { ...(data.value ?? {}), [ch]: state };
  }

  return { data, loading, loadedAt, hot, available, panelHot, load, setChannel };
});
