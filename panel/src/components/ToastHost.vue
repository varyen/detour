<script setup lang="ts">
import { useToastStore } from "@/stores/toast";
const toast = useToastStore();
</script>

<template>
  <div class="host" role="status" aria-live="polite">
    <TransitionGroup name="toast">
      <button
        v-for="t in toast.items"
        :key="t.id"
        class="toast"
        :class="t.kind"
        type="button"
        @click="toast.dismiss(t.id)"
      >
        {{ t.text }}
      </button>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.host {
  position: fixed;
  left: 50%;
  transform: translateX(-50%);
  /* На телефоне тосты не должны нырять под нижний таб-бар. */
  bottom: calc(18px + env(safe-area-inset-bottom));
  z-index: 70;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  width: max-content;
  max-width: min(560px, calc(100vw - 24px));
  pointer-events: none;
}
@media (max-width: 860px) {
  .host {
    bottom: calc(var(--tabbar) + 14px + env(safe-area-inset-bottom));
  }
}
.toast {
  pointer-events: auto;
  text-align: left;
  border: 1px solid var(--line-2);
  background: color-mix(in srgb, var(--ground) 94%, transparent);
  backdrop-filter: blur(10px);
  border-radius: var(--radius-sm);
  padding: 10px 16px;
  font-size: 13.5px;
  box-shadow: var(--shadow);
  max-width: 100%;
}
.toast.ok {
  border-color: color-mix(in srgb, var(--ok) 55%, transparent);
}
.toast.error {
  border-color: color-mix(in srgb, var(--bad) 60%, transparent);
  color: var(--bad);
}
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(10px);
}
</style>
