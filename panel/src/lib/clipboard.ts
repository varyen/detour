/* Копирование в буфер. Панель чаще всего открыта по http://192.168.8.1 — это
   не secure context, и navigator.clipboard там просто отсутствует (в Safari он
   ещё и требует, чтобы вызов был прямо в обработчике клика). Поэтому старый
   execCommand остаётся не «легаси», а рабочим путём.

   Возвращаем false, а не бросаем: вызывающий показывает текст, чтобы человек
   выделил его руками — это последний способ, который работает всегда. */
export async function copyText(text: string): Promise<boolean> {
  if (!text) return false;
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    /* Отказ в разрешении или небезопасный контекст — пробуем ниже. */
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    /* Вне экрана, но не display:none — из скрытого нельзя выделить. */
    ta.style.position = "fixed";
    ta.style.top = "0";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}
