<script setup lang="ts">
/* Одна большая кнопка «подключиться/отключиться» — главное действие приложения.
   Всё остальное на «Обзоре» — подробности; этой кнопке достаточно выбранного
   профиля. Нет профиля — ведёт туда, где его заводят: в форму со строкой для
   ссылки, если профилей нет вовсе, и в список, если выбрать есть из чего. */
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { diag } from "@/api";
import { useStatusStore } from "@/stores/status";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";

const status = useStatusStore();
const profiles = useProfilesStore();
const toast = useToastStore();
const router = useRouter();
const busy = ref(false);

const running = computed(() => status.singboxRunning);
const chain = computed(() => status.activeChain);
const hasActive = computed(() => chain.value.length > 0 || !!status.activeProfile);
const activeName = computed(() => {
  const ids = chain.value.length ? chain.value : [status.activeProfile];
  const names = ids.map((id) => profiles.rows.find((r) => r.id === id)?.name || id);
  return names.join(" → ");
});

const state = computed<"on" | "off" | "pick" | "none">(() => {
  if (running.value) return "on";
  if (hasActive.value) return "off";
  return profiles.rows.length ? "pick" : "none";
});

const label = computed(
  () =>
    ({
      on: "Отключиться",
      off: "Подключиться",
      pick: "Выбрать профиль",
      none: "Добавить профиль",
    })[state.value],
);

const caption = computed(
  () =>
    ({
      on: `Подключено · ${activeName.value}`,
      off: `Отключено · ${activeName.value}`,
      pick: "Профиль не выбран — выберите, через какой VPN подключаться",
      none: "Профилей ещё нет — добавьте ссылку от вашего VPN-сервиса",
    })[state.value],
);

async function press() {
  if (busy.value) return;
  if (state.value === "none") {
    void router.push({ path: "/profiles", query: { new: "1" } });
    return;
  }
  if (state.value === "pick") {
    void router.push({ path: "/profiles", query: { sort: "speed" } });
    return;
  }
  busy.value = true;
  try {
    if (state.value === "on") {
      await diag.singboxStop();
      toast.ok("Отключено — трафик идёт напрямую");
    } else {
      await diag.singboxStart();
      toast.ok(`Подключено: ${activeName.value}`);
    }
  } catch (e) {
    toast.fromError(e, state.value === "on" ? "Не удалось отключиться" : "Не удалось подключиться");
  } finally {
    busy.value = false;
    void status.refresh(true);
  }
}
</script>

<template>
  <section class="power" :class="state">
    <div class="info">
      <p class="state">
        <i class="dot" aria-hidden="true"></i>
        {{ state === "on" ? "VPN включён" : "VPN выключен" }}
      </p>
      <p class="cap">{{ caption }}</p>
    </div>
    <button
      class="go"
      type="button"
      :class="{ busy }"
      :disabled="busy"
      :aria-pressed="state === 'on'"
      @click="press"
    >
      <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
        <path d="M12 3v9" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" />
        <path
          d="M6.3 7.2a8 8 0 1 0 11.4 0"
          fill="none"
          stroke="currentColor"
          stroke-width="2.4"
          stroke-linecap="round"
        />
      </svg>
      {{ busy ? (state === "on" ? "Отключаю…" : "Подключаю…") : label }}
    </button>
  </section>
</template>

<style scoped>
.power {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  backdrop-filter: blur(10px);
  padding: 16px;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 14px 20px;
  min-width: 0;
  height: 100%;
}
.power.on {
  border-color: color-mix(in srgb, var(--ok) 45%, var(--line));
}
.info {
  flex: 1 1 220px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.state {
  display: flex;
  align-items: center;
  gap: 9px;
  margin: 0;
  font-size: 19px;
  font-weight: 650;
  color: var(--ink);
}
.dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--faint);
  flex: none;
}
.on .dot {
  background: var(--ok);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--ok) 22%, transparent);
}
.cap {
  margin: 0;
  font-size: 13.5px;
  color: var(--dim);
  overflow-wrap: anywhere;
}
.go {
  flex: 1 1 260px;
  max-width: 100%;
  min-height: 60px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border: 1px solid var(--accent);
  border-radius: 999px;
  background: var(--accent);
  color: var(--accent-on);
  font-size: 18px;
  font-weight: 650;
  padding: 0 28px;
  cursor: pointer;
  transition:
    filter 0.15s,
    background 0.15s;
}
.go:hover:not(:disabled) {
  filter: brightness(1.08);
}
.on .go {
  background: transparent;
  color: var(--bad);
  border-color: color-mix(in srgb, var(--bad) 60%, var(--line-2));
}
.on .go:hover:not(:disabled) {
  background: color-mix(in srgb, var(--bad) 10%, transparent);
  filter: none;
}
.go:disabled {
  cursor: default;
}
.go.busy {
  animation: pulse 1.1s ease-in-out infinite;
}
@keyframes pulse {
  50% {
    opacity: 0.6;
  }
}
@media (prefers-reduced-motion: reduce) {
  .go.busy {
    animation: none;
  }
}
</style>
