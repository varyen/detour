<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from "vue";
import { RouterLink, RouterView, useRoute, useRouter } from "vue-router";
import MatrixRain from "@/components/MatrixRain.vue";
import SectionIcon from "@/components/SectionIcon.vue";
import CommandPalette from "@/components/CommandPalette.vue";
import ToastHost from "@/components/ToastHost.vue";
import LoginView from "@/views/LoginView.vue";
import { SECTIONS } from "@/router";
import { useSessionStore } from "@/stores/session";
import { useStatusStore } from "@/stores/status";
import { useCommandStore } from "@/stores/commands";
import { useProfilesStore } from "@/stores/profiles";
import { useToastStore } from "@/stores/toast";
import { useTheme } from "@/composables/useTheme";

const session = useSessionStore();
const status = useStatusStore();
const commands = useCommandStore();
const profiles = useProfilesStore();
const toast = useToastStore();
const route = useRoute();
const router = useRouter();
const { theme, toggle: toggleTheme } = useTheme();

const title = computed(() => (route.meta.title as string) ?? "Обзор");

const subtitle = computed(() => {
  const d = status.data;
  if (!d) return "Соединяюсь с роутером…";
  const parts = [
    d.platform === "keenetic" ? "Keenetic" : "OpenWrt",
    d.binaries?.singbox_version ? `sing-box ${d.binaries.singbox_version}` : "",
    d.version ? `панель ${d.version}` : "",
  ].filter(Boolean);
  return parts.join(" · ");
});

function openPalette() {
  commands.open = true;
}

function onKeydown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    commands.open = !commands.open;
  }
}

let unregister: (() => void) | undefined;
let dropProvider: (() => void) | undefined;

onMounted(async () => {
  addEventListener("keydown", onKeydown);
  await session.bootstrap();

  /* Базовые команды палитры: переходы по разделам и то, что нужно из любого
     места. Разделы добавят свои при монтировании. */
  unregister = commands.register([
    ...SECTIONS.map((s) => ({
      id: `go:${s.name}`,
      title: `Перейти: ${s.title}`,
      group: "разделы",
      run: () => void router.push(s.path),
    })),
    {
      id: "app:refresh",
      title: "Обновить состояние",
      group: "система",
      keywords: "статус перечитать",
      run: () => void status.refresh(),
    },
    {
      id: "app:theme",
      title: "Переключить тему",
      group: "система",
      keywords: "светлая тёмная",
      run: () => toggleTheme(),
    },
    {
      id: "app:logout",
      title: "Выйти из панели",
      group: "система",
      run: () => void session.logout(),
    },
  ]);

  /* Профили в палитре — не постоянными командами, а по мере набора: их сотня, и
     в пустой палитре они бы утопили всё остальное. Зато «⌘K, три буквы имени,
     Enter» становится самым коротким способом сменить VPN. */
  dropProvider = commands.addProvider((q) => {
    const hits = profiles.rows
      .filter((r) => !r.isActive && r.name.toLowerCase().includes(q))
      .slice(0, 8);
    return hits.map((r) => ({
      id: `vpn:${r.id}`,
      title: `Подключить: ${r.name}`,
      group: "vpn",
      run: () => void profiles.activate(r.id),
    }));
  });
});

onBeforeUnmount(() => {
  removeEventListener("keydown", onKeydown);
  status.stopPolling();
  unregister?.();
  dropProvider?.();
});

/* Данные тянем только после входа — до него любой запрос вернёт 401. */
async function start() {
  await status.refresh();
  void status.refreshExtras();
  /* Список профилей нужен палитре с любого экрана — иначе поиск по имени VPN
     работал бы только после захода в «Профили». */
  void profiles.load();
  status.startPolling();
}

session.$subscribe((_m, s) => {
  if (s.screen === "ready" && !status.data && !status.loading) void start();
});
</script>

<template>
  <MatrixRain />

  <div v-if="session.screen === 'loading'" class="boot">
    <span class="eyebrow">Detour</span>
  </div>

  <LoginView v-else-if="!session.authorized" />

  <div v-else class="app">
    <nav class="rail" aria-label="Разделы">
      <RouterLink
        v-for="s in SECTIONS"
        :key="s.name"
        class="rail-btn"
        :to="s.path"
        :aria-label="s.title"
      >
        <SectionIcon :name="s.icon" />
        <span class="tip">{{ s.title }}</span>
      </RouterLink>
    </nav>

    <main class="main">
      <header class="top">
        <div class="head">
          <h1>{{ title }}</h1>
          <p class="sub">{{ subtitle }}</p>
        </div>

        <button class="kbar" type="button" @click="openPalette">
          <SectionIcon name="search" :size="14" />
          <span class="kbar-text">Найти VPN или действие</span>
          <kbd class="kbar-key">⌘K</kbd>
        </button>

        <button
          class="icon-btn"
          type="button"
          :aria-label="theme === 'dark' ? 'Светлая тема' : 'Тёмная тема'"
          @click="toggleTheme"
        >
          <SectionIcon :name="theme === 'dark' ? 'sun' : 'moon'" :size="17" />
        </button>
        <button
          class="icon-btn"
          type="button"
          aria-label="Выйти"
          @click="session.logout()"
        >
          <SectionIcon name="logout" :size="17" />
        </button>
      </header>

      <p v-if="status.error" class="alert">{{ status.error }}</p>

      <RouterView v-slot="{ Component }">
        <component :is="Component" />
      </RouterView>
    </main>

    <nav class="tabbar" aria-label="Разделы">
      <RouterLink
        v-for="s in SECTIONS"
        :key="s.name"
        class="tab"
        :to="s.path"
      >
        <SectionIcon :name="s.icon" :size="20" />
        <span>{{ s.title }}</span>
      </RouterLink>
    </nav>
  </div>

  <CommandPalette />
  <ToastHost />
  <span class="sr" aria-live="polite">{{ toast.items.at(-1)?.text ?? "" }}</span>
</template>

<style scoped>
.boot {
  min-height: 100dvh;
  display: grid;
  place-items: center;
}

.app {
  display: grid;
  grid-template-columns: var(--rail) minmax(0, 1fr);
  min-height: 100dvh;
}

/* ---- рейка (десктоп) ---- */
.rail {
  border-right: 1px solid var(--line);
  background: var(--panel-2);
  padding: 14px 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  position: sticky;
  top: 0;
  height: 100dvh;
  /* sticky сам по себе создаёт stacking context, а карточки в .main — свой
     (backdrop-filter). Без явного z-index рейка красится ПОД контентом и
     тултипы разделов уезжают под карточки. Ниже шторок (50) и тостов (70). */
  z-index: 20;
}
.rail-btn {
  width: 44px;
  height: 44px;
  border-radius: 12px;
  color: var(--faint);
  display: grid;
  place-items: center;
  position: relative;
  transition: color 0.16s, background 0.16s;
}
.rail-btn:hover {
  color: var(--ink);
  background: var(--panel-2);
}
.rail-btn.router-link-exact-active {
  color: var(--accent);
  background: var(--accent-wash);
}
.rail-btn.router-link-exact-active::before {
  content: "";
  position: absolute;
  left: -14px;
  top: 12px;
  bottom: 12px;
  width: 2px;
  border-radius: 2px;
  background: var(--accent);
}
.tip {
  position: absolute;
  left: 54px;
  white-space: nowrap;
  font-size: 12px;
  padding: 5px 9px;
  border-radius: 7px;
  background: var(--ink);
  color: var(--ground);
  opacity: 0;
  pointer-events: none;
  transform: translateX(-4px);
  transition: opacity 0.14s, transform 0.14s;
  z-index: 5;
}
.rail-btn:hover .tip {
  opacity: 1;
  transform: none;
}

/* ---- основная колонка ---- */
.main {
  padding: 18px clamp(14px, 2.6vw, 34px) 60px;
  min-width: 0;
}
.top {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 20px;
}
.head h1 {
  font-size: 21px;
  font-weight: 620;
  letter-spacing: -0.02em;
}
.sub {
  font-size: 13px;
  color: var(--dim);
}
.kbar {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 9px;
  border: 1px solid var(--line);
  background: var(--panel);
  border-radius: 999px;
  padding: 7px 8px 7px 14px;
  font-size: 13px;
  color: var(--dim);
  min-width: 250px;
}
.kbar:hover {
  border-color: var(--line-2);
  color: var(--ink);
}
.kbar-key {
  margin-left: auto;
}
.icon-btn {
  border: 1px solid var(--line);
  background: var(--panel);
  color: var(--dim);
  border-radius: 999px;
  width: 36px;
  height: 36px;
  display: grid;
  place-items: center;
}
.icon-btn:hover {
  color: var(--ink);
  border-color: var(--line-2);
}

.alert {
  border: 1px solid color-mix(in srgb, var(--bad) 45%, transparent);
  background: color-mix(in srgb, var(--bad) 10%, transparent);
  color: var(--bad);
  border-radius: var(--radius-sm);
  padding: 10px 14px;
  font-size: 13.5px;
  margin-bottom: 16px;
}

/* ---- нижний бар (телефон) ---- */
.tabbar {
  display: none;
}

@media (max-width: 860px) {
  .app {
    grid-template-columns: minmax(0, 1fr);
  }
  .rail {
    display: none;
  }
  .main {
    padding: 14px 14px calc(var(--tabbar) + 28px + env(safe-area-inset-bottom));
  }
  .top {
    gap: 10px;
  }
  .head {
    flex: 1 1 auto;
    min-width: 0;
  }
  .head h1 {
    font-size: 19px;
  }
  .sub {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Строка поиска на телефоне сжимается до кнопки — место дороже подсказки. */
  .kbar {
    margin-left: 0;
    min-width: 0;
    width: 36px;
    height: 36px;
    padding: 0;
    justify-content: center;
    order: 3;
  }
  .kbar-text,
  .kbar-key {
    display: none;
  }

  .tabbar {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 30;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    height: calc(var(--tabbar) + env(safe-area-inset-bottom));
    padding-bottom: env(safe-area-inset-bottom);
    border-top: 1px solid var(--line);
    background: color-mix(in srgb, var(--ground) 92%, transparent);
    backdrop-filter: blur(14px);
  }
  .tab {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    font-size: 10.5px;
    color: var(--faint);
    text-decoration: none;
    /* Цель нажатия во всю высоту бара — попасть пальцем должно быть легко. */
    min-height: 44px;
  }
  .tab.router-link-exact-active {
    color: var(--accent);
  }
}

.sr {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
}
</style>
