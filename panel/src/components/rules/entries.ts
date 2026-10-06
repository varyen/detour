/* Счёт записей в списках правил. Списки на роутере — обычные текстовые файлы,
   где шапка из комментариев занимает больше строк, чем сами правила: считать
   строки целиком значит показывать человеку неправду. Комментарием считается
   и `//` (наш формат), и `#` (так пишут в списках UDP и hosts). */

export function countEntries(text: string): number {
  let n = 0;
  for (const raw of String(text ?? "").split("\n")) {
    const line = raw.replace(/\/\/.*$/, "").replace(/#.*$/, "").trim();
    if (line) n += 1;
  }
  return n;
}

/** Русские формы: 1 запись, 2 записи, 5 записей. */
export function plural(n: number, forms: [string, string, string]): string {
  const abs = Math.abs(n) % 100;
  const tail = abs % 10;
  if (abs > 10 && abs < 20) return forms[2];
  if (tail > 1 && tail < 5) return forms[1];
  if (tail === 1) return forms[0];
  return forms[2];
}

export function entriesLabel(n: number): string {
  return `${n} ${plural(n, ["запись", "записи", "записей"])}`;
}

export function domainsLabel(n: number): string {
  return `${n} ${plural(n, ["домен", "домена", "доменов"])}`;
}

/* Строки, которые роутер молча пропустит. Бэкенд (sing-box.initd,
   route_map_section) берёт только IPv4/CIDR и домены — регэкспы ниже
   повторяют его буква в букву. IPv6 вынесен отдельно: перехват на роутере
   только IPv4, а IPv6 из LAN, пока VPN включён, отбивается целиком. */
const BACKEND_IPV4 = /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+(\/[0-9]+)?$/;
const BACKEND_DOMAIN = /^[a-zA-Z0-9]([a-zA-Z0-9._-]*\.)+[a-zA-Z]{2,}$/;
const IPV6 = /^\[?[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(%\w+)?\]?(\/[0-9]{1,3})?$/;

export function skippedEntries(text: string): { ipv6: string[]; other: string[] } {
  const ipv6: string[] = [];
  const other: string[] = [];
  for (const raw of String(text ?? "").split("\n")) {
    const line = raw.replace(/\/\/.*$/, "").replace(/#.*$/, "").trim().replace(/^\*\./, "");
    if (!line || BACKEND_IPV4.test(line) || BACKEND_DOMAIN.test(line)) continue;
    if (IPV6.test(line)) ipv6.push(line);
    /* Строка без точки и слэша — заголовок раздела, его пропуск задуман. */
    else if (/[./:]/.test(line)) other.push(line);
  }
  return { ipv6, other };
}
