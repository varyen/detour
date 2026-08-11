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

/**
 * Скорость из detour-health — она хранится в кбит/с. Пустая строка означает
 * «не измерялась»: роутер отдаёт -1 (а иногда 0), и показать это нулём было бы
 * враньём — канал не «нулевой», он просто ещё не проверялся.
 *
 * Ниже мегабита отдаём на откуп fmtBitrate, а мегабиты округляем сами: на
 * 87 Мбит/с десятые доли — шум замера, в колонке от них только рябь.
 */
export function fmtSpeedKbps(kbps?: number | null): string {
  if (typeof kbps !== "number" || !Number.isFinite(kbps) || kbps <= 0) return "";
  if (kbps < 1000) return fmtBitrate((kbps * 1000) / 8);
  const mbps = kbps / 1000;
  return `${mbps >= 10 ? Math.round(mbps) : mbps.toFixed(1)} Мбит/с`;
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
 * Флаг страны из ISO-кода: `NL` → 🇳🇱. Две буквы переводятся в regional indicator
 * symbols, поэтому ни картинок, ни справочника флагов везти не надо — рисует
 * системный шрифт. Пустая строка на всё, что не похоже на код страны.
 */
export function flagOf(cc?: string): string {
  const s = String(cc ?? "")
    .trim()
    .toUpperCase();
  if (!/^[A-Z]{2}$/.test(s)) return "";
  return String.fromCodePoint(
    0x1f1e6 + (s.charCodeAt(0) - 65),
    0x1f1e6 + (s.charCodeAt(1) - 65),
  );
}

/**
 * Обратная операция: флаг-эмодзи из имени профиля → ISO-код. Подписки почти
 * всегда ставят флаг первым символом («🇬🇷 Афины, Греция, Extra»), и это — та
 * страна, которую человек покупал.
 *
 * Проверено на 103 живых профилях: страна ВЫХОДА (флаг в имени) совпала со
 * страной эндпоинта лишь у 5, разошлась у 78 — провайдер держит несколько
 * входных узлов в DE/US/NL и разводит трафик по нужной стране уже внутри своей
 * сети. Поэтому для фильтра «по стране» верен именно флаг из имени, а не
 * geoip по адресу узла.
 */
export function ccFromName(name?: string): string {
  const m = String(name ?? "").match(
    /([\u{1F1E6}-\u{1F1FF}])([\u{1F1E6}-\u{1F1FF}])/u,
  );
  if (!m) return "";
  return (
    String.fromCharCode(m[1].codePointAt(0)! - 0x1f1e6 + 65) +
    String.fromCharCode(m[2].codePointAt(0)! - 0x1f1e6 + 65)
  );
}

/* Названия стран по-русски даёт сама платформа (Intl.DisplayNames) — 250 строк
   справочника в бандле не нужны. Инстанс дорогой, поэтому создаём его один раз и
   лениво; там, где Intl.DisplayNames нет (старый WebView), откатываемся на код. */
let regionNames: Intl.DisplayNames | null | undefined;

export function countryName(cc?: string): string {
  const s = String(cc ?? "")
    .trim()
    .toUpperCase();
  if (!/^[A-Z]{2}$/.test(s)) return "";
  if (regionNames === undefined) {
    try {
      regionNames = new Intl.DisplayNames(["ru"], { type: "region" });
    } catch {
      regionNames = null;
    }
  }
  try {
    return regionNames?.of(s) ?? s;
  } catch {
    return s;
  }
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
