<script setup lang="ts">
defineProps<{
  variant?: "primary" | "ghost" | "danger";
  busy?: boolean;
  disabled?: boolean;
  type?: "button" | "submit";
}>();
</script>

<template>
  <button
    :type="type ?? 'button'"
    class="btn"
    :class="variant ?? 'ghost'"
    :disabled="disabled || busy"
  >
    <span v-if="busy" class="spin" aria-hidden="true"></span>
    <slot />
  </button>
</template>

<style scoped>
.btn {
  border: 1px solid var(--line-2);
  background: transparent;
  border-radius: var(--radius-sm);
  padding: 8px 14px;
  font-size: 13.5px;
  color: var(--ink);
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 38px;
  transition: background 0.16s, border-color 0.16s, opacity 0.16s;
}
.btn:hover:not(:disabled) {
  background: var(--accent-wash);
  border-color: var(--accent);
}
.btn.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-on);
  font-weight: 600;
}
.btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
  background: var(--accent);
}
.btn.danger {
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 45%, transparent);
}
.btn.danger:hover:not(:disabled) {
  background: color-mix(in srgb, var(--bad) 12%, transparent);
  border-color: var(--bad);
}
.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.spin {
  width: 13px;
  height: 13px;
  border-radius: 50%;
  border: 2px solid currentColor;
  border-top-color: transparent;
  animation: spin 0.7s linear infinite;
  flex: none;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
