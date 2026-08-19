// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { BootstrapState, Project } from "./platform/desktop";
import { THEME_STORAGE_KEY } from "./theme";

const desktop = vi.hoisted(() => ({
  activateProject: vi.fn(),
  addProject: vi.fn(),
  backupLibrary: vi.fn(),
  bootstrapLibrary: vi.fn(),
  chooseBackupDestination: vi.fn(),
  chooseBackupFile: vi.fn(),
  chooseProjectFolder: vi.fn(),
  exportLibrary: vi.fn(),
  isDesktopRuntime: vi.fn(() => true),
  openInCode: vi.fn(),
  openTerminal: vi.fn(),
  refreshProject: vi.fn(),
  relinkProject: vi.fn(),
  removeProject: vi.fn(),
  restoreLibrary: vi.fn(),
  saveProjectFocus: vi.fn(),
}));

vi.mock("./platform/desktop", () => desktop);

const project: Project = {
  id: 7,
  name: "Rate Limiter",
  branch: "main",
  gitStatus: "clean",
  metadataStatus: "fresh",
  scripts: ["build", "test"],
  quest: "Prove the token bucket",
  checkpoint: "Add the burst capacity regression test.",
  createdAt: "2026-08-18T10:00:00.000Z",
  updatedAt: "2026-08-18T10:00:00.000Z",
  lastOpenedAt: null,
  metadataRefreshedAt: "2026-08-19T10:00:00.000Z",
  available: true,
};
const secondProject: Project = {
  ...project,
  id: 8,
  name: "Compiler Lab",
  branch: "feat/parser",
  gitStatus: "dirty",
  quest: "Finish the parser",
};
const bootstrap: BootstrapState = {
  projects: [project],
  activeProjectId: project.id,
  pendingLegacyIds: [],
  legacyMigrationComplete: true,
};

function openAppMenu() {
  fireEvent.click(screen.getByRole("button", { name: "App menu" }));
}

function openProjectMenu() {
  fireEvent.click(screen.getByRole("button", { name: "Project options" }));
}

describe("Launchpad hardened project lifecycle", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    desktop.isDesktopRuntime.mockReturnValue(true);
    desktop.bootstrapLibrary.mockResolvedValue(bootstrap);
    desktop.chooseProjectFolder.mockResolvedValue(null);
    desktop.chooseBackupFile.mockResolvedValue(null);
    desktop.chooseBackupDestination.mockResolvedValue(null);
    desktop.activateProject.mockResolvedValue(project);
    desktop.addProject.mockResolvedValue(project);
    desktop.refreshProject.mockResolvedValue(project);
    desktop.relinkProject.mockResolvedValue(project);
    desktop.removeProject.mockResolvedValue({ projects: [], activeProjectId: null });
    desktop.saveProjectFocus.mockResolvedValue(project);
    desktop.openInCode.mockResolvedValue({ ...project, lastOpenedAt: "2026-08-19T11:00:00.000Z" });
    desktop.openTerminal.mockResolvedValue({ ...project, lastOpenedAt: "2026-08-19T11:00:00.000Z" });
    desktop.backupLibrary.mockResolvedValue({ fileName: "launchpad-backup-123.sqlite3" });
    desktop.exportLibrary.mockResolvedValue({ fileName: "export.sqlite3" });
    desktop.restoreLibrary.mockResolvedValue({ projects: [project], activeProjectId: project.id });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("bootstraps once under Strict Mode and renders refreshed metadata", async () => {
    render(<StrictMode><App /></StrictMode>);
    expect(await screen.findByRole("button", { name: /Continue Rate Limiter/ })).toBeTruthy();
    expect(desktop.bootstrapLibrary).toHaveBeenCalledOnce();
    expect(desktop.bootstrapLibrary).toHaveBeenCalledWith([], null);
    expect(document.querySelector(".git-line")?.textContent).toContain("main");
  });

  it("uses the actual Launchpad mark and removes fake window controls", async () => {
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    expect(document.querySelector(".brand-mark")).toBeTruthy();
    expect(document.querySelector(".window-dots")).toBeNull();
  });

  it("persists the optional dark appearance from the app menu", async () => {
    const first = render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    openAppMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: /Dark appearance/ }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");

    first.unmount();
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    openAppMenu();
    expect(screen.getByRole("menuitem", { name: /Light appearance/ })).toBeTruthy();
  });

  it("opens editor and terminal by id and applies returned timestamps", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /Continue Rate Limiter/ }));
    await waitFor(() => expect(desktop.openInCode).toHaveBeenCalledWith(7));
    fireEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await waitFor(() => expect(desktop.openTerminal).toHaveBeenCalledWith(7));
  });

  it("uses one global operation lock to prevent overlapping actions", async () => {
    let resolveFolder: (path: string | null) => void = () => undefined;
    desktop.chooseProjectFolder.mockReturnValue(new Promise((resolve) => { resolveFolder = resolve; }));
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Add project/ }));
    const continueButton = screen.getByRole("button", { name: /Continue Rate Limiter/ });
    expect(continueButton).toHaveProperty("disabled", true);
    fireEvent.click(continueButton);
    expect(desktop.openInCode).not.toHaveBeenCalled();
    resolveFolder(null);
    await waitFor(() => expect(continueButton).toHaveProperty("disabled", false));
  });

  it("adds a real folder through the native upsert", async () => {
    const added = { ...secondProject, id: 12, name: "Launchpad" };
    desktop.chooseProjectFolder.mockResolvedValue("C:\\Launchpad");
    desktop.addProject.mockResolvedValue(added);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Add project/ }));
    expect(await screen.findByRole("button", { name: /Continue Launchpad/ })).toBeTruthy();
    expect(desktop.addProject).toHaveBeenCalledWith("C:\\Launchpad");
  });

  it("activates through one native operation and preserves selection on failure", async () => {
    desktop.bootstrapLibrary.mockResolvedValue({ ...bootstrap, projects: [project, secondProject] });
    desktop.activateProject.mockRejectedValueOnce("That project folder does not exist.");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /Compiler Lab/ }));
    expect((await screen.findByRole("status")).textContent).toContain("does not exist");
    expect(screen.getByRole("button", { name: /Continue Rate Limiter/ })).toBeTruthy();
  });

  it("keeps maintenance actions out of the main hierarchy but makes them available in project options", async () => {
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    expect(screen.queryByRole("button", { name: "Refresh metadata" })).toBeNull();
    openProjectMenu();
    expect(screen.getByRole("menuitem", { name: "Refresh metadata" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Relink folder…" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Remove from Launchpad" })).toBeTruthy();
  });

  it("refreshes metadata and promotes recovery controls only when a folder is missing", async () => {
    desktop.refreshProject.mockRejectedValue("That project folder does not exist.");
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    openProjectMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Refresh metadata" }));
    expect((await screen.findByRole("alert")).textContent).toContain("cannot find");
    expect(screen.getByRole("button", { name: "Relink folder" })).toBeTruthy();
  });

  it("relinks a missing project without exposing its old path", async () => {
    const missing = { ...project, available: false };
    const relinked = { ...project, name: "Rate Limiter Restored", available: true };
    desktop.bootstrapLibrary.mockResolvedValue({ ...bootstrap, projects: [missing] });
    desktop.chooseProjectFolder.mockResolvedValue("D:\\Repos\\rate-limiter");
    desktop.relinkProject.mockResolvedValue(relinked);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Relink folder" }));
    await waitFor(() => expect(desktop.relinkProject).toHaveBeenCalledWith(7, "D:\\Repos\\rate-limiter"));
    expect(await screen.findByRole("button", { name: /Continue Rate Limiter Restored/ })).toBeTruthy();
  });

  it("removes a project while explicitly preserving its filesystem", async () => {
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    openProjectMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Remove from Launchpad" }));
    await waitFor(() => expect(desktop.removeProject).toHaveBeenCalledWith(7));
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining("source files will stay untouched"));
    expect(await screen.findByRole("button", { name: "Choose a project folder" })).toBeTruthy();
  });

  it("pins focus drafts to the project that opened the modal and restores focus", async () => {
    const updated = { ...project, quest: "Ship safely", checkpoint: "Run the recovery test." };
    desktop.saveProjectFocus.mockResolvedValue(updated);
    render(<App />);
    const edit = await screen.findByRole("button", { name: "Edit focus" });
    fireEvent.click(edit);
    fireEvent.change(screen.getByLabelText("Current quest"), { target: { value: updated.quest } });
    fireEvent.change(screen.getByLabelText("Checkpoint"), { target: { value: updated.checkpoint } });
    fireEvent.click(screen.getByRole("button", { name: "Save focus" }));
    await waitFor(() => expect(desktop.saveProjectFocus).toHaveBeenCalledWith(7, updated.quest, updated.checkpoint));
    await waitFor(() => expect(document.activeElement).toBe(edit));
  });

  it("keeps failed legacy records and restores the legacy active id", async () => {
    localStorage.setItem(LEGACY_PROJECTS_KEY, JSON.stringify([
      { id: "ready", path: "C:\\ready", quest: "Ready" },
      { id: "offline", path: "D:\\offline", checkpoint: "Drive missing" },
    ]));
    localStorage.setItem(LEGACY_ACTIVE_KEY, "offline");
    desktop.bootstrapLibrary.mockResolvedValue({ ...bootstrap, pendingLegacyIds: ["offline"], legacyMigrationComplete: false });
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    expect(desktop.bootstrapLibrary).toHaveBeenCalledWith([
      { legacyId: "ready", path: "C:\\ready", quest: "Ready", checkpoint: undefined },
      { legacyId: "offline", path: "D:\\offline", quest: undefined, checkpoint: "Drive missing" },
    ], "offline");
    expect(JSON.parse(localStorage.getItem(LEGACY_PROJECTS_KEY) ?? "[]")).toEqual([
      { id: "offline", path: "D:\\offline", checkpoint: "Drive missing" },
    ]);
    expect(localStorage.getItem(LEGACY_ACTIVE_KEY)).toBe("offline");
  });

  it("leaves malformed prototype data untouched for manual recovery", async () => {
    localStorage.setItem(LEGACY_PROJECTS_KEY, "{not-json");
    render(<App />);
    expect((await screen.findByRole("alert")).textContent).toContain("left it untouched");
    expect(localStorage.getItem(LEGACY_PROJECTS_KEY)).toBe("{not-json");
    expect(desktop.bootstrapLibrary).not.toHaveBeenCalled();
  });

  it("creates, exports, and restores backups from the app menu", async () => {
    desktop.chooseBackupDestination.mockResolvedValue("D:\\export.sqlite3");
    desktop.chooseBackupFile.mockResolvedValue("D:\\restore.sqlite3");
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });

    openAppMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Back up now" }));
    expect((await screen.findByRole("status")).textContent).toContain("launchpad-backup-123.sqlite3");

    openAppMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Export backup…" }));
    await waitFor(() => expect(desktop.exportLibrary).toHaveBeenCalledWith("D:\\export.sqlite3"));

    openAppMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore backup…" }));
    await waitFor(() => expect(desktop.restoreLibrary).toHaveBeenCalledWith("D:\\restore.sqlite3"));
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining("safety backup"));
  });

  it("gives known projects distinct visual motifs and Unicode-safe initials", async () => {
    const names = ["Rate Limiter", "Nest", "Maw3id", "Sifr", "Launchpad", "🌸 Garden"];
    desktop.bootstrapLibrary.mockResolvedValue({
      ...bootstrap,
      projects: names.map((name, index) => ({ ...project, id: index + 1, name })),
      activeProjectId: 1,
    });
    render(<App />);
    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    const motifs = [...document.querySelectorAll(".project-cover .project-art")].map((node) => node.getAttribute("data-motif"));
    expect(motifs).toEqual(expect.arrayContaining(["signal", "window", "queue", "ledger", "spark"]));
    expect([...document.querySelectorAll(".art-symbol")].some((node) => node.textContent === "🌸G")).toBe(true);
  });

  it("keeps browser preview read-only with an honest empty state", async () => {
    desktop.isDesktopRuntime.mockReturnValue(false);
    desktop.bootstrapLibrary.mockResolvedValue({ projects: [], activeProjectId: null, pendingLegacyIds: [], legacyMigrationComplete: true });
    render(<App />);
    expect(await screen.findByRole("button", { name: "Choose a project folder" })).toBeTruthy();
    expect(desktop.bootstrapLibrary).toHaveBeenCalledWith([], null);
  });
});

const LEGACY_PROJECTS_KEY = "launchpad.projects.v1";
const LEGACY_ACTIVE_KEY = "launchpad.active-project.v1";
