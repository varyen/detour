<script setup lang="ts">
import { computed, ref, watch } from "vue";
import DrawerSheet from "@/components/DrawerSheet.vue";
import SectionIcon from "@/components/SectionIcon.vue";
import { useProfilesStore } from "@/stores/profiles";
import { useStatusStore } from "@/stores/status";
import { useToastStore } from "@/stores/toast";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const store = useProfilesStore();
const status = useStatusStore();
const toast = useToastStore();
const query = ref("");

const STATE_TEXT: Record<string, string> = {
  ok: "проверка проходит",
  slow: "отвечает медленно",
  dead: "не отвечает",
  unknown: "не проверялся",
};

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    query.value = "";
    await store.load();
    void store.loadProbes();
  },
);

const groups = computed(() => {
  const q = query.value.trim().toLowerCase();
  const out: { name: string; rows: typeof store.rows }[] = [];
  for (const [name, rows] of store.byGroup) {
    const hit = q
      ? rows.filter((r) => `${r.name} ${r.type} ${r.group ?? ""}`.toLowerCase().includes(q))
      : rows;
    if (hit.length) out.push({ name, rows: hit });
  }
  return out;
});

async function pick(id: string, label: string) {
  try {
    await store.activate(id);
    toast.ok(`Подключено: ${label}`);
    emit("close");
    void status.refresh(true);
  } catch (e) {
    toast.fromError(e, "Не удалось переключить профиль");
  }
}
</script>

<template>
  <DrawerSheet :open="open" title="Сменить VPN" @close="emit('close')">
    <template #sticky>
      <label class="search">
        <SectionIcon name="search" :size="14" />
        <input
          v-model="query"
          type="search"
          placeholder="Страна, протокол, группа…"
          autocomplete="off"
        />
      </label>
    </template>

    <p v-if="store.loading && !store.rows.length" class="note">Загружаю профили…</p>
    <p v-else-if="!groups.length" class="note">Ничего не нашлось</p>

    <template v-for="g in groups" :key="g.name">
      <p class="grp eyebrow">{{ g.name }}</p>
      <button
        v-for="r in g.rows"
        :key="r.id"
        class="item"
        type="button"
        :aria-current="r.isActive"
        :disabled="!!store.switching"
        @click="pick(r.id, r.name)"
      >
        <i class="hdot" :class="`h-${r.state}`" :title="STATE_TEXT[r.state]"></i>
        <span class="nm">
          {{ r.name }}
          <small>{{ r.type }}<template v-if="r.state !== 'unknown'"> · {{ STATE_TEXT[r.state] }}</template></small>
        </span>
        <span v-if="store.switching === r.id" class="ms">включаю…</span>
        <span v-else-if="r.ping?.rtt" class="ms num">{{ Math.round(r.ping.rtt) }} мс</span>
      </button>
    </template>
  </DrawerSheet>
</template>

<style scoped>
.search {
  display: flex;
  align-items: center;
  gap: 9px;
  border: 1px solid var(--line-2);
  border-radius: var(--radius-sm);
  padding: 8px 12px;
  background: var(--panel-2);
  color: var(--faint);
}
.search input {
  border: 0;
  background: transparent;
  outline: none;
  width: 100%;
  /* 16px — иначе iOS зумит страницу при фокусе. */
  font-size: 16px;
  color: var(--ink);
}
.note {
  color: var(--dim);
  font-size: 14px;
  padding: 8px 2px;
}
.grp {
  padding: 14px 6px 6px;
}
.item {
  width: 100%;
  text-align: left;
  border: 1px solid transparent;
  background: transparent;
  border-radius: var(--radius-sm);
  padding: 9px 10px;
  display: flex;
  align-items: center;
  gap: 11px;
  min-height: 46px;
}
.item:hover:not(:disabled) {
  background: var(--panel-2);
  border-color: var(--line);
}
.item[aria-current="true"] {
  background: var(--accent-wash);
  border-color: var(--accent);
}
.item:disabled {
  opacity: 0.6;
}
.nm {
  min-width: 0;
  font-size: 14.5px;
  overflow: hidden;
}
.nm small {
  display: block;
  font-size: 12px;
  color: var(--faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ms {
  margin-left: auto;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--dim);
  white-space: nowrap;
}
</style>
