import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  activateProject as activateProjectInLibrary,
  addProject as addProjectToLibrary,
  backupLibrary,
  bootstrapLibrary,
  chooseBackupDestination,
  chooseBackupFile,
  chooseProjectFolder,
  exportLibrary,
  isDesktopRuntime,
  openInCode,
  openTerminal,
  refreshProject as refreshProjectInLibrary,
  relinkProject as relinkProjectInLibrary,
  removeProject as removeProjectFromLibrary,
  restoreLibrary,
  saveProjectFocus,
  type LegacyProject,
  type LibraryState,
  type MetadataStatus,
  type Project,
} from "./platform/desktop";
import { applyTheme, readTheme, type Theme } from "./theme";

type Mood = "sakura" | "mint" | "sky" | "amber" | "night";
type Motif = "signal" | "window" | "queue" | "ledger" | "spark" | "orbit";
type Operation =
  | "add"
  | "activate"
  | "refresh"
  | "relink"
  | "remove"
  | "continue"
  | "terminal"
  | "save"
  | "backup"
  | "export"
  | "restore";
type FocusEditor = { projectId: number; quest: string; checkpoint: string };

const LEGACY_PROJECTS_KEY = "launchpad.projects.v1";
const LEGACY_ACTIVE_KEY = "launchpad.active-project.v1";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : fallback;
}

function legacyId(project: Record<string, unknown>, path: string) {
  return typeof project.id === "string" && project.id ? project.id : `path:${path}`;
}

function readLegacyState() {
  const saved = localStorage.getItem(LEGACY_PROJECTS_KEY);
  if (!saved) return { projects: [] as LegacyProject[], raw: [] as unknown[], activeId: null as string | null };
  let raw: unknown;
  try {
    raw = JSON.parse(saved);
  } catch {
    throw new Error("Launchpad found unreadable prototype data and left it untouched for recovery.");
  }
  if (!Array.isArray(raw)) {
    throw new Error("Launchpad found malformed prototype data and left it untouched for recovery.");
  }
  const projects = raw.map((candidate) => {
    if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) {
      throw new Error("Launchpad found malformed prototype data and left it untouched for recovery.");
    }
    const project = candidate as Record<string, unknown>;
    if (
      typeof project.path !== "string"
      || !project.path.trim()
      || (project.id !== undefined && (typeof project.id !== "string" || !project.id))
      || (project.quest !== undefined && typeof project.quest !== "string")
      || (project.checkpoint !== undefined && typeof project.checkpoint !== "string")
    ) {
      throw new Error("Launchpad found malformed prototype data and left it untouched for recovery.");
    }
    return {
      legacyId: legacyId(project, project.path),
      path: project.path,
      quest: typeof project.quest === "string" ? project.quest : undefined,
      checkpoint: typeof project.checkpoint === "string" ? project.checkpoint : undefined,
    };
  });
  return {
    projects,
    raw,
    activeId: localStorage.getItem(LEGACY_ACTIVE_KEY),
  };
}

function reconcileLegacyStorage(
  raw: unknown[],
  pendingIds: string[],
  activeId: string | null,
  complete: boolean,
) {
  if (complete) {
    localStorage.removeItem(LEGACY_PROJECTS_KEY);
    localStorage.removeItem(LEGACY_ACTIVE_KEY);
    return;
  }
  const pending = new Set(pendingIds);
  const retained = raw.filter((candidate) => {
    if (!candidate || typeof candidate !== "object") return false;
    const project = candidate as Record<string, unknown>;
    if (typeof project.path !== "string") return false;
    return pending.has(legacyId(project, project.path));
  });
  localStorage.setItem(LEGACY_PROJECTS_KEY, JSON.stringify(retained));
  if (!activeId || !pending.has(activeId)) localStorage.removeItem(LEGACY_ACTIVE_KEY);
}

async function bootstrapApp() {
  if (!isDesktopRuntime()) return bootstrapLibrary([], null);
  const legacy = readLegacyState();
  const state = await bootstrapLibrary(legacy.projects, legacy.activeId);
  reconcileLegacyStorage(
    legacy.raw,
    state.pendingLegacyIds,
    legacy.activeId,
    state.legacyMigrationComplete,
  );
  return state;
}

function hashName(name: string) {
  return [...name].reduce((hash, character) => ((hash * 31) + character.codePointAt(0)!) >>> 0, 17);
}

function moodFromName(name: string): Mood {
  const moods: Mood[] = ["sakura", "mint", "sky", "amber", "night"];
  return moods[hashName(name) % moods.length];
}

function motifFromName(name: string): Motif {
  const normalized = name.toLowerCase();
  if (normalized.includes("rate") || normalized.includes("limit")) return "signal";
  if (normalized.includes("nest") || normalized.includes("garden")) return "window";
  if (normalized.includes("maw") || normalized.includes("queue") || normalized.includes("clinic")) return "queue";
  if (normalized.includes("sifr") || normalized.includes("ledger") || normalized.includes("finance")) return "ledger";
  if (normalized.includes("launchpad")) return "spark";
  const motifs: Motif[] = ["orbit", "signal", "window", "ledger"];
  return motifs[hashName(name) % motifs.length];
}

function projectSymbol(name: string) {
  const words = name.trim().split(/[\s_-]+/).filter(Boolean);
  return words.slice(0, 2).map((word) => [...word][0]).join("").toUpperCase() || "✦";
}

function projectVisualStyle(name: string): CSSProperties {
  const hash = hashName(name);
  return {
    "--seed-a": `${18 + (hash % 65)}%`,
    "--seed-b": `${16 + ((hash >> 5) % 68)}%`,
    "--tilt": `${((hash >> 11) % 9) - 4}deg`,
  } as CSSProperties;
}

function BrandMark() {
  return (
    <svg className="brand-mark" viewBox="0 0 1024 1024" aria-hidden="true">
      <circle cx="512" cy="512" r="332" />
      <path d="M512 244c24 150 118 244 268 268-150 24-244 118-268 268-24-150-118-244-268-268 150-24 244-118 268-268Z" />
      <circle className="brand-mark-dot" cx="744" cy="292" r="58" />
    </svg>
  );
}

function ProjectArtwork({ project, compact = false }: { project: Project; compact?: boolean }) {
  const motif = motifFromName(project.name);
  return (
    <div
      className={`project-art mood-${moodFromName(project.name)} motif-${motif} ${compact ? "compact" : ""}`}
      style={projectVisualStyle(project.name)}
      data-motif={motif}
      aria-hidden="true"
    >
      <span className="art-symbol">{projectSymbol(project.name)}</span>
      <span className="art-shape art-shape-a" />
      <span className="art-shape art-shape-b" />
      <span className="art-shape art-shape-c" />
      <span className="art-grid" />
    </div>
  );
}

function metadataLabel(status: MetadataStatus) {
  const labels: Record<MetadataStatus, string> = {
    fresh: "fresh",
    unknown: "unknown",
    "not-a-repository": "not a Git repo",
    "git-unavailable": "Git unavailable",
    "invalid-repository": "Git error",
    timeout: "Git timed out",
  };
  return labels[status];
}

function projectTagline(project: Project) {
  if (!project.available) return "Folder unavailable";
  if (project.metadataStatus === "timeout") return "Git inspection timed out";
  if (project.metadataStatus === "git-unavailable") return "Git unavailable";
  if (project.gitStatus === "clean") return "Ready when you are";
  if (project.gitStatus === "dirty") return "Work in progress";
  return "Local project";
}

function relativeDate(value: string | null, now: Date) {
  if (!value) return "not yet";
  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) return "recently";
  const elapsedMinutes = Math.max(0, Math.round((now.getTime() - timestamp) / 60_000));
  if (elapsedMinutes < 2) return "just now";
  if (elapsedMinutes < 60) return `${elapsedMinutes}m ago`;
  const hours = Math.round(elapsedMinutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

function replaceProject(projects: Project[], updated: Project) {
  return projects.map((project) => project.id === updated.id ? updated : project);
}

function daypart(date: Date) {
  const hour = date.getHours();
  if (hour < 12) return "morning";
  if (hour < 18) return "afternoon";
  return "evening";
}

export default function App() {
  const [library, setLibrary] = useState<LibraryState | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [focusEditor, setFocusEditor] = useState<FocusEditor | null>(null);
  const [operation, setOperation] = useState<Operation | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [now, setNow] = useState(() => new Date());
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [appMenuOpen, setAppMenuOpen] = useState(false);
  const [projectMenuOpen, setProjectMenuOpen] = useState(false);
  const operationRef = useRef<Operation | null>(null);
  const bootstrapRequestRef = useRef<{ key: number; promise: ReturnType<typeof bootstrapApp> } | null>(null);
  const focusModalRef = useRef<HTMLElement>(null);
  const editFocusButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    if (bootstrapRequestRef.current?.key !== reloadKey) {
      bootstrapRequestRef.current = { key: reloadKey, promise: bootstrapApp() };
    }
    bootstrapRequestRef.current.promise
      .then((state) => {
        if (!cancelled) {
          setLibrary({ projects: state.projects, activeProjectId: state.activeProjectId });
          if (state.pendingLegacyIds.length) {
            setToast(`${state.pendingLegacyIds.length} prototype project${state.pendingLegacyIds.length === 1 ? " is" : "s are"} waiting for its folder to return.`);
          }
        }
      })
      .catch((error) => {
        if (!cancelled) setLoadError(errorMessage(error, "Launchpad could not load your library."));
      });
    return () => { cancelled = true; };
  }, [reloadKey]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => applyTheme(theme), [theme]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  function closeFocusEditor() {
    setFocusEditor(null);
    window.setTimeout(() => editFocusButtonRef.current?.focus(), 0);
  }

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (focusEditor) closeFocusEditor();
        setAppMenuOpen(false);
        setProjectMenuOpen(false);
        return;
      }
      if (event.key !== "Tab" || !focusEditor) return;
      const modal = focusModalRef.current;
      if (!modal) return;
      const focusable = [...modal.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
      )];
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [focusEditor]);

  const active = useMemo(() => {
    if (!library?.projects.length) return null;
    return library.projects.find((project) => project.id === library.activeProjectId)
      ?? library.projects[0];
  }, [library]);

  const isBusy = operation !== null;
  const greeting = daypart(now);
  const dateLabel = new Intl.DateTimeFormat(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
  }).format(now);

  async function runExclusive(kind: Operation, task: () => Promise<void>) {
    if (operationRef.current) return;
    operationRef.current = kind;
    setOperation(kind);
    try {
      await task();
    } finally {
      operationRef.current = null;
      setOperation(null);
    }
  }

  async function addProject() {
    await runExclusive("add", async () => {
      try {
        const path = await chooseProjectFolder();
        if (!path) {
          if (!isDesktopRuntime()) setToast("Open Launchpad as a desktop app to add a folder.");
          return;
        }
        const project = await addProjectToLibrary(path);
        setLibrary((current) => current ? {
          projects: [project, ...current.projects.filter((item) => item.id !== project.id)],
          activeProjectId: project.id,
        } : { projects: [project], activeProjectId: project.id });
        setToast(`${project.name} added.`);
      } catch (error) {
        setToast(errorMessage(error, "Could not add that project."));
      }
    });
  }

  async function activateProject(project: Project) {
    if (!library || project.id === active?.id || !project.available) return;
    await runExclusive("activate", async () => {
      try {
        const refreshed = await activateProjectInLibrary(project.id);
        setLibrary((current) => current ? {
          ...current,
          activeProjectId: refreshed.id,
          projects: replaceProject(current.projects, refreshed),
        } : current);
      } catch (error) {
        setToast(errorMessage(error, "Could not select that project."));
      }
    });
  }

  async function refreshProject(project: Project) {
    setProjectMenuOpen(false);
    await runExclusive("refresh", async () => {
      try {
        const refreshed = await refreshProjectInLibrary(project.id);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, refreshed),
        } : current);
        setToast(`${refreshed.name} refreshed.`);
      } catch (error) {
        if (errorMessage(error, "").includes("does not exist")) {
          setLibrary((current) => current ? {
            ...current,
            projects: current.projects.map((item) => item.id === project.id ? { ...item, available: false } : item),
          } : current);
        }
        setToast(errorMessage(error, "Could not refresh that project."));
      }
    });
  }

  async function relinkProject(project: Project) {
    setProjectMenuOpen(false);
    await runExclusive("relink", async () => {
      try {
        const path = await chooseProjectFolder();
        if (!path) return;
        const relinked = await relinkProjectInLibrary(project.id, path);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, relinked),
        } : current);
        setToast(`${relinked.name} relinked.`);
      } catch (error) {
        setToast(errorMessage(error, "Could not relink that project."));
      }
    });
  }

  async function removeProject(project: Project) {
    setProjectMenuOpen(false);
    if (!window.confirm(`Remove ${project.name} from Launchpad? Its folder and source files will stay untouched.`)) return;
    await runExclusive("remove", async () => {
      try {
        const nextLibrary = await removeProjectFromLibrary(project.id);
        setLibrary(nextLibrary);
        if (focusEditor?.projectId === project.id) closeFocusEditor();
        setToast(`${project.name} removed from Launchpad.`);
      } catch (error) {
        setToast(errorMessage(error, "Could not remove that project."));
      }
    });
  }

  async function continueProject() {
    if (!active) return;
    await runExclusive("continue", async () => {
      try {
        const updated = await openInCode(active.id);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, updated),
        } : current);
      } catch (error) {
        setToast(errorMessage(error, "Could not open VS Code."));
      }
    });
  }

  async function launchTerminal() {
    if (!active) return;
    await runExclusive("terminal", async () => {
      try {
        const updated = await openTerminal(active.id);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, updated),
        } : current);
      } catch (error) {
        setToast(errorMessage(error, "Could not open the terminal."));
      }
    });
  }

  function openFocusEditor() {
    if (!active || isBusy) return;
    setFocusEditor({ projectId: active.id, quest: active.quest, checkpoint: active.checkpoint });
  }

  async function saveFocus() {
    if (!focusEditor) return;
    const draft = focusEditor;
    await runExclusive("save", async () => {
      try {
        const updated = await saveProjectFocus(draft.projectId, draft.quest, draft.checkpoint);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, updated),
        } : current);
        closeFocusEditor();
        setToast("Focus saved.");
      } catch (error) {
        setToast(errorMessage(error, "Could not save that focus."));
      }
    });
  }

  async function createBackup() {
    setAppMenuOpen(false);
    await runExclusive("backup", async () => {
      try {
        const backup = await backupLibrary();
        setToast(`Backup created: ${backup.fileName}`);
      } catch (error) {
        setToast(errorMessage(error, "Could not create a backup."));
      }
    });
  }

  async function exportBackup() {
    setAppMenuOpen(false);
    await runExclusive("export", async () => {
      try {
        const path = await chooseBackupDestination();
        if (!path) return;
        const backup = await exportLibrary(path);
        setToast(`Backup exported: ${backup.fileName}`);
      } catch (error) {
        setToast(errorMessage(error, "Could not export the backup."));
      }
    });
  }

  async function restoreBackup() {
    setAppMenuOpen(false);
    await runExclusive("restore", async () => {
      try {
        const path = await chooseBackupFile();
        if (!path) return;
        if (!window.confirm("Restore this Launchpad backup? A safety backup of the current library will be created first.")) return;
        const restored = await restoreLibrary(path);
        setLibrary(restored);
        setFocusEditor(null);
        setProjectMenuOpen(false);
        setToast("Library restored safely.");
      } catch (error) {
        setToast(errorMessage(error, "Could not restore that backup."));
      }
    });
  }

  const header = (
    <header className="topbar">
      <div className="brand" aria-label="Launchpad"><BrandMark /><span>Launchpad</span></div>
      <div className="app-menu-shell">
        <button
          className="menu-trigger"
          type="button"
          aria-label="App menu"
          aria-expanded={appMenuOpen}
          onClick={() => setAppMenuOpen((open) => !open)}
        >
          •••
        </button>
        {appMenuOpen && (
          <div className="popover app-menu" role="menu">
            <button type="button" role="menuitem" onClick={() => { setTheme((current) => current === "light" ? "dark" : "light"); setAppMenuOpen(false); }}>
              <span>{theme === "light" ? "Dark appearance" : "Light appearance"}</span><kbd>{theme === "light" ? "◐" : "☼"}</kbd>
            </button>
            <div className="menu-rule" />
            <span className="menu-label">Library</span>
            <button type="button" role="menuitem" onClick={createBackup} disabled={isBusy}>Back up now</button>
            <button type="button" role="menuitem" onClick={exportBackup} disabled={isBusy}>Export backup…</button>
            <button type="button" role="menuitem" onClick={restoreBackup} disabled={isBusy}>Restore backup…</button>
          </div>
        )}
      </div>
    </header>
  );

  if (!library && !loadError) {
    return <div className="app-shell">{header}<main className="state-page"><div className="state-card" role="status"><BrandMark /><h1>Opening Launchpad</h1><p>Refreshing your last project.</p></div></main></div>;
  }

  if (loadError) {
    return <div className="app-shell">{header}<main className="state-page"><div className="state-card error-card" role="alert"><span className="state-glyph">!</span><h1>Launchpad could not open.</h1><p>{loadError}</p><button type="button" onClick={() => { setLibrary(null); setReloadKey((key) => key + 1); }}>Try again</button></div></main></div>;
  }

  if (!active) {
    return (
      <div className="app-shell">
        {header}
        <main className="empty-page">
          <section className="welcome"><span className="eyebrow">{dateLabel.toUpperCase()}</span><h1>Good {greeting} <span>🌱</span></h1></section>
          <section className="empty-library">
            <div className="empty-mark"><BrandMark /></div>
            <h2>Add your first project.</h2>
            <p>Choose a local folder. Launchpad will remember the project and where you left off.</p>
            <button type="button" onClick={addProject} disabled={isBusy}>{operation === "add" ? "Opening folders…" : "Choose a project folder"}</button>
          </section>
        </main>
        <div className="page-footer"><span>Local-first</span><span>Windows · v0.1</span></div>
        {toast && <div className="toast" role="status">{toast}</div>}
      </div>
    );
  }

  return (
    <div className="app-shell">
      {header}
      <main>
        <section className="welcome">
          <span className="eyebrow">{dateLabel.toUpperCase()}</span>
          <h1>Good {greeting} <span>🌸</span></h1>
          <p>{active.name} is right where you left it.</p>
        </section>

        <section className="focus-panel">
          <div className="focus-visual">
            <ProjectArtwork project={active} />
            <div className="focus-project-name"><strong>{active.name}</strong><span>{projectTagline(active)}</span></div>
          </div>

          <div className="focus-copy">
            <div className="focus-topline">
              <div className="git-line" title={`${active.branch} · ${metadataLabel(active.metadataStatus)}`}>
                <i className={active.gitStatus} /><span className="branch-name">{active.branch}</span><span>·</span>{metadataLabel(active.metadataStatus)}
              </div>
              <div className="project-menu-shell">
                <button className="menu-trigger small" type="button" aria-label="Project options" aria-expanded={projectMenuOpen} onClick={() => setProjectMenuOpen((open) => !open)}>•••</button>
                {projectMenuOpen && (
                  <div className="popover project-menu" role="menu">
                    <button type="button" role="menuitem" onClick={() => refreshProject(active)} disabled={isBusy || !active.available}>Refresh metadata</button>
                    <button type="button" role="menuitem" onClick={() => relinkProject(active)} disabled={isBusy}>Relink folder…</button>
                    <div className="menu-rule" />
                    <button className="danger" type="button" role="menuitem" onClick={() => removeProject(active)} disabled={isBusy}>Remove from Launchpad</button>
                  </div>
                )}
              </div>
            </div>

            <span className="eyebrow">CURRENT QUEST</span>
            <h2>{active.quest}</h2>
            <p className="checkpoint">“{active.checkpoint}”</p>

            {active.available ? (
              <div className="focus-actions">
                <button className="continue" type="button" onClick={continueProject} disabled={isBusy}>Continue {active.name}<span>{operation === "continue" ? "…" : "→"}</span></button>
                <button className="terminal" type="button" onClick={launchTerminal} disabled={isBusy} aria-label="Open terminal">{operation === "terminal" ? "…" : ">_"}</button>
              </div>
            ) : (
              <div className="broken-actions" role="alert"><span>Launchpad cannot find this folder.</span><button type="button" onClick={() => relinkProject(active)} disabled={isBusy}>Relink folder</button><button type="button" onClick={() => removeProject(active)} disabled={isBusy}>Remove</button></div>
            )}

            <div className="focus-meta">
              <div><small>SCRIPTS</small><strong title={active.scripts.join(" · ")}>{active.scripts.length ? active.scripts.join(" · ") : "none detected"}</strong></div>
              <div><small>UPDATED</small><strong>{relativeDate(active.metadataRefreshedAt, now)}</strong></div>
              <button ref={editFocusButtonRef} type="button" onClick={openFocusEditor} disabled={isBusy}>Edit focus</button>
            </div>
          </div>
        </section>

        <section className="collection">
          <div className="section-head"><div><span className="eyebrow">YOUR WORLDS</span><h3>Projects</h3></div><button className="add-project" type="button" onClick={addProject} disabled={isBusy}>Add project <span>＋</span></button></div>
          <div className="shelf">
            {library!.projects.map((project) => (
              <div className="project-cover-wrap" key={project.id}>
                <button
                  type="button"
                  className={`project-cover ${project.id === active.id ? "active" : ""} ${!project.available ? "unavailable" : ""}`}
                  onClick={() => void activateProject(project)}
                  aria-pressed={project.id === active.id}
                  disabled={isBusy || !project.available}
                >
                  <ProjectArtwork project={project} compact />
                  <div className="cover-copy"><strong title={project.name}>{project.name}</strong><small>{projectTagline(project)}</small></div>
                  <footer><span>{relativeDate(project.lastOpenedAt, now)}</span><span>{project.available ? metadataLabel(project.metadataStatus) : "missing"}</span></footer>
                </button>
                {!project.available && <div className="cover-recovery"><button type="button" onClick={() => relinkProject(project)} disabled={isBusy}>Relink</button><button type="button" onClick={() => removeProject(project)} disabled={isBusy}>Remove</button></div>}
              </div>
            ))}
            <button className="project-cover add-cover" type="button" onClick={addProject} disabled={isBusy}><span className="plus">＋</span><div className="cover-copy"><strong>Add project</strong><small>Choose a local folder</small></div></button>
          </div>
        </section>
      </main>

      <div className="page-footer"><span>Local-first</span><span>Windows · v0.1</span></div>

      {focusEditor && (
        <div className="backdrop" onMouseDown={closeFocusEditor}>
          <section ref={focusModalRef} className="checkpoint-modal" role="dialog" aria-modal="true" aria-labelledby="focus-title" onMouseDown={(event) => event.stopPropagation()}>
            <span className="eyebrow">LEAVE A TRAIL 🌱</span><h2 id="focus-title">Where should Future You continue?</h2>
            <p>One concrete next step is enough.</p>
            <label>Current quest<input autoFocus maxLength={120} value={focusEditor.quest} onChange={(event) => setFocusEditor({ ...focusEditor, quest: event.target.value })} /></label>
            <label>Checkpoint<textarea maxLength={180} value={focusEditor.checkpoint} onChange={(event) => setFocusEditor({ ...focusEditor, checkpoint: event.target.value })} /></label>
            <div><button type="button" onClick={closeFocusEditor}>Cancel</button><button className="save" type="button" onClick={saveFocus} disabled={isBusy}>{operation === "save" ? "Saving…" : "Save focus"}</button></div>
          </section>
        </div>
      )}
      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
