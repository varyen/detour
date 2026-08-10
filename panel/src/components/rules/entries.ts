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
