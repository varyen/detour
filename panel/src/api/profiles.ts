import { requestJson, requestJsonTolerant } from "./client";
import type {
  Chain,
  ChainsResponse,
  ProfilesListResponse,
  Subscription,
  WarpStatus,
} from "./types";

/* Переключение профиля/цепочки идёт по пути `reconfigure` — он пропускает
   поштучный сброс conntrack (тот занимал ~112 с на большом whitelist), но
   всё равно не мгновенный. */
const SWITCH_TIMEOUT = 90_000;

/**
 * Перевод профиля в другую папку. Отдельной ручки под группы у роутера нет:
 * группа — обычное строковое поле профиля, так что правится тем же
 * `profile_save`, что и всё остальное.
 *
 * Читаем профиль целиком (`profile_get`) и пишем обратно с новым `group`:
 * `profile_save` НЕ сливает поля, а перезаписывает файл телом запроса, поэтому
 * отправить укороченную запись из `profiles_list` (там нет ни outbound, ни
 * ключей) значило бы потерять настройки подключения.
 *
 * Пустая строка — «без папки»: файл-хранилище не различает отсутствие поля и
 * пустое значение, а панель и роутер везде читают `group` через «пусто = нет».
 */
async function setProfileGroup(id: string, group: string): Promise<void> {
  const raw = await requestJson<Record<string, unknown>>("profile_get", {
    params: { name: id },
  });
  /* id восстанавливаем из имени файла: в старых записях поля может не быть, а
     без него profile_save вывел бы имя файла из name — и профиль уехал бы в
     другой файл, оставив прежний на диске. */
  await requestJson<{ ok: boolean }>("profile_save", {
    body: { ...raw, id, group },
  });
}

export const profiles = {
  list: () => requestJson<ProfilesListResponse>("profiles_list", { timeoutMs: 45_000 }),

  get: (name: string) =>
    requestJson<Record<string, unknown>>("profile_get", { params: { name } }),

  save: (profile: Record<string, unknown>) =>
    requestJson<{ ok: boolean }>("profile_save", { body: profile }),

  setGroup: setProfileGroup,

  remove: (name: string) =>
    requestJson<{ ok: boolean }>("profile_delete", {
      method: "POST",
      params: { name },
    }),

  activate: (name: string) =>
    requestJson<{ ok: boolean }>("profile_activate", {
      method: "POST",
      params: { name },
      timeoutMs: SWITCH_TIMEOUT,
    }),

  exportAll: () => requestJson<Record<string, unknown>>("profiles_export"),

  /** Массово разрешить/запретить участие в авто-переключении. */
  setAutoswitch: (ids: string[], eligible: boolean) =>
    requestJson<{ ok: boolean; excluded: number }>("autoswitch_set", {
      params: { eligible: eligible ? 1 : 0 },
      body: ids.join("\n"),
    }),

  setSpeedcheck: (ids: string[], eligible: boolean) =>
    requestJson<{ ok: boolean }>("speedcheck_set", {
      params: { eligible: eligible ? 1 : 0 },
      body: ids.join("\n"),
    }),
};

export const chains = {
  list: () => requestJson<ChainsResponse>("chains_list"),
  save: (chain: Chain) =>
    requestJson<{ ok: boolean }>("chain_save", {
      body: chain,
      timeoutMs: SWITCH_TIMEOUT,
    }),
  remove: (id: string) =>
    requestJson<{ ok: boolean }>("chain_delete", { body: { id } }),
  activate: (profileIds: string[]) =>
    requestJson<{ ok: boolean }>("chain_activate", {
      body: profileIds.join(","),
      timeoutMs: SWITCH_TIMEOUT,
    }),
};

export const subscriptions = {
  list: () =>
    requestJson<{ ok: boolean; subscriptions: Subscription[] }>(
      "subscriptions_list",
    ),

  /** Пробная загрузка подписки — заголовки нужны, чтобы показать лимиты. */
  fetch: (url: string, userAgent?: string) =>
    requestJson<{
      ok: boolean;
      body: string;
      headers: Record<string, string>;
    }>("subscription_fetch", {
      body: { url, user_agent: userAgent },
      timeoutMs: 40_000,
    }),

  saveOne: (sub: Subscription) =>
    requestJson<{ ok: boolean }>("subscription_save_one", { body: sub }),

  deleteOne: (id: string) =>
    requestJson<{ ok: boolean }>("subscription_delete_one", {
      method: "POST",
      params: { id },
    }),

  refreshOne: (id: string) =>
    requestJson<{ ok: boolean; output?: string; error?: string }>(
      "subscription_refresh_one",
      { method: "POST", params: { id }, timeoutMs: 180_000 },
    ),

  refreshAll: () =>
    requestJson<{ ok: boolean; output?: string; error?: string }>(
      "subscription_refresh_all",
      { method: "POST", timeoutMs: 600_000 },
    ),
};

export const warp = {
  status: () => requestJsonTolerant<WarpStatus>("warp_status"),
  /** Регистрация уходит в фон: CF-API из РФ отвечает только через активный VPN. */
  register: (via = "") =>
    requestJson<{ ok: boolean; started: boolean }>("warp_register", {
      body: { via },
    }),
  verify: (id: string) =>
    requestJson<{ ok: boolean; started: boolean }>("warp_verify", {
      body: { id },
    }),
  remove: (id: string) =>
    requestJson<{ ok: boolean }>("warp_delete", { body: { id }, timeoutMs: 45_000 }),
};
