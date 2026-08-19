// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { LibraryState, Project } from "./platform/desktop";

const desktop = vi.hoisted(() => ({
  addProject: vi.fn(),
  chooseProjectFolder: vi.fn(),
  importLegacyProjects: vi.fn(),
  isDesktopRuntime: vi.fn(() => true),
  loadLibrary: vi.fn(),
  openInCode: vi.fn(),
  openTerminal: vi.fn(),
  refreshProject: vi.fn(),
  saveProjectFocus: vi.fn(),
  selectProject: vi.fn(),
}));

vi.mock("./platform/desktop", () => desktop);

const project: Project = {
  id: 7,
  name: "Rate Limiter",
  path: "\\\\?\\C:\\Users\\vPro\\rate-limiter",
  branch: "master",
  gitStatus: "clean",
  scripts: ["build", "test"],
  quest: "Prove the token bucket",
  checkpoint: "Add the burst capacity regression test.",
  createdAt: "2026-08-18T10:00:00.000Z",
  updatedAt: "2026-08-18T10:00:00.000Z",
  lastOpenedAt: null,
};

const library: LibraryState = { projects: [project], activeProjectId: project.id };
const secondProject: Project = {
  ...project,
  id: 8,
  name: "Compiler Lab",
  path: "C:\\CompilerLab",
  branch: "feat/parser",
  gitStatus: "dirty",
  quest: "Finish the parser",
};

describe("Launchpad project flow", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    desktop.isDesktopRuntime.mockReturnValue(true);
    desktop.loadLibrary.mockResolvedValue(library);
    desktop.importLegacyProjects.mockResolvedValue(library);
    desktop.chooseProjectFolder.mockResolvedValue(null);
    desktop.addProject.mockResolvedValue(project);
    desktop.refreshProject.mockResolvedValue(project);
    desktop.selectProject.mockResolvedValue(undefined);
    desktop.saveProjectFocus.mockResolvedValue(project);
    desktop.openTerminal.mockResolvedValue(undefined);
    desktop.openInCode.mockResolvedValue({ ...project, lastOpenedAt: "2026-08-19T10:00:00.000Z" });
  });

  afterEach(cleanup);

  it("loads the native library and launches project actions by database id", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Continue Rate Limiter/ }));
    await waitFor(() => expect(desktop.openInCode).toHaveBeenCalledWith(7));

    fireEvent.click(screen.getByRole("button", { name: "Open terminal" }));
    await waitFor(() => expect(desktop.openTerminal).toHaveBeenCalledWith(7));
    expect(document.querySelector(".git-line")?.textContent).toContain("master");
    expect(screen.getByText("build · test")).toBeTruthy();
  });

  it("adds a folder through the native duplicate-safe project command", async () => {
    const added = { ...project, id: 12, name: "Launchpad", path: "C:\\Launchpad" };
    desktop.chooseProjectFolder.mockResolvedValue("C:\\Launchpad");
    desktop.addProject.mockResolvedValue(added);

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Add project/ }));

    expect(await screen.findByRole("button", { name: /Continue Launchpad/ })).toBeTruthy();
    expect(desktop.addProject).toHaveBeenCalledWith("C:\\Launchpad");
  });

  it("saves both quest and checkpoint through SQLite", async () => {
    const updated = {
      ...project,
      quest: "Ship the token bucket",
      checkpoint: "Capacity and refill tests are next.",
    };
    desktop.saveProjectFocus.mockResolvedValue(updated);

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /Edit focus/ }));
    fireEvent.change(screen.getByLabelText("Current quest"), { target: { value: updated.quest } });
    fireEvent.change(screen.getByLabelText("Checkpoint"), { target: { value: updated.checkpoint } });
    fireEvent.click(screen.getByRole("button", { name: /Save focus/ }));

    await waitFor(() => expect(desktop.saveProjectFocus).toHaveBeenCalledWith(
      7,
      updated.quest,
      updated.checkpoint,
    ));
    expect(await screen.findByText(updated.quest)).toBeTruthy();
  });

  it("imports real legacy paths once and removes prototype storage", async () => {
    localStorage.setItem("launchpad.projects.v1", JSON.stringify([
      { name: "Decorative sample" },
      { path: "C:\\real-project", quest: "Keep this quest", checkpoint: "Keep this note" },
    ]));
    localStorage.setItem("launchpad.active-project.v1", "old-id");

    render(<App />);

    await screen.findByRole("button", { name: /Continue Rate Limiter/ });
    expect(desktop.importLegacyProjects).toHaveBeenCalledWith([
      { path: "C:\\real-project", quest: "Keep this quest", checkpoint: "Keep this note" },
    ]);
    expect(localStorage.getItem("launchpad.projects.v1")).toBeNull();
    expect(localStorage.getItem("launchpad.active-project.v1")).toBeNull();
  });

  it("shows an honest empty state without seeded showcase projects", async () => {
    desktop.loadLibrary.mockResolvedValue({ projects: [], activeProjectId: null });
    render(<App />);

    expect(await screen.findByRole("button", { name: "Choose a project folder" })).toBeTruthy();
    expect(screen.queryByText("Sifr")).toBeNull();
    expect(screen.queryByText("Maw3id")).toBeNull();
  });

  it("recovers from a library load failure without restarting the app", async () => {
    desktop.loadLibrary.mockRejectedValueOnce("The database is temporarily locked.");
    render(<App />);

    expect((await screen.findByRole("alert")).textContent).toContain("temporarily locked");
    desktop.loadLibrary.mockResolvedValue(library);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    expect(await screen.findByRole("button", { name: /Continue Rate Limiter/ })).toBeTruthy();
    expect(desktop.loadLibrary).toHaveBeenCalledTimes(2);
  });

  it("persists selection and refreshes repository metadata", async () => {
    desktop.loadLibrary.mockResolvedValue({
      projects: [project, secondProject],
      activeProjectId: project.id,
    });
    desktop.refreshProject.mockResolvedValue({ ...secondProject, branch: "feat/parser-v2" });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Compiler Lab/ }));

    await waitFor(() => expect(desktop.selectProject).toHaveBeenCalledWith(8));
    expect(desktop.refreshProject).toHaveBeenCalledWith(8);
    expect(await screen.findByRole("button", { name: /Continue Compiler Lab/ })).toBeTruthy();
    expect(document.querySelector(".git-line")?.textContent).toContain("feat/parser-v2");
  });

  it("rolls back an optimistic selection when native refresh fails", async () => {
    desktop.loadLibrary.mockResolvedValue({
      projects: [project, secondProject],
      activeProjectId: project.id,
    });
    desktop.refreshProject.mockRejectedValue("That project folder does not exist.");
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Compiler Lab/ }));

    expect((await screen.findByRole("status")).textContent).toContain("does not exist");
    expect(screen.getByRole("button", { name: /Continue Rate Limiter/ })).toBeTruthy();
  });

  it("keeps unsaved focus visible when native validation rejects it", async () => {
    desktop.saveProjectFocus.mockRejectedValue("Keep the current quest between 1 and 120 characters.");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /Edit focus/ }));
    fireEvent.change(screen.getByLabelText("Current quest"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: /Save focus/ }));

    expect((await screen.findByRole("status")).textContent).toContain("between 1 and 120");
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByLabelText("Checkpoint")).toHaveProperty("value", project.checkpoint);
  });

  it("does not duplicate a project when the native upsert returns an existing id", async () => {
    desktop.chooseProjectFolder.mockResolvedValue(project.path);
    desktop.addProject.mockResolvedValue({ ...project, branch: "feat/refill" });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Add project/ }));

    await waitFor(() => expect(desktop.addProject).toHaveBeenCalledWith(project.path));
    expect(document.querySelectorAll(".project-cover:not(.add-cover)")).toHaveLength(1);
    expect(document.querySelector(".git-line")?.textContent).toContain("feat/refill");
  });

  it("treats closing the folder picker as cancellation", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Add project/ }));

    await waitFor(() => expect(desktop.chooseProjectFolder).toHaveBeenCalledOnce());
    expect(desktop.addProject).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
