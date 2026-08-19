export type GitStatus = "clean" | "dirty" | "unknown";

export type Project = {
  id: number;
  name: string;
  path: string;
  branch: string;
  gitStatus: GitStatus;
  scripts: string[];
  quest: string;
  checkpoint: string;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt: string | null;
};

export type LibraryState = {
  projects: Project[];
  activeProjectId: number | null;
};

export type LegacyProject = {
  path: string;
  quest?: string;
  checkpoint?: string;
};

export function isDesktopRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktopRuntime()) {
    throw new Error("This action is available in the Launchpad desktop app.");
  }
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function chooseProjectFolder(): Promise<string | null> {
  if (!isDesktopRuntime()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export function loadLibrary(): Promise<LibraryState> {
  if (!isDesktopRuntime()) {
    return Promise.resolve({ projects: [], activeProjectId: null });
  }
  return invoke<LibraryState>("load_library");
}

export function importLegacyProjects(projects: LegacyProject[]): Promise<LibraryState> {
  return invoke<LibraryState>("import_legacy_projects", { projects });
}

export function addProject(path: string): Promise<Project> {
  return invoke<Project>("add_project", { path });
}

export function refreshProject(id: number): Promise<Project> {
  return invoke<Project>("refresh_project", { id });
}

export function selectProject(id: number | null): Promise<void> {
  return invoke<void>("select_project", { id });
}

export function saveProjectFocus(
  id: number,
  quest: string,
  checkpoint: string,
): Promise<Project> {
  return invoke<Project>("save_project_focus", { id, quest, checkpoint });
}

export function openInCode(id: number): Promise<Project> {
  return invoke<Project>("open_in_vscode", { id });
}

export function openTerminal(id: number): Promise<void> {
  return invoke<void>("open_terminal", { id });
}
