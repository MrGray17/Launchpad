import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";

export type GitStatus = "clean" | "dirty" | "unknown";
export type MetadataStatus =
  | "fresh"
  | "unknown"
  | "not-a-repository"
  | "git-unavailable"
  | "invalid-repository"
  | "timeout";

export type Project = {
  id: number;
  name: string;
  branch: string;
  gitStatus: GitStatus;
  metadataStatus: MetadataStatus;
  scripts: string[];
  quest: string;
  checkpoint: string;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt: string | null;
  metadataRefreshedAt: string | null;
  available: boolean;
};

export type LibraryState = {
  projects: Project[];
  activeProjectId: number | null;
};

export type LegacyProject = {
  legacyId: string;
  path: string;
  quest?: string;
  checkpoint?: string;
};

export type BootstrapState = LibraryState & {
  pendingLegacyIds: string[];
  legacyMigrationComplete: boolean;
};

export type BackupResult = {
  fileName: string;
};

export function isDesktopRuntime() {
  return isTauri();
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktopRuntime()) {
    throw new Error("This action is available in the Launchpad desktop app.");
  }
  return tauriInvoke<T>(command, args);
}

export async function chooseProjectFolder(): Promise<string | null> {
  if (!isDesktopRuntime()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export function bootstrapLibrary(
  projects: LegacyProject[],
  activeLegacyId: string | null,
): Promise<BootstrapState> {
  if (!isDesktopRuntime()) {
    return Promise.resolve({
      projects: [],
      activeProjectId: null,
      pendingLegacyIds: [],
      legacyMigrationComplete: true,
    });
  }
  return invoke<BootstrapState>("bootstrap", { projects, activeLegacyId });
}

export function addProject(path: string): Promise<Project> {
  return invoke<Project>("add_project", { path });
}

export function activateProject(id: number): Promise<Project> {
  return invoke<Project>("activate_project", { id });
}

export function refreshProject(id: number): Promise<Project> {
  return invoke<Project>("refresh_project", { id });
}

export function relinkProject(id: number, path: string): Promise<Project> {
  return invoke<Project>("relink_project", { id, path });
}

export function removeProject(id: number): Promise<LibraryState> {
  return invoke<LibraryState>("remove_project", { id });
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

export function openTerminal(id: number): Promise<Project> {
  return invoke<Project>("open_terminal", { id });
}

export function backupLibrary(): Promise<BackupResult> {
  return invoke<BackupResult>("backup_library");
}

export function exportLibrary(): Promise<BackupResult | null> {
  return invoke<BackupResult | null>("export_library");
}

export function restoreLibrary(): Promise<LibraryState | null> {
  return invoke<LibraryState | null>("restore_library");
}
