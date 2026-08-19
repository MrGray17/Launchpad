import { useEffect, useMemo, useRef, useState } from "react";
import {
  activateProject as activateProjectInLibrary,
  addProject as addProjectToLibrary,
  backupLibrary,
  bootstrapLibrary,
  chooseProjectFolder,
  isDesktopRuntime,
  openInCode,
  openTerminal,
  refreshProject as refreshProjectInLibrary,
  relinkProject as relinkProjectInLibrary,
  removeProject as removeProjectFromLibrary,
  saveProjectFocus,
  type LegacyProject,
  type LibraryState,
  type MetadataStatus,
  type Project,
} from "./platform/desktop";
import { applyTheme, readTheme, type Theme } from "./theme";

type Mood = "sakura" | "mint" | "sky" | "amber" | "night";
type Operation = "add" | "activate" | "refresh" | "relink" | "remove" | "continue" | "terminal" | "save" | "backup";
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

function moodFromName(name: string): Mood {
  const moods: Mood[] = ["sakura", "mint", "sky", "amber", "night"];
  const score = [...name].reduce((sum, character) => sum + character.charCodeAt(0), 0);
  return moods[score % moods.length];
}

function projectSymbol(name: string) {
  const words = name.trim().split(/[\s_-]+/).filter(Boolean);
  return words.slice(0, 2).map((word) => [...word][0]).join("").toUpperCase() || "✦";
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
  if (!project.available) return "Folder unavailable — relink or remove.";
  if (project.metadataStatus === "timeout") return "Git inspection took too long.";
  if (project.metadataStatus === "git-unavailable") return "Git is not available on PATH.";
  if (project.gitStatus === "clean") return "Ready when you are.";
  if (project.gitStatus === "dirty") return "Work in progress.";
  return "A local world of its own.";
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
  const operationRef = useRef<Operation | null>(null);
  const bootstrapRequestRef = useRef<{
    key: number;
    promise: ReturnType<typeof bootstrapApp>;
  } | null>(null);
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
    if (!focusEditor) return;
    const modal = focusModalRef.current;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeFocusEditor();
        return;
      }
      if (event.key !== "Tab" || !modal) return;
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
        setToast(`${project.name} is ready.`);
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
    await runExclusive("refresh", async () => {
      try {
        const refreshed = await refreshProjectInLibrary(project.id);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, refreshed),
        } : current);
        setToast(`${refreshed.name} is up to date.`);
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
    await runExclusive("relink", async () => {
      try {
        const path = await chooseProjectFolder();
        if (!path) return;
        const relinked = await relinkProjectInLibrary(project.id, path);
        setLibrary((current) => current ? {
          ...current,
          projects: replaceProject(current.projects, relinked),
        } : current);
        setToast(`${relinked.name} has been relinked.`);
      } catch (error) {
        setToast(errorMessage(error, "Could not relink that project."));
      }
    });
  }

  async function removeProject(project: Project) {
    if (!window.confirm(`Remove ${project.name} from Launchpad? Its folder will not be deleted.`)) return;
    await runExclusive("remove", async () => {
      try {
        const nextLibrary = await removeProjectFromLibrary(project.id);
        setLibrary(nextLibrary);
        if (focusEditor?.projectId === project.id) closeFocusEditor();
        setToast(`${project.name} was removed from Launchpad. Its files are untouched.`);
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
    setFocusEditor({
      projectId: active.id,
      quest: active.quest,
      checkpoint: active.checkpoint,
    });
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
        setToast("Focus saved for Future You.");
      } catch (error) {
        setToast(errorMessage(error, "Could not save that focus."));
      }
    });
  }

  async function createBackup() {
    await runExclusive("backup", async () => {
      try {
        const backup = await backupLibrary();
        setToast(`Backup created: ${backup.fileName}`);
      } catch (error) {
        setToast(errorMessage(error, "Could not create a backup."));
      }
    });
  }

  const header = (
    <header className="topbar">
      <div className="brand" aria-label="Launchpad">✦ <span>Launchpad</span></div>
      <span className="tiny-copy">your little collection of worlds</span>
      <div className="topbar-tools">
        <button
          className="theme-toggle"
          type="button"
          aria-label={`Switch to ${theme === "light" ? "dark grey" : "light"} theme`}
          aria-pressed={theme === "dark"}
          onClick={() => setTheme((current) => current === "light" ? "dark" : "light")}
        >
          <span aria-hidden="true">{theme === "light" ? "◐" : "☼"}</span>
          {theme === "light" ? "Dark" : "Light"}
        </button>
        <div className="window-dots" aria-hidden="true"><i /><i /><i /></div>
      </div>
    </header>
  );

  if (!library && !loadError) {
    return <div className="app-shell">{header}<main className="state-page"><div className="state-card" role="status"><span className="state-glyph">✦</span><h1>Opening your library…</h1><p>Refreshing the project you last used.</p></div></main></div>;
  }

  if (loadError) {
    return <div className="app-shell">{header}<main className="state-page"><div className="state-card error-card" role="alert"><span className="state-glyph">!</span><h1>Your library could not open.</h1><p>{loadError}</p><button type="button" onClick={() => { setLibrary(null); setReloadKey((key) => key + 1); }}>Try again</button></div></main></div>;
  }

  if (!active) {
    return (
      <div className="app-shell">
        {header}
        <main className="empty-page">
          <section className="welcome"><div><span className="eyebrow">{dateLabel.toUpperCase()}</span><h1>Good {greeting} <span>🌱</span></h1><p>Your collection is ready for its first real project.</p></div></section>
          <section className="empty-library">
            <span className="empty-mark">＋</span><span className="eyebrow">START YOUR COLLECTION</span>
            <h2>Add a project you already care about.</h2>
            <p>Launchpad will inspect its repository and remember your focus locally.</p>
            <button type="button" onClick={addProject} disabled={isBusy}>{operation === "add" ? "Opening folders…" : "Choose a project folder"}</button>
          </section>
        </main>
        <div className="page-footer"><span>Local first. No streaks. No guilt.</span><span>Launchpad v0.1 · Windows-first</span></div>
        {toast && <div className="toast" role="status">{toast}</div>}
      </div>
    );
  }

  const cleanCount = library!.projects.filter((project) => project.gitStatus === "clean").length;

  return (
    <div className="app-shell">
      {header}
      <main>
        <section className="welcome">
          <div><span className="eyebrow">{dateLabel.toUpperCase()}</span><h1>Good {greeting} <span>🌸</span></h1><p>{active.name} is right where you left it.</p></div>
          <div className="local-note"><b>⌂</b><span>local library<br />on this device</span></div>
        </section>

        <section className="focus-panel">
          <div className={`focus-art mood-${moodFromName(active.name)}`}>
            <span className="focus-symbol">{projectSymbol(active.name)}</span>
            <div><strong>{active.name}</strong><small>{projectTagline(active)}</small></div>
          </div>
          <div className="focus-copy">
            <div className="git-line" title={`${active.branch} · ${metadataLabel(active.metadataStatus)}`}><i className={active.gitStatus} /><span className="branch-name">{active.branch}</span><span>·</span>{metadataLabel(active.metadataStatus)}</div>
            <span className="eyebrow">CURRENT QUEST ✨</span><h2>{active.quest}</h2><p className="checkpoint">“{active.checkpoint}”</p>

            {active.available ? (
              <div className="focus-actions">
                <button className="continue" type="button" onClick={continueProject} disabled={isBusy}>Continue {active.name}<span>{operation === "continue" ? "…" : "→"}</span></button>
                <button className="terminal" type="button" onClick={launchTerminal} disabled={isBusy} aria-label="Open terminal">{operation === "terminal" ? "…" : ">_"}</button>
              </div>
            ) : (
              <div className="broken-actions" role="alert"><span>Launchpad cannot find this project folder.</span><button type="button" onClick={() => relinkProject(active)} disabled={isBusy}>Relink folder</button><button type="button" onClick={() => removeProject(active)} disabled={isBusy}>Remove</button></div>
            )}

            <div className="meta-row">
              <div><small>SCRIPTS</small><strong title={active.scripts.join(" · ")}>{active.scripts.length ? active.scripts.join(" · ") : "none detected"}</strong></div>
              <div><small>METADATA</small><strong>{relativeDate(active.metadataRefreshedAt, now)}</strong></div>
              <button ref={editFocusButtonRef} type="button" onClick={openFocusEditor} disabled={isBusy}>Edit focus 🌱</button>
            </div>
            <div className="project-tools">
              <button type="button" onClick={() => refreshProject(active)} disabled={isBusy || !active.available}>Refresh</button>
              <button type="button" onClick={() => relinkProject(active)} disabled={isBusy}>Relink</button>
              <button type="button" onClick={() => removeProject(active)} disabled={isBusy}>Remove</button>
            </div>
          </div>
        </section>

        <section className="collection">
          <div className="section-head">
            <div><span className="eyebrow">MY COLLECTION</span><h3>Little worlds, still growing.</h3></div>
            <div className="library-actions"><button type="button" onClick={createBackup} disabled={isBusy}>Back up</button><button type="button" onClick={addProject} disabled={isBusy}>Add project <span>＋</span></button></div>
          </div>
          <div className="shelf">
            {library!.projects.map((project) => (
              <div className="project-cover-wrap" key={project.id}>
                <button
                  type="button"
                  className={`project-cover mood-${moodFromName(project.name)} ${project.id === active.id ? "active" : ""} ${!project.available ? "unavailable" : ""}`}
                  onClick={() => void activateProject(project)}
                  aria-pressed={project.id === active.id}
                  disabled={isBusy || !project.available}
                >
                  <span className="cover-symbol">{projectSymbol(project.name)}</span>
                  <div><strong title={project.name}>{project.name}</strong><small>{projectTagline(project)}</small></div>
                  <footer><span>{relativeDate(project.lastOpenedAt, now)}</span><span>{project.available ? metadataLabel(project.metadataStatus) : "missing"}</span></footer>
                </button>
                {!project.available && <div className="cover-recovery"><button type="button" onClick={() => relinkProject(project)} disabled={isBusy}>Relink</button><button type="button" onClick={() => removeProject(project)} disabled={isBusy}>Remove</button></div>}
              </div>
            ))}
            <button className="project-cover add-cover" type="button" onClick={addProject} disabled={isBusy}><span className="plus">＋</span><div><strong>Add a local project</strong><small>Choose a folder and let Launchpad inspect it.</small></div></button>
          </div>
          <div className="shelf-edge" />
        </section>

        <section className="today" aria-label="Library summary">
          <div><span className="eyebrow">YOUR LIBRARY</span><strong>A quiet place to pick up the thread.</strong></div>
          <div><small>PROJECTS</small><strong>{library!.projects.length}</strong><span>stored locally</span></div>
          <div><small>READY</small><strong>{cleanCount}</strong><span>clean working trees</span></div>
          <div className="cat">₍^. .^₎⟆ <span>no scoring, just context</span></div>
        </section>
      </main>

      <div className="page-footer"><span>Local first. No streaks. No guilt.</span><span>Launchpad v0.1 · Windows-first</span></div>

      {focusEditor && (
        <div className="backdrop" onMouseDown={closeFocusEditor}>
          <section ref={focusModalRef} className="checkpoint-modal" role="dialog" aria-modal="true" aria-labelledby="focus-title" onMouseDown={(event) => event.stopPropagation()}>
            <span className="pin">✦</span><span className="eyebrow">LEAVE A TRAIL 🌱</span><h2 id="focus-title">Where should Future You continue?</h2>
            <p>A concrete quest and one useful checkpoint make the next five minutes easy.</p>
            <label>Current quest<input autoFocus maxLength={120} value={focusEditor.quest} onChange={(event) => setFocusEditor({ ...focusEditor, quest: event.target.value })} /></label>
            <label>Checkpoint<textarea maxLength={180} value={focusEditor.checkpoint} onChange={(event) => setFocusEditor({ ...focusEditor, checkpoint: event.target.value })} /></label>
            <div><button type="button" onClick={closeFocusEditor}>Not now</button><button className="save" type="button" onClick={saveFocus} disabled={isBusy}>{operation === "save" ? "Saving…" : "Save focus ✨"}</button></div>
          </section>
        </div>
      )}
      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
