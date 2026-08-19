// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { applyTheme, initializeTheme, readTheme, THEME_STORAGE_KEY } from "./theme";

describe("dark-grey theme preference", () => {
  afterEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("style");
    document.querySelector('meta[name="theme-color"]')?.remove();
    vi.restoreAllMocks();
  });

  it("defaults invalid or absent preferences to the original light theme", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "sepia");
    expect(readTheme()).toBe("light");
  });

  it("applies and persists dark grey across launches", () => {
    document.head.insertAdjacentHTML("beforeend", '<meta name="theme-color" content="#fff">');
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
    expect(document.querySelector('meta[name="theme-color"]')?.getAttribute("content")).toBe("#202124");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(initializeTheme()).toBe("dark");
  });

  it("still applies the theme when preference storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("Storage blocked");
    });
    expect(() => applyTheme("dark")).not.toThrow();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
