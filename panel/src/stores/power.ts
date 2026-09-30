import { defineStore } from "pinia";
import { ref } from "vue";
import { diag, poll, services } from "@/api";
import type { ApplyLogResponse, PowerCheck } from "@/api";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";

type Kind = "off" | "on" | "uninstall";

/* Полная остановка, включение и удаление панели. Все три уходят на роутере в
   фон (перезапуск dnsmasq, снос ipset'ов и пакетов — дольше HTTP-таймаута), ход
   виден в apply_log. Стор общий: баннер «панель остановлена» и раздел
   «Сервисы» показывают одну и ту же операцию. */
export const usePowerStore = defineStore("power", () => {
  const busy = ref<Kind | "">("");
  const log = ref("");
  /** Удаление дошло до конца: CGI больше не отвечает — панели на роутере нет. */
  const removed = ref(false);
  /** Снимок «что сейчас живо» — им остановка проверяется без SSH. */
  const check = ref<PowerCheck | null>(null);
  const checking = ref(false);

  const status = useStatusStore();
  const toast = useToastStore();

  async function loadCheck(quiet = false) {
    checking.value = true;
    try {
      check.value = await services.powerCheck();
    } catch (e) {
      if (!quiet) toast.fromError(e, "Не удалось проверить");
    } finally {
      checking.value = false;
    }
  }

  async function run(kind: "off" | "on") {
    if (busy.value) return;
    busy.value = kind;
    log.value = "";
    try {
      await (kind === "off" ? services.powerOff() : services.powerOn());
      const r = await poll<ApplyLogResponse | null>(() => diag.applyLog(), {
        done: (v) => v?.done === true,
        intervalMs: 1500,
        timeoutMs: 240_000,
        onTick: (v) => {
          if (v && typeof v.log === "string") log.value = v.log;
        },
      });
      const rc = Number(String(r?.rc ?? "").trim() || NaN);
      if (r?.done === true && rc === 0) {
        toast.ok(
          kind === "off"
            ? "Detour остановлен: правила сняты, трафик идёт напрямую"
            : "Detour снова работает",
        );
      } else {
        toast.error(kind === "off" ? "Остановка завершилась с ошибкой" : "Включение завершилось с ошибкой");
      }
    } catch (e) {
      toast.fromError(e, kind === "off" ? "Не удалось остановить" : "Не удалось включить");
    } finally {
      busy.value = "";
      await Promise.allSettled([status.refresh(true), loadCheck(true)]);
    }
  }

  async function uninstall(keepEngines: boolean) {
    if (busy.value) return;
    busy.value = "uninstall";
    log.value = "";
    try {
      await services.selfUninstall(keepEngines);
    } catch (e) {
      busy.value = "";
      toast.fromError(e, "Не удалось запустить удаление");
      return;
    }
    status.stopPolling();
    /* Пакет панели сносится на втором шаге — вместе с CGI, поэтому код возврата
       сюда обычно не доходит. Признак конца — роутер перестал отвечать на
       apply_log несколько раз подряд, уже после того как лог пошёл. */
    const deadline = Date.now() + 420_000;
    let misses = 0;
    let seen = false;
    for (;;) {
      try {
        const v = await diag.applyLog();
        if (v && typeof v.log === "string") {
          log.value = v.log;
          seen = true;
          misses = 0;
        } else if (seen) {
          misses++;
        }
        if (v?.done === true) {
          /* Код возврата дошёл — значит, CGI жив и удаление прервалось. */
          if (Number(String(v.rc ?? "").trim()) !== 0) {
            busy.value = "";
            toast.error("Удаление прервано — Detour остановлен, подробности в журнале");
            status.startPolling();
            await status.refresh(true);
            return;
          }
          break;
        }
      } catch {
        if (seen) misses++;
      }
      if (misses >= 3 || Date.now() > deadline) break;
      await new Promise((r) => setTimeout(r, 1500));
    }
    busy.value = "";
    removed.value = true;
  }

  return { busy, log, removed, check, checking, loadCheck, run, uninstall };
});
