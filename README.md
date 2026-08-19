# Launchpad 🌸

**Your personal home for everything you build.**

Launchpad is a local-first desktop developer home that treats your projects like a collection of little worlds instead of rows in an enterprise dashboard. Open Launchpad, see exactly where you left off, and continue.

## What exists now

The first vertical slice is in the repository:

- Cozy React + TypeScript project library
- Game / manga-cover inspired project shelf
- One active **Current Quest** per project
- Persistent checkpoints using local storage for the first slice
- Add-local-project flow inside Tauri
- Native Git branch + dirty-state inspection
- Native `package.json` script detection
- Safe VS Code launcher
- Safe terminal launcher
- Responsive handcrafted visual system
- Tauri v2 desktop shell

The intentionally missing feature is arbitrary command execution. Before Launchpad runs `npm run dev`, tests, or custom commands on your behalf, it needs an explicit per-project trust model. That is a security boundary, not a TODO to rush.

## Stack

- Tauri 2
- React 19
- TypeScript
- Vite
- Rust for native desktop actions

## Run it

### Browser UI

```bash
npm install
npm run dev
```

### Desktop app

Install the Tauri prerequisites for your OS, then:

```bash
npm install
npm run tauri dev
```

On Windows, the current native actions expect:

- Git on PATH for working-tree inspection
- VS Code's `code` command on PATH
- Windows Terminal (`wt.exe`) for terminal launch

## Current architecture

```text
src/
├── App.tsx
├── main.tsx
├── styles.css
└── platform/
    └── desktop.ts

src-tauri/
├── capabilities/
│   └── default.json
├── src/
│   ├── lib.rs
│   └── main.rs
├── Cargo.toml
└── tauri.conf.json
```

The React layer talks to native behavior through `src/platform/desktop.ts`. That boundary stays small on purpose: UI code should not care whether project inspection, launching, or future persistence is implemented through Tauri, mocks, or another adapter.

## Next slice

1. Replace the seeded showcase projects with the real local collection as the default experience.
2. Persist projects, quests, checkpoints, and sessions in SQLite.
3. Add latest Git commit and modified-file count to native inspection.
4. Add **trusted actions** (`dev`, `test`, `build`) with explicit first-run approval.
5. Track one real session from **Continue → work → checkpoint**.
6. Add tests around project inspection and persistence.

No GitHub integration, AI, stats dashboard, or theme engine until that core loop feels excellent.
