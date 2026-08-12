import type { InjectionKey, Ref } from "vue";

/** Состояние режима правки главной: делится между «Обзором» и слотами карточек. */
export interface EditContext {
  editing: Ref<boolean>;
  dragId: Ref<string | null>;
  dropIndex: Ref<number>;
  dx: Ref<number>;
  dy: Ref<number>;
  grab: (e: PointerEvent, id: string) => void;
}

export const EDIT_KEY: InjectionKey<EditContext> = Symbol("dash-edit");
