/* CGI отвечает вперемешку: часть сообщений об ошибках по-русски, часть —
   английские строки, оставшиеся с тех пор, когда панель была одна и её читал
   автор. Человеку, который просто хочет включить VPN, «Invalid credentials»
   или «bad chain?» не говорят ничего. Переводим известные ответы здесь, в одном
   месте, а не в каждом экране; незнакомое показываем как есть — соврать
   переводом хуже, чем показать оригинал. */

const EXACT: Record<string, string> = {
  "Invalid credentials": "Неверный логин или пароль",
  "auth": "Нужно войти заново",
  "POST required": "Действие требует отправки формы — обновите страницу",
  "id required": "Не указано, к чему применить действие",
  "url required": "Не указан адрес",
  "profile not found": "Профиль не найден",
  "chain not found": "Цепочка не найдена",
  "not found": "Не найдено",
  "invalid json": "Роутер не понял отправленные данные",
  "no active profile": "Сейчас не выбран ни один VPN",
  "failed to render config (bad chain?)":
    "Не удалось собрать конфигурацию — проверьте активную цепочку",
  "failed to render config": "Не удалось собрать конфигурацию",
  "detour-health not installed": "Проверка работоспособности не установлена",
  "detour-meter not installed": "Счётчики трафика не установлены",
  "detour-cert not installed": "Помощник сертификатов не установлен",
  "detour-portmap not installed": "Проброс сервисов не установлен",
  "detour-warp not installed": "Помощник WARP не установлен",
};

/** Куски, по которым узнаём ответ, если он собран из шаблона. */
const PARTIAL: [RegExp, string][] = [
  [/^url must start with/i, "Адрес должен начинаться с http:// или https://"],
  [/only a-z A-Z 0-9/i, "В имени допустимы только латиница, цифры, точка, дефис и подчёркивание"],
  [/id invalid/i, "Недопустимый идентификатор"],
  [/timeout/i, "Роутер не ответил вовремя"],
  [/no space left/i, "На роутере кончилось место"],
];

export function translateApiError(raw: string): string {
  const s = String(raw ?? "").trim();
  if (!s) return s;
  const exact = EXACT[s] ?? EXACT[s.toLowerCase()];
  if (exact) return exact;
  for (const [re, text] of PARTIAL) if (re.test(s)) return text;
  return s;
}
