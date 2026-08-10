import { ref } from "vue";

export type Theme = "light" | "dark";

function current(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

const theme = ref<Theme>(current());

export function useTheme() {
  function set(t: Theme) {
    theme.value = t;
    document.documentElement.dataset.theme = t;
    try {
      localStorage.setItem("detour-theme", t);
    } catch {
      /* приватный режим — тема просто не запомнится */
    }
    /* Строка адреса браузера на телефоне красится в цвет фона. */
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) {
      meta.setAttribute(
        "content",
        getComputedStyle(document.documentElement).getPropertyValue("--ground").trim(),
      );
    }
  }

  const toggle = () => set(theme.value === "dark" ? "light" : "dark");

  return { theme, set, toggle };
}
