<script setup lang="ts">
/* Одна линия обновления: панель, sing-box, tpws, nfqws2 — у всех одинаковый
   набор полей ({current,available,upstream}) и одинаковые действия, поэтому
   строка общая. */
import { computed } from "vue";
import UiButton from "@/components/UiButton.vue";
import type { UpdateChannelState } from "@/api";
import { fmtAgo } from "@/lib/format";

const props = defineProps<{
  title: string;
  state: UpdateChannelState | null;
  /** Что установлено сейчас, если состояние канала об этом молчит. */
  installed?: string;
  busyCheck?: boolean;
  busyApply?: boolean;
  applyLabel?: string;
  note?: string;
}>();

const emit = defineEmits<{ check: []; apply: []; changelog: [] }>();

/* Имена полей у state-файлов обновлятора и у сводки разные (current/available
   против current_version/available_version) — читаем оба, иначе после ручной
   проверки строка теряет версии и падает на fallback `installed`. */
const current = computed(
  () => props.state?.current_version || props.state?.current || props.installed || "",
);
const available = computed(
  () => props.state?.available_version || props.state?.available || "",
);
const has = computed(() => props.state?.update_available === true);

/* last_check — строка ISO-8601 («2026-08-10T15:00:14Z»), а fmtAgo ждёт unix-
   секунды. Раньше здесь читалось поле `checked`, которого в ответах CGI нет
   вообще, поэтому «проверяли N назад» не показывалось никогда. */
const checked = computed(() => {
  const raw = String(props.state?.last_check ?? "").trim();
  if (raw) {
    const ms = Date.parse(raw);
    /* Неразбираемую дату молча пропускаем: соврать «только что» хуже, чем
       не показать строку совсем. */
    if (Number.isFinite(ms)) return fmtAgo(ms / 1000);
  }
  return fmtAgo(props.state?.checked);
});
const hasChangelog = computed(() => !!props.state?.changelog_b64);
</script>

<template>
  <div class="row">
    <div class="info">
      <p class="nm">{{ title }}</p>
      <p class="ver num">
        <span>сейчас {{ current || "неизвестно" }}</span>
        <span v-if="available && available !== current">в источнике {{ available }}</span>
        <span v-if="checked">проверяли {{ checked }}</span>
      </p>
      <p v-if="has" class="chip ok">Есть обновление</p>
      <p v-else-if="state" class="chip">Установлена свежая версия</p>
      <p v-if="state?.upstream_newer" class="chip warn">
        У разработчика вышла {{ state?.upstream }} — в источнике её пока нет
      </p>
      <p v-if="state?.error" class="chip bad">{{ state.error }}</p>
      <p v-if="note" class="note">{{ note }}</p>
    </div>

    <div class="acts">
      <UiButton :busy="busyCheck" @click="emit('check')">Проверить</UiButton>
      <UiButton
        variant="primary"
        :disabled="!has"
        :busy="busyApply"
        @click="emit('apply')"
      >
        {{ applyLabel ?? "Установить" }}
      </UiButton>
      <UiButton v-if="hasChangelog" @click="emit('changelog')">Что нового</UiButton>
    </div>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  align-items: flex-start;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--panel-2);
  padding: 11px 12px;
  min-width: 0;
}
.info {
  flex: 1 1 240px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.nm {
  font-size: 15px;
  font-weight: 600;
}
.ver {
  font-size: 12.5px;
  color: var(--dim);
  display: flex;
  flex-wrap: wrap;
  gap: 2px 12px;
}
.chip {
  font-size: 12.5px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.chip.ok {
  color: var(--ok);
  font-weight: 600;
}
.chip.warn {
  color: var(--warn);
}
.chip.bad {
  color: var(--bad);
}
.note {
  font-size: 12px;
  color: var(--faint);
}
.acts {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;
}
@media (max-width: 860px) {
  .acts .btn {
    min-height: 44px;
  }
}
</style>
