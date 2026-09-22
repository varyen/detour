/* Транспорт до shell-CGI на роутере.
 *
 * Особенности бэкенда, которые здесь учтены (все проверены по detour-api):
 *  - тело POST всегда отправляется как text/plain: CGI не умеет разбирать
 *    form-encoded, он читает сырой поток;
 *  - Content-Type ответа ВСЕГДА application/json, даже когда тело — обычный
 *    текст (bypass_strategy GET) или пусто (упавший хелпер). Слепой res.json()
 *    падал бы, поэтому парсер терпимый;
 *  - на самообновлении панели lighttpd/uhttpd, который держит этот же CGI,
 *    перезапускается — ответ приходит обрезанным или не приходит вовсе. Это не
 *    ошибка, а ожидаемый исход, вызывающий код должен уметь его отличить;
 *  - любое действие без сессии отвечает 401 {"ok":false,"error":"auth"}.
 */

import { translateApiError } from "./messages";

export const API_URL = "/cgi-bin/detour-api";

/** Сколько ждём ответа, если действие не объявило свой таймаут. */
const DEFAULT_TIMEOUT_MS = 20_000;

/* Кто отвечает на запросы: в клиенте это служба на самом устройстве. */
const HOST_SILENT = __CLIENT__ ? "Служба Detour не ответила" : "Роутер не ответил";
const HOST_SAID = __CLIENT__ ? "Служба Detour ответила" : "Роутер ответил";
const HOST_UNREACHABLE = __CLIENT__ ? "Нет связи со службой Detour" : "Нет связи с роутером";

export class AuthError extends Error {
  constructor() {
    super("Требуется вход");
    this.name = "AuthError";
  }
}

export class LockedOutError extends Error {
  constructor(public retryAfter: number) {
    super("Слишком много попыток входа");
    this.name = "LockedOutError";
  }
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status?: number,
    public raw?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/** Ответ пришёл пустым/обрезанным — веб-сервер перезапускается. */
export class ServerRestartingError extends Error {
  constructor() {
    super("Панель перезапускается");
    this.name = "ServerRestartingError";
  }
}

type Listener = () => void;
const authListeners = new Set<Listener>();

/** Подписка на «сессия протухла» — стор сессии показывает экран входа. */
export function onAuthRequired(fn: Listener): () => void {
  authListeners.add(fn);
  return () => authListeners.delete(fn);
}

export interface RequestOptions {
  /** Дополнительные query-параметры (id, name, eligible, …). */
  params?: Record<string, string | number | boolean | undefined>;
  /**
   * Тело: строка уходит как есть, Blob/File/ArrayBuffer — байт в байт (загрузка
   * .ipk, импорт конфигурации), всё остальное — через JSON.stringify.
   */
  body?: unknown;
  method?: "GET" | "POST";
  timeoutMs?: number;
  signal?: AbortSignal;
  /** Пустое тело — это нормальный ответ, а не признак перезапуска. */
  allowEmpty?: boolean;
}

/**
 * Двоичное тело нельзя сериализовать: `JSON.stringify(blob)` даёт «{}», и на
 * роутер вместо .ipk уезжают два байта. Такие тела уходят в fetch как есть.
 */
function isBinaryBody(b: unknown): b is BodyInit {
  return (
    b instanceof Blob ||
    b instanceof ArrayBuffer ||
    ArrayBuffer.isView(b) ||
    b instanceof FormData ||
    b instanceof URLSearchParams ||
    (typeof ReadableStream !== "undefined" && b instanceof ReadableStream)
  );
}

function encodeBody(b: unknown): BodyInit | undefined {
  if (b === undefined) return undefined;
  if (typeof b === "string") return b;
  if (isBinaryBody(b)) return b;
  return JSON.stringify(b);
}

/**
 * FormData/URLSearchParams заголовок ставят себе сами (boundary/кодировка) —
 * туда лезть нельзя. Всё остальное CGI читает со stdin, ему важна лишь длина.
 */
function bodyHeaders(b: unknown): Record<string, string> {
  if (b === undefined) return {};
  if (b instanceof FormData || b instanceof URLSearchParams) return {};
  if (isBinaryBody(b)) return { "Content-Type": "application/octet-stream" };
  return { "Content-Type": "text/plain" };
}

function buildUrl(
  action: string,
  params?: RequestOptions["params"],
): string {
  const u = new URL(API_URL, location.origin);
  u.searchParams.set("action", action);
  for (const [k, v] of Object.entries(params ?? {})) {
    if (v === undefined) continue;
    u.searchParams.set(k, String(v));
  }
  return u.pathname + u.search;
}

interface RawResponse {
  status: number;
  text: string;
  retryAfter?: string | null;
}

async function viaFetch(action: string, opts: RequestOptions): Promise<RawResponse> {
  const method = opts.method ?? (opts.body !== undefined ? "POST" : "GET");
  const ctrl = new AbortController();
  const timer = setTimeout(
    () => ctrl.abort(),
    opts.timeoutMs ?? DEFAULT_TIMEOUT_MS,
  );
  if (opts.signal) {
    opts.signal.addEventListener("abort", () => ctrl.abort(), { once: true });
  }

  let res: Response;
  try {
    res = await fetch(buildUrl(action, opts.params), {
      method,
      credentials: "same-origin",
      cache: "no-store",
      signal: ctrl.signal,
      headers: bodyHeaders(opts.body),
      body: encodeBody(opts.body),
    });
  } catch (e) {
    clearTimeout(timer);
    if ((e as Error).name === "AbortError") {
      throw new ApiError(`${HOST_SILENT} за отведённое время (${action})`);
    }
    throw new ApiError(`${HOST_UNREACHABLE} (${action})`);
  }
  clearTimeout(timer);

  const text = res.status === 401 ? "" : await res.text().catch(() => "");
  return { status: res.status, text, retryAfter: res.headers.get("Retry-After") };
}

type InvokeBody = { kind: "text" | "base64"; data: string } | null;

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}

async function encodeInvokeBody(b: unknown): Promise<InvokeBody> {
  if (b === undefined) return null;
  if (typeof b === "string") return { kind: "text", data: b };
  if (b instanceof URLSearchParams) return { kind: "text", data: b.toString() };
  if (b instanceof Blob) {
    return { kind: "base64", data: bytesToBase64(new Uint8Array(await b.arrayBuffer())) };
  }
  if (b instanceof ArrayBuffer) return { kind: "base64", data: bytesToBase64(new Uint8Array(b)) };
  if (ArrayBuffer.isView(b)) {
    return { kind: "base64", data: bytesToBase64(new Uint8Array(b.buffer, b.byteOffset, b.byteLength)) };
  }
  if (b instanceof FormData) throw new ApiError("Загрузка форм в приложении не поддерживается");
  return { kind: "text", data: JSON.stringify(b) };
}

/** Приложение Detour: тот же запрос уходит в локальную службу через Tauri. */
async function viaInvoke(action: string, opts: RequestOptions): Promise<RawResponse> {
  const { invoke } = await import("@tauri-apps/api/core");
  const params: Record<string, string> = {};
  for (const [k, v] of Object.entries(opts.params ?? {})) {
    if (v !== undefined) params[k] = String(v);
  }
  const body = await encodeInvokeBody(opts.body);

  let timer: ReturnType<typeof setTimeout> | undefined;
  const limit = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new ApiError(`Служба Detour не ответила за отведённое время (${action})`)),
      opts.timeoutMs ?? DEFAULT_TIMEOUT_MS,
    );
    opts.signal?.addEventListener("abort", () => reject(new ApiError(`Запрос отменён (${action})`)), {
      once: true,
    });
  });
  try {
    const r = await Promise.race([
      invoke<{ status: number; body: string }>("api", { action, params, body }),
      limit,
    ]);
    return { status: r.status, text: r.body };
  } catch (e) {
    if (e instanceof ApiError) throw e;
    throw new ApiError(`Нет связи со службой Detour (${action})`);
  } finally {
    clearTimeout(timer);
  }
}

/* В `--mode client` панель может открыться и в обычном браузере (разработка
   через dev-http службы) — тогда Tauri нет и запросы идут обычным fetch. */
const useInvoke = __CLIENT__ && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Сырой запрос: отдаёт текст ответа. Здесь же обработка 401/429/перезапуска. */
export async function requestText(
  action: string,
  opts: RequestOptions = {},
): Promise<string> {
  const res = useInvoke ? await viaInvoke(action, opts) : await viaFetch(action, opts);

  if (res.status === 401) {
    for (const fn of authListeners) fn();
    throw new AuthError();
  }

  const text = res.text;

  if (res.status === 429) {
    let retry = 60;
    try {
      const j = JSON.parse(text) as { retry_after?: number };
      if (typeof j.retry_after === "number") retry = j.retry_after;
    } catch {
      /* тело не JSON — берём заголовок */
      const h = Number(res.retryAfter);
      if (Number.isFinite(h) && h > 0) retry = h;
    }
    throw new LockedOutError(retry);
  }

  if (res.status < 200 || res.status >= 300) {
    throw new ApiError(`${HOST_SAID} ${res.status} (${action})`, res.status, text);
  }

  if (!text.trim() && !opts.allowEmpty) throw new ServerRestartingError();
  return text;
}

/** Терпимый разбор: не-JSON и пустое тело не считаются падением. */
export function parseLoose<T>(text: string): T | string | null {
  const t = text.trim();
  if (!t) return null;
  if (t[0] !== "{" && t[0] !== "[") return text;
  try {
    return JSON.parse(t) as T;
  } catch {
    return text;
  }
}

/** Конверт {ok:false,error} — единственный способ бэкенда сообщить об ошибке. */
function throwIfEnvelopeError(v: unknown, action: string): void {
  if (!v || typeof v !== "object") return;
  const o = v as { ok?: unknown; error?: unknown };
  if (o.ok === false) {
    const msg =
      typeof o.error === "string" && o.error
        ? translateApiError(o.error)
        : `Ошибка: ${action}`;
    throw new ApiError(msg);
  }
}

/**
 * Запрос, ожидающий JSON-объект. Кидает ApiError на конверте {ok:false}.
 * Для действий, которые отдают сырой файл (config.json, профиль, state-файлы),
 * конверта нет — тогда просто вернётся распарсенный объект.
 */
export async function requestJson<T>(
  action: string,
  opts: RequestOptions = {},
): Promise<T> {
  const text = await requestText(action, opts);
  const v = parseLoose<T>(text);
  if (v === null) throw new ServerRestartingError();
  if (typeof v === "string") {
    throw new ApiError(`Ожидался JSON, пришёл текст (${action})`, undefined, v);
  }
  throwIfEnvelopeError(v, action);
  return v;
}

/** То же, но пустой/битый ответ трактуется как «сервер перезапускается». */
export async function requestJsonTolerant<T>(
  action: string,
  opts: RequestOptions = {},
): Promise<T | null> {
  try {
    return await requestJson<T>(action, opts);
  } catch (e) {
    if (e instanceof ServerRestartingError) return null;
    if (e instanceof ApiError && !e.status) return null;
    throw e;
  }
}

/** Действия, отдающие большой текст внутри JSON-строки, и bypass_strategy. */
export async function requestRawText(
  action: string,
  opts: RequestOptions = {},
): Promise<string> {
  return requestText(action, { allowEmpty: true, ...opts });
}

export interface PollOptions<T> {
  /** Готово? Вернуть true, чтобы остановиться. */
  done: (v: T) => boolean;
  intervalMs?: number;
  timeoutMs?: number;
  /** Вызывается на каждом такте — для прогресса в интерфейсе. */
  onTick?: (v: T) => void;
}

/**
 * Опрос состояния для отсоединённых операций (обновление, сертификат, WARP,
 * swap, полная проверка здоровья). Бэкенд возвращает {started:true} сразу, а
 * результат кладёт в state-файл — другого способа узнать исход нет.
 */
export async function poll<T>(
  fetcher: () => Promise<T>,
  opts: PollOptions<T>,
): Promise<T> {
  const interval = opts.intervalMs ?? 1500;
  const deadline = Date.now() + (opts.timeoutMs ?? 180_000);
  let last: T | undefined;
  for (;;) {
    try {
      last = await fetcher();
      opts.onTick?.(last);
      if (opts.done(last)) return last;
    } catch (e) {
      /* Во время перезапуска сервиса запрос обязан падать — это не конец
         операции, продолжаем опрашивать до дедлайна. */
      if (e instanceof AuthError) throw e;
    }
    if (Date.now() > deadline) {
      if (last !== undefined) return last;
      throw new ApiError("Операция не завершилась за отведённое время");
    }
    await new Promise((r) => setTimeout(r, interval));
  }
}

/** base64 для логина: CGI ждёт base64(user + "\n" + password) в UTF-8. */
export function utf8ToBase64(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}
