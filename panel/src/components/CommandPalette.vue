<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useCommandStore } from "@/stores/commands";

const store = useCommandStore();
const query = ref("");
const sel = ref(0);
const input = ref<HTMLInputElement | null>(null);
const listEl = ref<HTMLElement | null>(null);

const results = computed(() => store.search(query.value));

watch(
  () => store.open,
  async (open) => {
    if (!open) return;
    query.value = "";
    sel.value = 0;
    await nextTick();
    input.value?.focus();
  },
);

watch(results, () => {
  sel.value = 0;
});

async function scrollToSel() {
  await nextTick();
  listEl.value
    ?.querySelector<HTMLElement>('[data-sel="1"]')
    ?.scrollIntoView({ block: "nearest" });
}

function move(delta: number) {
  if (!results.value.length) return;
  sel.value = Math.min(Math.max(sel.value + delta, 0), results.value.length - 1);
  void scrollToSel();
}

async function run(i: number) {
  const cmd = results.value[i];
  if (!cmd) return;
  store.open = false;
  await cmd.run();
}

function onKey(e: KeyboardEvent) {
  if (e.key === "ArrowDown") {
    e.preventDefault();
    move(1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    move(-1);
  } else if (e.key === "Enter") {
    e.preventDefault();
    void run(sel.value);
  } else if (e.key === "Escape") {
    store.open = false;
  }
}
</script>

<template>
  <Transition name="pal">
    <div
      v-if="store.open"
      class="scrim"
      role="dialog"
      aria-modal="true"
      aria-label="Поиск действия"
      @click.self="store.open = false"
    >
      <div class="box">
        <input
          ref="input"
          v-model="query"
          class="q"
          type="text"
          placeholder="Что сделать?"
          autocomplete="off"
          spellcheck="false"
          @keydown="onKey"
        />
        <div ref="listEl" class="list">
          <button
            v-for="(c, i) in results"
            :key="c.id"
            class="cmd"
            type="button"
            :data-sel="i === sel ? 1 : 0"
            @click="run(i)"
            @mousemove="sel = i"
          >
            <span class="t">{{ c.title }}</span>
            <small>{{ c.group }}</small>
          </button>
          <p v-if="!results.length" class="empty">Ничего не нашлось</p>
        </div>
        <div class="foot">
          <span><kbd>↑</kbd><kbd>↓</kbd> выбрать</span>
          <span><kbd>⏎</kbd> выполнить</span>
          <span><kbd>esc</kbd> закрыть</span>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  justify-content: center;
  padding: 12vh 14px 0;
  background: color-mix(in srgb, var(--ground) 58%, transparent);
  backdrop-filter: blur(4px);
}
@media (max-width: 640px) {
  .scrim {
    padding-top: 6vh;
  }
}
.box {
  width: min(620px, 100%);
  height: max-content;
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--line-2);
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--ground) 95%, transparent);
  box-shadow: var(--shadow);
}
.q {
  border: 0;
  border-bottom: 1px solid var(--line);
  background: transparent;
  outline: none;
  padding: 15px 18px;
  font-size: 16px;
}
.list {
  overflow-y: auto;
  padding: 6px;
  -webkit-overflow-scrolling: touch;
}
.cmd {
  width: 100%;
  text-align: left;
  border: 0;
  background: transparent;
  border-radius: var(--radius-sm);
  padding: 10px 12px;
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 14.5px;
}
.cmd small {
  margin-left: auto;
  font-family: var(--mono);
  font-size: 10.5px;
  color: var(--faint);
  letter-spacing: 0.1em;
  text-transform: uppercase;
  white-space: nowrap;
}
.cmd[data-sel="1"] {
  background: var(--accent-wash);
  color: var(--accent);
}
.t {
  min-width: 0;
}
.empty {
  padding: 14px 12px;
  color: var(--faint);
  font-size: 14px;
}
.foot {
  border-top: 1px solid var(--line);
  padding: 8px 14px;
  font-size: 12px;
  color: var(--faint);
  display: flex;
  gap: 16px;
}
@media (max-width: 640px) {
  .foot {
    display: none;
  }
}
.pal-enter-active,
.pal-leave-active {
  transition: opacity 0.16s;
}
.pal-enter-from,
.pal-leave-to {
  opacity: 0;
}
</style>
