import { requestJson, requestText, utf8ToBase64 } from "./client";

export type PanelRole = "admin" | "vpn";

export interface PanelUser {
  user: string;
  role: PanelRole;
  /** Сколько клиентов VPN-сервера закреплено за логином. */
  clients: number;
}

export interface SetupStatus {
  setup_required: boolean;
}

export const auth = {
  setupStatus: () => requestJson<SetupStatus>("panel_setup_status"),

  /** role: admin — вся панель, vpn — только свои клиенты VPN-сервера. Нет поля — старый роутер. */
  checkAuth: () => requestJson<{ ok: boolean; user?: string; role?: PanelRole }>("check_auth"),

  users: () =>
    requestJson<{ ok: boolean; me: string; users: PanelUser[] }>("panel_users"),
  userAdd: (user: string, password: string, role: PanelRole) =>
    requestJson<{ ok: boolean }>("panel_user_add", { body: { user, password, role } }),
  userSet: (user: string, patch: { password?: string; role?: PanelRole }) =>
    requestJson<{ ok: boolean }>("panel_user_set", { body: { user, ...patch } }),
  userDel: (user: string) => requestJson<{ ok: boolean }>("panel_user_del", { body: { user } }),

  /** CGI ждёт base64(login + "\n" + пароль) сырым телом. */
  login: (user: string, password: string) =>
    requestJson<{ ok: boolean; error?: string }>("login", {
      body: utf8ToBase64(`${user}\n${password}`),
      /* Неверный пароль бэкенд наказывает паузой в 2 секунды. */
      timeoutMs: 30_000,
    }),

  firstSetup: (user: string, password: string) =>
    requestJson<{ ok: boolean }>("panel_first_setup", {
      body: { user, password },
    }),

  logout: () => requestText("logout", { allowEmpty: true }),

  changePassword: (p: {
    old_password: string;
    new_password: string;
    new_user?: string;
  }) =>
    requestJson<{ ok: boolean; user_changed?: boolean }>(
      "panel_change_password",
      { body: p, timeoutMs: 30_000 },
    ),
};
