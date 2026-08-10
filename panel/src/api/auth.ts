import { requestJson, requestText, utf8ToBase64 } from "./client";

export interface SetupStatus {
  setup_required: boolean;
}

export const auth = {
  setupStatus: () => requestJson<SetupStatus>("panel_setup_status"),

  checkAuth: () => requestJson<{ ok: boolean; user?: string }>("check_auth"),

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
