import { createRouter, createWebHashHistory } from "vue-router";

/* Хеш-роутинг, а не history: uhttpd и lighttpd на роутере не умеют отдавать
   index.html на произвольный путь, а городить rewrite ради красивого URL в
   локальной панели — плохой размен. */

export const SECTIONS = [
  { name: "overview", path: "/", title: "Обзор", icon: "gauge" },
  { name: "profiles", path: "/profiles", title: "Профили", icon: "list" },
  { name: "rules", path: "/rules", title: "Правила", icon: "route" },
  { name: "services", path: "/services", title: "Сервисы", icon: "server" },
  { name: "journal", path: "/journal", title: "Журнал", icon: "terminal" },
] as const;

export type SectionName = (typeof SECTIONS)[number]["name"];

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "overview",
      component: () => import("@/views/OverviewView.vue"),
      meta: { title: "Обзор" },
    },
    {
      path: "/profiles",
      name: "profiles",
      component: () => import("@/views/ProfilesView.vue"),
      meta: { title: "Профили и подписки" },
    },
    {
      path: "/rules",
      name: "rules",
      component: () => import("@/views/RulesView.vue"),
      meta: { title: "Правила маршрутизации" },
    },
    {
      path: "/services",
      name: "services",
      component: () => import("@/views/ServicesView.vue"),
      meta: { title: "Сервисы и доступ" },
    },
    {
      path: "/journal",
      name: "journal",
      component: () => import("@/views/JournalView.vue"),
      meta: { title: "Журнал" },
    },
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});

/* Панель обновляется прямо под открытой вкладкой: opkg кладёт новые файлы, а имена
   кусков содержат хэш — старая вкладка уходит за кусок, которого больше нет, и
   переход по разделу молча умирает. Ловим ровно этот случай и перезагружаемся
   один раз (флаг в sessionStorage), чтобы при настоящей поломке не зациклиться. */
const RELOADED = "detour:chunk-reload";

router.onError((err, to) => {
  const msg = String((err as Error)?.message ?? err);
  const missing =
    /dynamically imported module|Importing a module script failed|Failed to fetch/i.test(msg);
  if (!missing) return;
  if (sessionStorage.getItem(RELOADED)) return;
  sessionStorage.setItem(RELOADED, "1");
  location.replace(location.pathname + "#" + to.fullPath);
  location.reload();
});

router.afterEach(() => {
  sessionStorage.removeItem(RELOADED);
});
