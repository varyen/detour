import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { AuthError, LockedOutError, auth, onAuthRequired } from "@/api";
import type { PanelRole } from "@/api";

type Screen = "loading" | "login" | "setup" | "ready";

export const useSessionStore = defineStore("session", () => {
  const screen = ref<Screen>("loading");
  const user = ref("");
  const role = ref<PanelRole>("admin");
  const error = ref("");
  const busy = ref(false);
  /** До какого времени вход заблокирован после серии неудач (unix, мс). */
  const lockedUntil = ref(0);

  const authorized = computed(() => screen.value === "ready");
  /** Админ «только VPN-сервер»: панель сводится к его клиентам. */
  const vpnOnly = computed(() => role.value === "vpn");

  async function loadRole() {
    try {
      const r = await auth.checkAuth();
      role.value = r.role ?? "admin";
    } catch {
      role.value = "admin";
    }
  }

  /* Любой 401 из любого запроса роняет панель на экран входа: сессия живёт в
     tmpfs и исчезает при перезагрузке роутера, так что это штатный путь. */
  onAuthRequired(() => {
    if (screen.value === "ready") {
      screen.value = "login";
      error.value = "Сессия истекла — войдите заново";
    }
  });

  async function bootstrap() {
    screen.value = "loading";
    try {
      const r = await auth.checkAuth();
      user.value = r.user ?? "";
      role.value = r.role ?? "admin";
      screen.value = "ready";
      return;
    } catch (e) {
      if (!(e instanceof AuthError)) {
        /* Роутер недоступен — показываем вход, там будет понятная ошибка. */
        error.value = e instanceof Error ? e.message : "Нет связи с роутером";
      }
    }
    try {
      const s = await auth.setupStatus();
      screen.value = s.setup_required ? "setup" : "login";
    } catch {
      screen.value = "login";
    }
  }

  async function login(u: string, p: string) {
    busy.value = true;
    error.value = "";
    try {
      await auth.login(u, p);
      user.value = u;
      await loadRole();
      lockedUntil.value = 0;
      screen.value = "ready";
      return true;
    } catch (e) {
      if (e instanceof LockedOutError) {
        lockedUntil.value = Date.now() + e.retryAfter * 1000;
        error.value = `Слишком много попыток. Повторите через ${e.retryAfter} с`;
      } else {
        error.value = e instanceof Error ? e.message : "Не удалось войти";
      }
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function firstSetup(u: string, p: string) {
    busy.value = true;
    error.value = "";
    try {
      /* CGI сам ставит cookie после создания учётки — второй вход не нужен. */
      await auth.firstSetup(u, p);
      user.value = u;
      role.value = "admin";
      screen.value = "ready";
      return true;
    } catch (e) {
      error.value = e instanceof Error ? e.message : "Не удалось создать учётную запись";
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function logout() {
    try {
      await auth.logout();
    } finally {
      user.value = "";
      role.value = "admin";
      screen.value = "login";
    }
  }

  return {
    screen,
    user,
    role,
    vpnOnly,
    error,
    busy,
    lockedUntil,
    authorized,
    bootstrap,
    login,
    firstSetup,
    logout,
  };
});
