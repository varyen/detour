/* Мелкие преобразования для показа. Отдельным файлом, потому что shell-CGI
   отвечает не так аккуратно, как хотелось бы: числа приходят строками, а
   отсутствующее значение — строкой "null". Без единой проверки в интерфейсе
   вылезает «порт null». */

/** Значение реально задано? "null", "", "0" от BusyBox считаются пустыми. */
export function isSet(v: unknown): boolean {
  if (v === null || v === undefined) return false;
  const s = String(v).trim();
  return s !== "" && s !== "null" && s !== "0" && s !== "-";
}

/** Число из поля, которое может прийти строкой. */
export function asNum(v: unknown, fallback = 0): number {
  const n = typeof v === "number" ? v : Number(String(v ?? "").trim());
  return Number.isFinite(n) ? n : fallback;
}

/** «1 041» — узкий неразрывный пробел, чтобы число не рвалось по строкам. */
export function fmtInt(n: number): string {
  return new Intl.NumberFormat("ru-RU").format(Math.round(n));
}

export function fmtBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 Б";
  const units = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const v = bytes / Math.pow(1024, i);
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function fmtBitrate(bytesPerSec: number): string {
  const bits = bytesPerSec * 8;
  if (bits >= 1e9) return `${(bits / 1e9).toFixed(1)} Гбит/с`;
  if (bits >= 1e6) return `${(bits / 1e6).toFixed(1)} Мбит/с`;
  if (bits >= 1e3) return `${Math.round(bits / 1e3)} Кбит/с`;
  return `${Math.round(bits)} бит/с`;
}

/** «5 минут назад» — для отметок времени в секундах (unix). */
export function fmtAgo(tsSeconds?: number): string {
  if (!tsSeconds) return "";
  const diff = Math.max(0, Date.now() / 1000 - tsSeconds);
  if (diff < 60) return "только что";
  if (diff < 3600) return `${Math.round(diff / 60)} мин назад`;
  if (diff < 86400) return `${Math.round(diff / 3600)} ч назад`;
  return `${Math.round(diff / 86400)} сут назад`;
}

/**
 * Дата в человеческом виде. `openssl x509 -enddate` отдаёт «Oct 27 05:29:33 2026
 * GMT» — показывать это как есть на русском экране незачем. Что не разобралось,
 * возвращаем нетронутым: соврать датой хуже, чем показать сырую строку.
 */
export function fmtDate(raw?: string): string {
  const s = String(raw ?? "").trim();
  if (!s) return "";
  const ms = Date.parse(s.endsWith(" GMT") ? s.slice(0, -4) + " UTC" : s);
  if (!Number.isFinite(ms)) return s;
  return new Intl.DateTimeFormat("ru-RU", {
    day: "numeric",
    month: "long",
    year: "numeric",
  }).format(new Date(ms));
}
