import { useEffect, useMemo, useState } from "react";
import {
  addProject as addProjectToLibrary,
  chooseProjectFolder,
  importLegacyProjects,
  isDesktopRuntime,
  loadLibrary,
  openInCode,
  openTerminal,
  refreshProject,
  saveProjectFocus,
  selectProject,
  type LegacyProject,
  type LibraryState,
  type Project,
} from "./platform/desktop";

type Mood = "sakura" | "mint" | "sky" | "amber" | "night";
type BusyAction = "add" | "continue" | "terminal" | "save" | null;

const LEGACY_PROJECTS_KEY = "launchpad.projects.v1";
const LEGACY_ACTIVE_KEY = "launchpad.active-project.v1";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : fallback;
}

function readLegacyProjects(): LegacyProject[] {
  try {
    const saved = localStorage.getItem(LEGACY_PROJECTS_KEY);
    if (!saved) return [];
    const parsed: unknown = JSON.parse(saved);
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((candidate) => {
      if (!candidate || typeof candidate !== "object") return [];
      const project = candidate as Record<string, unknown>;
      if (typeof project.path !== "string" || !project.path.trim()) return [];
      return [{
        path: project.path,
        quest: typeof project.quest === "string" ? project.quest : undefined,
        checkpoint: typeof project.checkpoint === "string" ? project.checkpoint : undefined,
      }];
    });
  } catch {
    return [];
  }
}

function clearLegacyStorage() {
  try {
    localStorage.removeItem(LEGACY_PROJECTS_KEY);
    localStorage.removeItem(LEGACY_ACTIVE_KEY);
  } catch {
    // SQLite is authoritative; inaccessible legacy storage is safe to ignore.
  }
}

async function bootstrapLibrary() {
  let library = await loadLibrary();
  if (!isDesktopRuntime()) return library;

  const legacyProjects = readLegacyProjects();
  if (legacyProjects.length) {
    library = await importLegacyProjects(legacyProjects);
  }
  clearLegacyStorage();
  return library;
}

function moodFromName(name: string): Mood {
  const moods: Mood[] = ["sakura", "mint", "sky", "amber", "night"];
  const score = [...name].reduce((sum, character) => sum + character.charCodeAt(0), 0);
  return moods[score % moods.length];
}

function projectSymbol(name: string) {
  const words = name.trim().split(/[\s_-]+/).filter(Boolean);
  return words.slice(0, 2).map((word) => word[0]).join("").toUpperCase() || "✦";
}

function projectTagline(project: Project) {
  if (project.gitStatus === "clean") return "Ready when you are.";
  if (project.gitStatus === "dirty") return "Work in progress.";
  return "A local world of its own.";
}

function relativeDate(value: string | null) {
  if (!value) return "not opened yet";
  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) return "recently";
  const elapsedMinutes = Math.max(0, Math.round((Date.now() - timestamp) / 60_000));
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
  const [focusOpen, setFocusOpen] = useState(false);
  const [questDraft, setQuestDraft] = useState("");
  const [checkpointDraft, setCheckpointDraft] = useState("");
  const [busyAction, setBusyAction] = useState<BusyAction>(null);
  const [toast, setToast] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    bootstrapLibrary()
      .then((nextLibrary) => {
        if (!cancelled) setLibrary(nextLibrary);
      })
      .catch((error) => {
        if (!cancelled) setLoadError(errorMessage(error, "Launchpad could not load your library."));
      });
    return () => { cancelled = true; };
  }, [reloadKey]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 2800);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (!focusOpen) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setFocusOpen(false);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [focusOpen]);

  const active = useMemo(() => {
    if (!library?.projects.length) return null;
    return library.projects.find((project) => project.id === library.activeProjectId)
      ?? library.projects[0];
  }, [library]);

  const now = new Date();
  const greeting = daypart(now);
  const dateLabel = new Intl.DateTimeFormat(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
  }).format(now);

  async function addProject() {
    setBusyAction("add");
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
    } finally {
      setBusyAction(null);
    }
  }

  async function activateProject(project: Project) {
    if (!library || project.id === active?.id) return;
    const previousId = library.activeProjectId;
    setLibrary({ ...library, activeProjectId: project.id });
    try {
      await selectProject(project.id);
      const refreshed = await refreshProject(project.id);
      setLibrary((current) => current ? {
        ...current,
        projects: replaceProject(current.projects, refreshed),
      } : current);
    } catch (error) {
      setLibrary((current) => current ? { ...current, activeProjectId: previousId } : current);
      setToast(errorMessage(error, "Could not select that project."));
    }
  }

  async function continueProject() {
    if (!active) return;
    setBusyAction("continue");
    try {
      const updated = await openInCode(active.id);
      setLibrary((current) => current ? {
        ...current,
        projects: replaceProject(current.projects, updated),
      } : current);
    } catch (error) {
      setToast(errorMessage(error, "Could not open VS Code."));
    } finally {
      setBusyAction(null);
    }
  }

  async function launchTerminal() {
    if (!active) return;
    setBusyAction("terminal");
    try {
      await openTerminal(active.id);
    } catch (error) {
      setToast(errorMessage(error, "Could not open the terminal."));
    } finally {
      setBusyAction(null);
    }
  }

  function openFocusEditor() {
    if (!active) return;
    setQuestDraft(active.quest);
    setCheckpointDraft(active.checkpoint);
    setFocusOpen(true);
  }

  async function saveFocus() {
    if (!active) return;
    setBusyAction("save");
    try {
      const updated = await saveProjectFocus(active.id, questDraft, checkpointDraft);
      setLibrary((current) => current ? {
        ...current,
        projects: replaceProject(current.projects, updated),
      } : current);
      setFocusOpen(false);
      setToast("Focus saved for Future You.");
    } catch (error) {
      setToast(errorMessage(error, "Could not save that focus."));
    } finally {
      setBusyAction(null);
    }
  }

  const header = (
    <header className="topbar">
      <div className="brand" aria-label="Launchpad">✦ <span>Launchpad</span></div>
      <span className="tiny-copy">your little collection of worlds</span>
      <div className="window-dots" aria-hidden="true"><i /><i /><i /></div>
    </header>
  );

  if (!library && !loadError) {
    return <div className="app-shell">{header}<main className="state-page"><div className="state-card" role="status"><span className="state-glyph">✦</span><h1>Opening your library…</h1><p>Everything stays on this device.</p></div></main></div>;
  }

  if (loadError) {
    return <div className="app-shell">{header}<main className="state-page"><div className="state-card error-card" role="alert"><span className="state-glyph">!</span><h1>Your library could not open.</h1><p>{loadError}</p><button type="button" onClick={() => { setLibrary(null); setReloadKey((key) => key + 1); }}>Try again</button></div></main></div>;
  }

  if (!active) {
    return (
      <div className="app-shell">
        {header}
        <main className="empty-page">
          <section className="welcome">
            <div><span className="eyebrow">{dateLabel.toUpperCase()}</span><h1>Good {greeting} <span>🌱</span></h1><p>Your collection is ready for its first real project.</p></div>
          </section>
          <section className="empty-library">
            <span className="empty-mark">＋</span>
            <span className="eyebrow">START YOUR COLLECTION</span>
            <h2>Add a project you already care about.</h2>
            <p>Launchpad will inspect its Git state and package scripts, then remember your quest and checkpoint locally.</p>
            <button type="button" onClick={addProject} disabled={busyAction === "add"}>{busyAction === "add" ? "Opening folders…" : "Choose a project folder"}</button>
          </section>
        </main>
        <div className="page-footer"><span>Local first. No streaks. No guilt.</span><span>Launchpad v0.1 · first light</span></div>
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
          <div>
            <span className="eyebrow">{dateLabel.toUpperCase()}</span>
            <h1>Good {greeting} <span>🌸</span></h1>
            <p>{active.name} is right where you left it.</p>
          </div>
          <div className="local-note"><b>⌂</b><span>local library<br />on this device</span></div>
        </section>

        <section className="focus-panel">
          <div className={`focus-art mood-${moodFromName(active.name)}`}>
            <span className="focus-symbol">{projectSymbol(active.name)}</span>
            <div><strong>{active.name}</strong><small>{projectTagline(active)}</small></div>
          </div>

          <div className="focus-copy">
            <div className="git-line"><i className={active.gitStatus} />{active.branch}<span>·</span>{active.gitStatus}</div>
            <span className="eyebrow">CURRENT QUEST ✨</span>
            <h2>{active.quest}</h2>
            <p className="checkpoint">“{active.checkpoint}”</p>

            <div className="focus-actions">
              <button className="continue" type="button" onClick={continueProject} disabled={busyAction !== null}>Continue {active.name}<span>{busyAction === "continue" ? "…" : "→"}</span></button>
              <button className="terminal" type="button" onClick={launchTerminal} disabled={busyAction !== null} aria-label="Open terminal">{busyAction === "terminal" ? "…" : ">_"}</button>
            </div>

            <div className="meta-row">
              <div><small>SCRIPTS</small><strong>{active.scripts.length ? active.scripts.join(" · ") : "none detected"}</strong></div>
              <div><small>LAST OPENED</small><strong>{relativeDate(active.lastOpenedAt)}</strong></div>
              <button type="button" onClick={openFocusEditor}>Edit focus 🌱</button>
            </div>
          </div>
        </section>

        <section className="collection">
          <div className="section-head">
            <div><span className="eyebrow">MY COLLECTION</span><h3>Little worlds, still growing.</h3></div>
            <button type="button" onClick={addProject} disabled={busyAction === "add"}>Add project <span>＋</span></button>
          </div>

          <div className="shelf">
            {library!.projects.map((project) => (
              <button
                key={project.id}
                type="button"
                className={`project-cover mood-${moodFromName(project.name)} ${project.id === active.id ? "active" : ""}`}
                onClick={() => void activateProject(project)}
                aria-pressed={project.id === active.id}
              >
                <span className="cover-symbol">{projectSymbol(project.name)}</span>
                <div><strong>{project.name}</strong><small>{projectTagline(project)}</small></div>
                <footer><span>{relativeDate(project.lastOpenedAt)}</span><span>{project.gitStatus}</span></footer>
              </button>
            ))}
            <button className="project-cover add-cover" type="button" onClick={addProject} disabled={busyAction === "add"}>
              <span className="plus">＋</span>
              <div><strong>Add a local project</strong><small>Choose a folder and let Launchpad inspect it.</small></div>
            </button>
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

      <div className="page-footer"><span>Local first. No streaks. No guilt.</span><span>Launchpad v0.1 · first light 🌱</span></div>

      {focusOpen && (
        <div className="backdrop" onMouseDown={() => setFocusOpen(false)}>
          <section className="checkpoint-modal" role="dialog" aria-modal="true" aria-labelledby="focus-title" onMouseDown={(event) => event.stopPropagation()}>
            <span className="pin">✦</span>
            <span className="eyebrow">LEAVE A TRAIL 🌱</span>
            <h2 id="focus-title">Where should Future You continue?</h2>
            <p>A concrete quest and one useful checkpoint make the next five minutes easy.</p>
            <label>Current quest<input autoFocus maxLength={120} value={questDraft} onChange={(event) => setQuestDraft(event.target.value)} /></label>
            <label>Checkpoint<textarea maxLength={180} value={checkpointDraft} onChange={(event) => setCheckpointDraft(event.target.value)} /></label>
            <div><button type="button" onClick={() => setFocusOpen(false)}>Not now</button><button className="save" type="button" onClick={saveFocus} disabled={busyAction === "save"}>{busyAction === "save" ? "Saving…" : "Save focus ✨"}</button></div>
          </section>
        </div>
      )}

      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
