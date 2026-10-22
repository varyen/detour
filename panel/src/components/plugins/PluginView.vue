<script setup lang="ts">
/* Содержимое, которое плагин описывает данными, а не вёрсткой: короткая фраза,
   метрики «подпись — значение» и кнопки-действия. Так рисуются и карточка на
   «Обзоре», и шапка секции настроек — плагину не нужно знать стили панели. */
import { computed, ref } from "vue";
import UiButton from "@/components/UiButton.vue";
import { plugins } from "@/api";
import type { PluginAction, PluginViewData } from "@/api";
import { useToastStore } from "@/stores/toast";

const props = defineProps<{
  pluginId: string;
  data: PluginViewData | null;
  /** Действия из манифеста — идут перед теми, что прислал сам ответ. */
  actions?: PluginAction[];
}>();

const emit = defineEmits<{ done: [] }>();

const toast = useToastStore();
const busy = ref("");

const allActions = computed<PluginAction[]>(() => {
  const seen = new Set<string>();
  return [...(props.actions ?? []), ...(Array.isArray(props.data?.actions) ? props.data!.actions! : [])].filter(
    (a) => a && a.op && a.label && !seen.has(a.op) && !!seen.add(a.op),
  );
});

const metrics = computed(() => (Array.isArray(props.data?.metrics) ? props.data!.metrics! : []));

async function run(a: PluginAction) {
  if (a.confirm && !window.confirm(a.confirm)) return;
  busy.value = a.op;
  try {
    const r = await plugins.call(props.pluginId, a.op, {});
    toast.ok(r?.message || `${a.label}: готово`);
    emit("done");
  } catch (e) {
    toast.fromError(e, `${a.label}: не получилось`);
  } finally {
    busy.value = "";
  }
}
</script>

<template>
  <div class="pv">
    <p v-if="data?.lead" class="lead">{{ data.lead }}</p>
    <dl v-if="metrics.length" class="metrics">
      <div v-for="(m, i) in metrics" :key="i" :title="m.hint">
        <dt>{{ m.label }}</dt>
        <dd :class="m.tone">{{ m.value }}</dd>
      </div>
    </dl>
    <div v-if="allActions.length" class="acts">
      <UiButton
        v-for="a in allActions"
        :key="a.op"
        :variant="a.variant"
        :busy="busy === a.op"
        :disabled="!!busy && busy !== a.op"
        @click="run(a)"
      >
        {{ a.label }}
      </UiButton>
    </div>
  </div>
</template>

<style scoped>
.pv {
  display: flex;
  flex-direction: column;
  gap: 9px;
  min-width: 0;
}
.lead {
  margin: 0;
  font-size: 13.5px;
  color: var(--ink);
  overflow-wrap: anywhere;
}
.metrics {
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
  gap: 8px 14px;
}
.metrics div {
  min-width: 0;
}
.metrics dt {
  font-size: 11.5px;
  color: var(--faint);
}
.metrics dd {
  margin: 2px 0 0;
  font-family: var(--mono);
  font-size: 14px;
  color: var(--ink);
  overflow-wrap: anywhere;
}
.metrics dd.ok {
  color: var(--ok);
}
.metrics dd.warn {
  color: var(--warn);
}
.metrics dd.bad {
  color: var(--bad);
}
.acts {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
</style>
