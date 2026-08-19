import { useEffect, useMemo, useState } from "react";
import { chooseProjectFolder, inspectProject, openInCode, openTerminal } from "./platform/desktop";

type Mood = "sakura" | "mint" | "sky" | "amber" | "night";

type Project = {
  id: string;
  name: string;
  tagline: string;
  path?: string;
  branch: string;
  gitStatus: "clean" | "dirty" | "unknown";
  quest: string;
  checkpoint: string;
  lastWorked: string;
  mood: Mood;
  symbol: string;
  scripts: string[];
};

const seedProjects: Project[] = [
  {
    id: "sifr",
    name: "Sifr",
    tagline: "Fault-tolerant financial infrastructure.",
    branch: "feat/ledger-core",
    gitStatus: "clean",
    quest: "Build the account model",
    checkpoint: "Implement account transfer locking.",
    lastWorked: "today",
    mood: "sakura",
    symbol: "零",
    scripts: ["dev", "test", "build"],
  },
  {
    id: "rate-limiter",
    name: "Rate Limiter",
    tagline: "Control the flood.",
    branch: "feat/token-bucket",
    gitStatus: "dirty",
    quest: "Implement token bucket",
    checkpoint: "Compare burst behavior against fixed window.",
    lastWorked: "today",
    mood: "night",
    symbol: "//",
    scripts: ["test", "build"],
  },
  {
    id: "maw3id",
    name: "Maw3id",
    tagline: "A calmer clinic queue.",
    branch: "main",
    gitStatus: "clean",
    quest: "Polish receptionist queue",
    checkpoint: "Doctor status transitions still need tests.",
    lastWorked: "2 days ago",
    mood: "sky",
    symbol: "م",
    scripts: ["dev", "test"],
  },
  {
    id: "nest",
    name: "Nest",
    tagline: "Your quiet place to work.",
    branch: "main",
    gitStatus: "clean",
    quest: "Shape the focus room",
    checkpoint: "Try the rainy-evening music layout next.",
    lastWorked: "3 days ago",
    mood: "mint",
    symbol: "🌿",
    scripts: ["dev", "build"],
  },
];

const PROJECTS_KEY = "launchpad.projects.v1";
const ACTIVE_KEY = "launchpad.active-project.v1";

function loadProjects(): Project[] {
  try {
    const saved = localStorage.getItem(PROJECTS_KEY);
    return saved ? JSON.parse(saved) : seedProjects;
  } catch {
    return seedProjects;
  }
}

function slugify(value: string) {
  return value.toLowerCase().trim().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
}

function moodFromName(name: string): Mood {
  const moods: Mood[] = ["sakura", "mint", "sky", "amber", "night"];
  const score = [...name].reduce((sum, char) => sum + char.charCodeAt(0), 0);
  return moods[score % moods.length];
}

export default function App() {
  const [projects, setProjects] = useState<Project[]>(loadProjects);
  const [activeId, setActiveId] = useState(() => localStorage.getItem(ACTIVE_KEY) ?? loadProjects()[0]?.id ?? "");
  const [checkpointOpen, setCheckpointOpen] = useState(false);
  const [checkpointDraft, setCheckpointDraft] = useState("");
  const [toast, setToast] = useState<string | null>(null);

  const active = useMemo(
    () => projects.find((project) => project.id === activeId) ?? projects[0],
    [projects, activeId],
  );

  useEffect(() => localStorage.setItem(PROJECTS_KEY, JSON.stringify(projects)), [projects]);
  useEffect(() => localStorage.setItem(ACTIVE_KEY, activeId), [activeId]);
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 2600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  async function addProject() {
    try {
      const path = await chooseProjectFolder();
      if (!path) {
        setToast("Folder picking works inside the Tauri desktop app ✨");
        return;
      }

      const snapshot = await inspectProject(path);
      const baseId = slugify(snapshot.name) || "project";
      const id = projects.some((project) => project.id === baseId) ? `${baseId}-${Date.now()}` : baseId;
      const project: Project = {
        id,
        name: snapshot.name,
        tagline: "A new little world.",
        path: snapshot.path,
        branch: snapshot.branch,
        gitStatus: snapshot.gitStatus,
        quest: "Choose the next concrete step",
        checkpoint: "Start by deciding what done looks like.",
        lastWorked: "just added",
        mood: moodFromName(snapshot.name),
        symbol: snapshot.name.slice(0, 2).toUpperCase(),
        scripts: snapshot.scripts,
      };
      setProjects((current) => [...current, project]);
      setActiveId(project.id);
      setToast(`${project.name} joined your collection 🌸`);
    } catch (error) {
      setToast(error instanceof Error ? error.message : "Could not add that project.");
    }
  }

  async function continueProject() {
    if (!active.path) {
      setToast("This sample project is decorative. Add a real local project first.");
      return;
    }
    try {
      await openInCode(active.path);
    } catch (error) {
      setToast(error instanceof Error ? error.message : "Could not open VS Code.");
    }
  }

  async function launchTerminal() {
    if (!active.path) {
      setToast("Add a real local project first.");
      return;
    }
    try {
      await openTerminal(active.path);
    } catch (error) {
      setToast(error instanceof Error ? error.message : "Could not open the terminal.");
    }
  }

  function saveCheckpoint() {
    const value = checkpointDraft.trim();
    if (!value) return;
    setProjects((current) => current.map((project) => project.id === active.id ? { ...project, checkpoint: value } : project));
    setCheckpointOpen(false);
    setToast("Checkpoint saved for Future You 🌱");
  }

  if (!active) return null;

  return (
    <div className="app-shell">
      <header className="topbar">
        <button className="brand" type="button">✦ <span>Launchpad</span></button>
        <span className="tiny-copy">your little collection of worlds</span>
        <div className="window-dots" aria-hidden="true"><i /><i /><i /></div>
      </header>

      <main>
        <section className="welcome">
          <div>
            <span className="eyebrow">WEDNESDAY · AFTERNOON</span>
            <h1>Good afternoon, Yazid <span>🌸</span></h1>
            <p>{active.name} is right where you left it.</p>
          </div>
          <div className="weather-note"><b>☀</b><span>soft light<br />open window</span></div>
        </section>

        <section className="focus-panel">
          <div className={`focus-art mood-${active.mood}`}>
            <span className="focus-symbol">{active.symbol}</span>
            <div><strong>{active.name}</strong><small>{active.tagline}</small></div>
          </div>

          <div className="focus-copy">
            <div className="git-line"><i className={active.gitStatus} />{active.branch}<span>·</span>{active.gitStatus}</div>
            <span className="eyebrow">CURRENT QUEST ✨</span>
            <h2>{active.quest}</h2>
            <p className="checkpoint">“{active.checkpoint}”</p>

            <div className="focus-actions">
              <button className="continue" type="button" onClick={continueProject}>Continue {active.name}<span>→</span></button>
              <button className="terminal" type="button" onClick={launchTerminal} aria-label="Open terminal">&gt;_</button>
            </div>

            <div className="meta-row">
              <div><small>SCRIPTS</small><strong>{active.scripts.length ? active.scripts.join(" · ") : "none detected"}</strong></div>
              <div><small>LAST WORKED</small><strong>{active.lastWorked}</strong></div>
              <button type="button" onClick={() => { setCheckpointDraft(active.checkpoint); setCheckpointOpen(true); }}>Leave checkpoint 🌱</button>
            </div>
          </div>
        </section>

        <section className="collection">
          <div className="section-head">
            <div><span className="eyebrow">MY COLLECTION</span><h3>Little worlds, still growing.</h3></div>
            <button type="button" onClick={addProject}>Add project <span>＋</span></button>
          </div>

          <div className="shelf">
            {projects.map((project) => (
              <button
                key={project.id}
                type="button"
                className={`project-cover mood-${project.mood} ${project.id === active.id ? "active" : ""}`}
                onClick={() => setActiveId(project.id)}
              >
                <span className="cover-symbol">{project.symbol}</span>
                <div><strong>{project.name}</strong><small>{project.tagline}</small></div>
                <footer><span>{project.lastWorked}</span><span>{project.gitStatus}</span></footer>
              </button>
            ))}
            <button className="project-cover add-cover" type="button" onClick={addProject}>
              <span className="plus">＋</span>
              <div><strong>Add a local project</strong><small>Choose a folder and let Launchpad inspect it.</small></div>
            </button>
          </div>
          <div className="shelf-edge" />
        </section>

        <section className="today">
          <div><span className="eyebrow">TODAY</span><strong>Two good sessions is plenty.</strong></div>
          <div><small>11:41</small><strong>Rate Limiter</strong><span>1h 08m</span></div>
          <div><small>15:12</small><strong>Sifr ✦</strong><span>1h 18m</span></div>
          <div className="cat">₍^. .^₎⟆ <span>all good ✨</span></div>
        </section>
      </main>

      <div className="page-footer"><span>Local first. No streaks. No guilt.</span><span>Launchpad v0.1 · first light 🌱</span></div>

      {checkpointOpen && (
        <div className="backdrop" onMouseDown={() => setCheckpointOpen(false)}>
          <section className="checkpoint-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
            <span className="pin">✦</span>
            <span className="eyebrow">LEAVE A TRAIL 🌱</span>
            <h2>Where should Future You continue?</h2>
            <p>One sentence. Make tomorrow’s first five minutes easy.</p>
            <textarea autoFocus maxLength={180} value={checkpointDraft} onChange={(event) => setCheckpointDraft(event.target.value)} />
            <div><button type="button" onClick={() => setCheckpointOpen(false)}>Not now</button><button className="save" type="button" onClick={saveCheckpoint}>Save checkpoint ✨</button></div>
          </section>
        </div>
      )}

      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
