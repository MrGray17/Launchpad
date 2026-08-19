export type ProjectSnapshot = {
  name: string;
  path: string;
  branch: string;
  gitStatus: "clean" | "dirty" | "unknown";
  scripts: string[];
};

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

export async function chooseProjectFolder(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function inspectProject(path: string): Promise<ProjectSnapshot> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProjectSnapshot>("inspect_project", { path });
}

export async function openInCode(path: string): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_in_vscode", { path });
}

export async function openTerminal(path: string): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_terminal", { path });
}
