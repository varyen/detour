<script setup lang="ts">
const props = defineProps<{
  modelValue: boolean;
  label: string;
  hint?: string;
  disabled?: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{ "update:modelValue": [boolean] }>();

function flip() {
  if (props.disabled || props.busy) return;
  emit("update:modelValue", !props.modelValue);
}
</script>

<template>
  <div class="row" :class="{ off: disabled }">
    <button
      class="sw"
      type="button"
      role="switch"
      :aria-checked="modelValue"
      :aria-label="label"
      :disabled="disabled || busy"
      :data-busy="busy ? 1 : 0"
      @click="flip"
    ></button>
    <button class="text" type="button" :disabled="disabled || busy" @click="flip">
      <span class="lbl">{{ label }}</span>
      <small v-if="hint">{{ hint }}</small>
    </button>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  align-items: center;
  gap: 11px;
  font-size: 14px;
}
.row.off {
  opacity: 0.55;
}
.sw {
  position: relative;
  width: 38px;
  height: 22px;
  border-radius: 999px;
  border: 1px solid var(--line-2);
  background: var(--panel-2);
  padding: 0;
  flex: none;
  transition: background 0.2s, border-color 0.2s;
}
.sw::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--dim);
  transition: transform 0.2s, background 0.2s;
}
.sw[aria-checked="true"] {
  background: var(--accent);
  border-color: var(--accent);
}
.sw[aria-checked="true"]::after {
  transform: translateX(16px);
  background: var(--accent-on);
}
.sw[data-busy="1"]::after {
  animation: pulse 1s ease-in-out infinite;
}
@keyframes pulse {
  50% {
    opacity: 0.35;
  }
}
.text {
  border: 0;
  background: transparent;
  padding: 0;
  text-align: left;
  min-width: 0;
}
.lbl {
  display: block;
}
.text small {
  display: block;
  font-size: 12px;
  color: var(--faint);
}
</style>
