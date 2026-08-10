import { defineStore } from "pinia";
import { ref } from "vue";

export type ToastKind = "info" | "ok" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

let seq = 0;

export const useToastStore = defineStore("toast", () => {
  const items = ref<Toast[]>([]);

  function push(text: string, kind: ToastKind = "info", ms = 3200) {
    const id = ++seq;
    items.value.push({ id, kind, text });
    setTimeout(() => dismiss(id), ms);
    return id;
  }

  function dismiss(id: number) {
    items.value = items.value.filter((t) => t.id !== id);
  }

  const ok = (t: string) => push(t, "ok");
  const info = (t: string) => push(t, "info");
  /** Ошибки живут дольше: их читают, а не замечают краем глаза. */
  const error = (t: string) => push(t, "error", 6000);

  /** Единая точка превращения исключения в понятное сообщение. */
  function fromError(e: unknown, fallback = "Не получилось") {
    const msg = e instanceof Error && e.message ? e.message : fallback;
    return error(msg);
  }

  return { items, push, dismiss, ok, info, error, fromError };
});
