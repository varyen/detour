import { defineStore } from "pinia";
import { computed, ref } from "vue";

export interface Command {
  id: string;
  title: string;
  /** Группа для правой подписи: «правила», «система», … */
  group: string;
  /** Дополнительные слова для поиска, которых нет в заголовке. */
  keywords?: string;
  run: () => void | Promise<void>;
  /** Скрыть, когда возможность недоступна на этой платформе/сборке. */
  available?: () => boolean;
}

/* Палитра — замена семи вкладкам старой панели: редкие действия не занимают
   место на экране, но остаются в одном нажатии. Разделы регистрируют свои
   команды при монтировании и снимают при уходе. */
/**
 * Источник команд, которые зависят от введённого текста — например, список VPN:
 * держать в реестре сотню профилей бессмысленно, они нужны только когда человек
 * начал печатать имя.
 */
export type CommandProvider = (query: string) => Command[];

export const useCommandStore = defineStore("commands", () => {
  const registry = ref<Command[]>([]);
  const providers = ref<CommandProvider[]>([]);
  const open = ref(false);

  function unregister(ids: string[]) {
    const drop = new Set(ids);
    registry.value = registry.value.filter((c) => !drop.has(c.id));
  }

  function register(cmds: Command[]) {
    const ids = cmds.map((c) => c.id);
    const dup = new Set(ids);
    registry.value = [...registry.value.filter((c) => !dup.has(c.id)), ...cmds];
    return () => unregister(ids);
  }

  const visible = computed(() =>
    registry.value.filter((c) => !c.available || c.available()),
  );

  function addProvider(fn: CommandProvider) {
    providers.value = [...providers.value, fn];
    return () => {
      providers.value = providers.value.filter((p) => p !== fn);
    };
  }

  function search(q: string): Command[] {
    const query = q.trim().toLowerCase();
    if (!query) return visible.value;
    const hits = visible.value.filter((c) =>
      `${c.title} ${c.group} ${c.keywords ?? ""}`.toLowerCase().includes(query),
    );
    const dynamic = providers.value.flatMap((p) => p(query));
    /* Профиль, который человек ищет по имени, важнее общей команды. */
    return [...dynamic, ...hits];
  }

  return { registry, open, visible, register, unregister, addProvider, search };
});
