/* Отдать файл человеку. В браузере и на десктопном клиенте — обычная загрузка
   blob-ссылки; Android WebView такую молча игнорирует, поэтому в клиенте файл
   сохраняет приложение (команда `save_file`). Ответ — куда лёг файл, если это
   известно: «Download/Detour/…» или "share" (отдан в системное «Поделиться»). */

const inTauri = __CLIENT__ && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function saveFile(name: string, content: string, mime = "application/json"): Promise<string | null> {
  if (inTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      return await invoke<string>("save_file", { name, content, mime });
    } catch (e) {
      if (String(e) !== "unsupported") throw new Error(String(e));
    }
  }
  const url = URL.createObjectURL(new Blob([content], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
  return null;
}

/** Текст для тоста: где искать сохранённый файл. */
export function savedWhere(where: string | null, fallback: string): string {
  if (!where) return fallback;
  if (where === "share") return "Файл передан — выберите, куда сохранить";
  return `Сохранено: ${where}`;
}
