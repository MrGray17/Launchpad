// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  addProject,
  chooseProjectFolder,
  importLegacyProjects,
  isDesktopRuntime,
  loadLibrary,
  openInCode,
  openTerminal,
  refreshProject,
  saveProjectFocus,
  selectProject,
} from "./desktop";

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: native.open }));

function enableDesktopRuntime() {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
}

function disableDesktopRuntime() {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
}

describe("desktop platform boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    enableDesktopRuntime();
    native.invoke.mockResolvedValue(undefined);
    native.open.mockResolvedValue(null);
  });

  afterEach(disableDesktopRuntime);

  it("maps every library operation to its exact native command and payload", async () => {
    await loadLibrary();
    await importLegacyProjects([{ path: "C:\\repo", quest: "Ship" }]);
    await addProject("C:\\repo");
    await refreshProject(4);
    await selectProject(null);
    await saveProjectFocus(4, "Ship", "Run the tests");
    await openInCode(4);
    await openTerminal(4);

    expect(native.invoke.mock.calls).toEqual([
      ["load_library", undefined],
      ["import_legacy_projects", { projects: [{ path: "C:\\repo", quest: "Ship" }] }],
      ["add_project", { path: "C:\\repo" }],
      ["refresh_project", { id: 4 }],
      ["select_project", { id: null }],
      ["save_project_focus", { id: 4, quest: "Ship", checkpoint: "Run the tests" }],
      ["open_in_vscode", { id: 4 }],
      ["open_terminal", { id: 4 }],
    ]);
  });

  it("configures the native folder picker for one directory", async () => {
    native.open.mockResolvedValue("C:\\repo");

    await expect(chooseProjectFolder()).resolves.toBe("C:\\repo");
    expect(native.open).toHaveBeenCalledWith({ directory: true, multiple: false });
  });

  it("keeps browser preview read-only and never attempts native IPC", async () => {
    disableDesktopRuntime();

    expect(isDesktopRuntime()).toBe(false);
    await expect(loadLibrary()).resolves.toEqual({ projects: [], activeProjectId: null });
    await expect(chooseProjectFolder()).resolves.toBeNull();
    await expect(addProject("C:\\repo")).rejects.toThrow(
      "This action is available in the Launchpad desktop app.",
    );
    expect(native.invoke).not.toHaveBeenCalled();
    expect(native.open).not.toHaveBeenCalled();
  });
});
