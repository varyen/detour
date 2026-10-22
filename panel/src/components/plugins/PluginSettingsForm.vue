<script setup lang="ts">
/* Форма настроек по схеме из манифеста плагина. Значения проверяет и хранит
   роутер (detour-plugin settings-set): здесь только удобный ввод. Пароли с
   роутера не приходят — пустое поле значит «оставить как есть». */
import { computed, reactive, ref, watch } from "vue";
import FormField from "@/components/services/FormField.vue";
import SwitchToggle from "@/components/SwitchToggle.vue";
import UiButton from "@/components/UiButton.vue";
import { plugins } from "@/api";
import type { InstalledPlugin, PluginField } from "@/api";
import { useToastStore } from "@/stores/toast";

const props = defineProps<{ plugin: InstalledPlugin; fields: PluginField[] }>();
const emit = defineEmits<{ saved: [] }>();

const toast = useToastStore();
const busy = ref(false);
const form = reactive<Record<string, unknown>>({});

function reset() {
  for (const f of props.fields) {
    const v = props.plugin.settings[f.key] ?? f.default;
    form[f.key] = f.type === "bool" ? v === true || v === "true" : f.type === "password" ? "" : (v ?? "");
  }
}
watch(() => [props.plugin, props.fields], reset, { immediate: true });

const dirty = computed(() =>
  props.fields.some((f) => {
    if (f.type === "password") return form[f.key] !== "";
    const cur = props.plugin.settings[f.key] ?? f.default;
    if (f.type === "bool") return form[f.key] !== (cur === true || cur === "true");
    return String(form[f.key] ?? "") !== String(cur ?? "");
  }),
);

function options(f: PluginField) {
  return (f.options ?? []).map((o) =>
    typeof o === "object" ? { value: String(o.value), label: o.label ?? String(o.value) } : { value: o, label: o },
  );
}

function passwordHint(f: PluginField) {
  const set = props.plugin.secrets[f.key];
  const base = set ? "задан — оставьте пустым, чтобы не менять" : "не задан";
  return f.hint ? `${f.hint} · ${base}` : base;
}

async function save() {
  busy.value = true;
  try {
    const body: Record<string, unknown> = {};
    for (const f of props.fields) {
      const v = form[f.key];
      if (f.type === "number") body[f.key] = v === "" ? undefined : Number(v);
      else body[f.key] = v;
    }
    const r = await plugins.saveSettings(props.plugin.id, body);
    toast.ok(r?.message || "Настройки сохранены");
    emit("saved");
  } catch (e) {
    toast.fromError(e, "Не удалось сохранить настройки");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <form class="psf" @submit.prevent="save">
    <template v-for="f in fields" :key="f.key">
      <SwitchToggle
        v-if="f.type === 'bool'"
        :model-value="form[f.key] === true"
        :label="f.label || f.key"
        :hint="f.hint"
        @update:model-value="form[f.key] = $event"
      />
      <FormField v-else-if="f.type === 'select'" :label="f.label || f.key" :hint="f.hint">
        <select v-model="form[f.key]">
          <option v-for="o in options(f)" :key="o.value" :value="o.value">{{ o.label }}</option>
        </select>
      </FormField>
      <FormField v-else-if="f.type === 'textarea'" :label="f.label || f.key" :hint="f.hint">
        <textarea
          :value="String(form[f.key] ?? '')"
          rows="4"
          @input="form[f.key] = ($event.target as HTMLTextAreaElement).value" :placeholder="f.placeholder" :maxlength="f.maxlength" />
      </FormField>
      <FormField v-else-if="f.type === 'password'" :label="f.label || f.key" :hint="passwordHint(f)">
        <input v-model="form[f.key]" type="password" autocomplete="new-password" :placeholder="f.placeholder" />
      </FormField>
      <FormField v-else :label="f.label || f.key" :hint="f.hint">
        <input
          v-model="form[f.key]"
          :type="f.type === 'number' ? 'number' : 'text'"
          :inputmode="f.type === 'number' ? 'numeric' : undefined"
          :min="f.min"
          :max="f.max"
          :maxlength="f.maxlength"
          :placeholder="f.placeholder"
        />
      </FormField>
    </template>
    <div class="acts">
      <UiButton type="submit" variant="primary" :busy="busy" :disabled="!dirty">Сохранить</UiButton>
      <UiButton v-if="dirty" :disabled="busy" @click="reset">Отменить</UiButton>
    </div>
  </form>
</template>

<style scoped>
.psf {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
.acts {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
</style>
