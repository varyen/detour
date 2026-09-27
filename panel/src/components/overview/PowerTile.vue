<script setup lang="ts">
/* Одна большая кнопка «подключиться/отключиться» — главное действие приложения.
   Всё остальное на «Обзоре» — подробности; этой кнопке достаточно выбранного
   профиля. Нет профиля — ведёт туда, где его заводят: в форму со строкой для
   ссылки, если профилей нет вовсе, и в список, если выбрать есть из чего. */
import { computed, ref, useId } from "vue";
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

/* Орбита из символов фонового дождя — набор случайный, но один на всё время
   жизни плитки, иначе строка мигала бы на каждой перерисовке. */
const GLYPHS = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモ0123456789<>[]{}/*+-=$#@%&";
const orbitText = Array.from({ length: 30 }, () => GLYPHS[(Math.random() * GLYPHS.length) | 0]).join("");
const orbitId = `orbit-${useId()}`;

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
    <div class="knob">
      <button
        class="go"
        type="button"
        :class="{ busy }"
        :disabled="busy"
        :aria-pressed="state === 'on'"
        :aria-label="busy ? (state === 'on' ? 'Отключаю…' : 'Подключаю…') : label"
        @click="press"
      >
        <svg class="orbit" viewBox="0 0 156 156" aria-hidden="true">
          <defs>
            <path :id="orbitId" d="M78 78m-68 0a68 68 0 1 1 136 0a68 68 0 1 1-136 0" />
          </defs>
          <text><textPath :href="`#${orbitId}`">{{ orbitText }}</textPath></text>
        </svg>
        <svg class="ring" viewBox="0 0 128 128" aria-hidden="true">
          <circle class="track" cx="64" cy="64" r="60" />
          <circle class="arc" cx="64" cy="64" r="60" pathLength="360" />
        </svg>
        <svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M12 3v9" />
          <path d="M6.3 7.2a8 8 0 1 0 11.4 0" />
        </svg>
      </button>
      <span class="label">{{ busy ? (state === "on" ? "Отключаю…" : "Подключаю…") : label }}</span>
    </div>
  </section>
</template>

<style scoped>
.power {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  backdrop-filter: blur(10px);
  padding: 16px 20px;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 14px 20px;
  min-width: 0;
  height: 100%;
}
.power.on {
  border-color: color-mix(in srgb, var(--ok) 45%, var(--line));
}
.info {
  flex: 1 1 200px;
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

/* Круглая кнопка: символы дождя бегут по орбите, внутри кольцо-индикатор.
   Включено — кольцо замкнуто и светится, орбита быстрее; подключение — по
   кольцу бежит дуга. */
.knob {
  flex: none;
  margin: 0 auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}
.go {
  --c: var(--accent);
  position: relative;
  width: 108px;
  height: 108px;
  border-radius: 50%;
  border: 1px solid var(--line-2);
  padding: 0;
  background: var(--panel);
  color: var(--dim);
  display: grid;
  place-items: center;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
  transition:
    box-shadow 0.3s,
    color 0.2s,
    transform 0.15s;
}
.on .go {
  --c: var(--ok);
  color: var(--ok);
  box-shadow: 0 0 36px color-mix(in srgb, var(--ok) 28%, transparent);
}
.go.busy {
  color: var(--accent);
}
.go:hover:not(:disabled) {
  color: var(--c);
}
.go:active:not(:disabled) {
  transform: scale(0.96);
}
.go:disabled {
  cursor: default;
}
.go:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 18px;
}
.icon {
  position: relative;
  z-index: 2;
  width: 38%;
  height: 38%;
  overflow: visible;
}
.icon path {
  fill: none;
  stroke: currentColor;
  stroke-width: 2.4;
  stroke-linecap: round;
}
.orbit {
  position: absolute;
  inset: -14px;
  width: calc(100% + 28px);
  height: calc(100% + 28px);
  animation: turn 22s linear infinite;
}
.orbit text {
  font: 600 10.5px var(--mono);
  letter-spacing: 2.2px;
  fill: var(--faint);
  transition: fill 0.3s;
}
.go:hover:not(:disabled) .orbit text {
  fill: var(--accent);
}
.on .orbit {
  animation-duration: 9s;
}
.on .orbit text,
.on .go:hover .orbit text {
  fill: var(--ok);
}
.go.busy .orbit {
  animation-duration: 2.2s;
}
.ring {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  transform: rotate(-90deg);
}
.ring circle {
  fill: none;
  stroke-width: 3;
}
.track {
  stroke: var(--line);
}
.arc {
  stroke: var(--c);
  stroke-linecap: round;
  stroke-dasharray: 0 400;
  transition: stroke-dasharray 0.6s ease;
}
.on .arc {
  stroke-dasharray: 360 400;
  filter: drop-shadow(0 0 5px var(--ok));
}
.go.busy .arc {
  stroke: var(--accent);
  stroke-dasharray: 90 400;
  transform-origin: 64px 64px;
  animation: turn 1s linear infinite;
  filter: none;
}
.label {
  font-size: 13px;
  font-weight: 600;
  color: var(--dim);
}
.on .label {
  color: var(--ok);
}
@keyframes turn {
  to {
    transform: rotate(360deg);
  }
}
@media (prefers-reduced-motion: reduce) {
  .orbit,
  .go.busy .arc {
    animation: none;
  }
}
</style>
