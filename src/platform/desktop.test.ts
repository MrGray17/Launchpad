// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  activateProject,
  addProject,
  backupLibrary,
  bootstrapLibrary,
  chooseBackupDestination,
  chooseBackupFile,
  chooseProjectFolder,
  exportLibrary,
  isDesktopRuntime,
  openInCode,
  openTerminal,
  refreshProject,
  relinkProject,
  removeProject,
  restoreLibrary,
  saveProjectFocus,
} from "./desktop";

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
  open: vi.fn(),
  save: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, isTauri: native.isTauri }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: native.open, save: native.save }));

describe("desktop platform boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    native.isTauri.mockReturnValue(true);
    native.invoke.mockResolvedValue(undefined);
    native.open.mockResolvedValue(null);
    native.save.mockResolvedValue(null);
  });

  afterEach(() => vi.restoreAllMocks());

  it("maps the narrow project and recovery API to exact native commands", async () => {
    const legacy = [{ legacyId: "old", path: "C:\\repo", quest: "Ship" }];
    await bootstrapLibrary(legacy, "old");
    await addProject("C:\\repo");
    await activateProject(4);
    await refreshProject(4);
    await relinkProject(4, "D:\\repo");
    await removeProject(4);
    await saveProjectFocus(4, "Ship", "Run the tests");
    await openInCode(4);
    await openTerminal(4);
    await backupLibrary();
    await exportLibrary("D:\\launchpad-backup.sqlite3");
    await restoreLibrary("D:\\launchpad-backup.sqlite3");

    expect(native.invoke.mock.calls).toEqual([
      ["bootstrap", { projects: legacy, activeLegacyId: "old" }],
      ["add_project", { path: "C:\\repo" }],
      ["activate_project", { id: 4 }],
      ["refresh_project", { id: 4 }],
      ["relink_project", { id: 4, path: "D:\\repo" }],
      ["remove_project", { id: 4 }],
      ["save_project_focus", { id: 4, quest: "Ship", checkpoint: "Run the tests" }],
      ["open_in_vscode", { id: 4 }],
      ["open_terminal", { id: 4 }],
      ["backup_library", undefined],
      ["export_library", { path: "D:\\launchpad-backup.sqlite3" }],
      ["restore_library", { path: "D:\\launchpad-backup.sqlite3" }],
    ]);
  });

  it("uses native dialogs for folders, backup import, and backup export", async () => {
    native.open.mockResolvedValueOnce("C:\\repo").mockResolvedValueOnce("D:\\backup.sqlite3");
    native.save.mockResolvedValue("D:\\export.sqlite3");

    expect(isDesktopRuntime()).toBe(true);
    await expect(chooseProjectFolder()).resolves.toBe("C:\\repo");
    await expect(chooseBackupFile()).resolves.toBe("D:\\backup.sqlite3");
    await expect(chooseBackupDestination()).resolves.toBe("D:\\export.sqlite3");

    expect(native.open).toHaveBeenNthCalledWith(1, { directory: true, multiple: false });
    expect(native.open).toHaveBeenNthCalledWith(2, {
      directory: false,
      multiple: false,
      filters: [{ name: "Launchpad backup", extensions: ["sqlite3"] }],
    });
    expect(native.save).toHaveBeenCalledWith({
      defaultPath: "launchpad-backup.sqlite3",
      filters: [{ name: "Launchpad backup", extensions: ["sqlite3"] }],
    });
  });

  it("keeps browser preview read-only and never attempts native IPC", async () => {
    native.isTauri.mockReturnValue(false);
    expect(isDesktopRuntime()).toBe(false);
    await expect(bootstrapLibrary([], null)).resolves.toEqual({
      projects: [], activeProjectId: null, pendingLegacyIds: [], legacyMigrationComplete: true,
    });
    await expect(chooseProjectFolder()).resolves.toBeNull();
    await expect(chooseBackupFile()).resolves.toBeNull();
    await expect(chooseBackupDestination()).resolves.toBeNull();
    await expect(addProject("C:\\repo")).rejects.toThrow(
      "This action is available in the Launchpad desktop app.",
    );
    expect(native.invoke).not.toHaveBeenCalled();
    expect(native.open).not.toHaveBeenCalled();
    expect(native.save).not.toHaveBeenCalled();
  });
});
