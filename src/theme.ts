export type Theme = "light" | "dark";

export const THEME_STORAGE_KEY = "launchpad.theme.v1";

export function readTheme(): Theme {
  try {
    return localStorage.getItem(THEME_STORAGE_KEY) === "dark" ? "dark" : "light";
  } catch {
    return "light";
  }
}

export function applyTheme(theme: Theme, persist = true) {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  document
    .querySelector<HTMLMetaElement>('meta[name="theme-color"]')
    ?.setAttribute("content", theme === "dark" ? "#202124" : "#f7efe6");

  if (persist) {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // The visible theme still works when webview storage is unavailable.
    }
  }
}

export function initializeTheme() {
  const theme = readTheme();
  applyTheme(theme, false);
  return theme;
}
