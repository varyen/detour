<script setup lang="ts">
/* Боковая шторка на широком экране и нижний лист на телефоне — один
   компонент, потому что содержимое и поведение совпадают, различается только
   геометрия. Модалок в панели больше нет: всё, что раньше открывалось поверх
   с затемнением на весь экран, теперь выезжает сбоку/снизу. */
import { onBeforeUnmount, watch } from "vue";
import SectionIcon from "@/components/SectionIcon.vue";

const props = defineProps<{ open: boolean; title: string; wide?: boolean }>();
const emit = defineEmits<{ close: [] }>();

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
}

watch(
  () => props.open,
  (open) => {
    /* Пока шторка открыта, фон не должен уезжать под пальцем. */
    document.body.style.overflow = open ? "hidden" : "";
    if (open) addEventListener("keydown", onKey);
    else removeEventListener("keydown", onKey);
  },
);

onBeforeUnmount(() => {
  document.body.style.overflow = "";
  removeEventListener("keydown", onKey);
});
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="open" class="scrim" @click="emit('close')"></div>
    </Transition>
    <Transition name="sheet">
      <aside
        v-if="open"
        class="sheet"
        :class="{ wide }"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
      >
        <header>
          <span class="grip" aria-hidden="true"></span>
          <h2>{{ title }}</h2>
          <button class="x" type="button" aria-label="Закрыть" @click="emit('close')">
            <SectionIcon name="close" :size="18" />
          </button>
        </header>
        <div v-if="$slots.sticky" class="sticky"><slot name="sticky" /></div>
        <div class="body"><slot /></div>
        <footer v-if="$slots.footer"><slot name="footer" /></footer>
      </aside>
    </Transition>
  </Teleport>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  z-index: 50;
  background: color-mix(in srgb, var(--ground) 62%, transparent);
  backdrop-filter: blur(3px);
}
.sheet {
  position: fixed;
  z-index: 51;
  top: 0;
  right: 0;
  bottom: 0;
  width: min(440px, 100%);
  display: flex;
  flex-direction: column;
  background: color-mix(in srgb, var(--ground) 93%, transparent);
  border-left: 1px solid var(--line);
  box-shadow: var(--shadow);
}
.sheet.wide {
  width: min(760px, 100%);
}
header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px 16px 12px;
  border-bottom: 1px solid var(--line);
}
header h2 {
  font-size: 16px;
  font-weight: 600;
  min-width: 0;
}
.grip {
  display: none;
}
.x {
  margin-left: auto;
  border: 0;
  background: transparent;
  color: var(--dim);
  border-radius: 8px;
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
}
.x:hover {
  background: var(--panel-2);
  color: var(--ink);
}
.sticky {
  padding: 12px 16px;
  border-bottom: 1px solid var(--line);
}
.body {
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: 12px 16px 20px;
  flex: 1;
}
footer {
  border-top: 1px solid var(--line);
  padding: 12px 16px calc(12px + env(safe-area-inset-bottom));
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

@media (max-width: 860px) {
  /* На телефоне — лист снизу: до верхнего края экрана тянуться неудобно. */
  .sheet,
  .sheet.wide {
    top: auto;
    left: 0;
    right: 0;
    width: 100%;
    max-height: 88dvh;
    border-left: 0;
    border-top: 1px solid var(--line);
    border-radius: 18px 18px 0 0;
    padding-bottom: env(safe-area-inset-bottom);
  }
  header {
    padding-top: 10px;
    position: relative;
  }
  .grip {
    display: block;
    position: absolute;
    top: 6px;
    left: 50%;
    transform: translateX(-50%);
    width: 36px;
    height: 4px;
    border-radius: 2px;
    background: var(--line-2);
  }
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
.sheet-enter-active,
.sheet-leave-active {
  transition: transform 0.26s cubic-bezier(0.3, 0.8, 0.3, 1);
}
.sheet-enter-from,
.sheet-leave-to {
  transform: translateX(100%);
}
@media (max-width: 860px) {
  .sheet-enter-from,
  .sheet-leave-to {
    transform: translateY(100%);
  }
}
</style>
