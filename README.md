# Launchpad 🌸

**A local-first desktop home for everything you build.**

Launchpad treats projects like a collection of living worlds instead of rows in an enterprise dashboard. It remembers the next concrete quest, the checkpoint you left for Future You, and enough real repository context to help you resume quickly.

## Production foundation

- Real local projects only—no seeded showcase data
- SQLite library stored in the operating system's app-data directory
- Versioned, transactional database migrations
- Canonical-path de-duplication when adding projects
- Persisted active project, quest, checkpoint, and last-opened time
- Native Git branch and clean/dirty inspection
- Native `package.json` script detection
- Safe VS Code and terminal launchers that resolve projects by database ID
- One-time migration of real projects from the prototype's local storage
- Loading, empty, failure, and in-progress UI states
- Restrictive release Content Security Policy with no remote font dependency
- Rust persistence/inspection tests and React workflow tests

Arbitrary command execution is intentionally absent. Before Launchpad runs project scripts, it needs an explicit per-project trust model and a clear record of what was approved.

## Stack

- Tauri 2
- React 19 and TypeScript
- Vite and Vitest
- Rust and SQLite (`rusqlite` with bundled SQLite)

## Run it

Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system, then:

```bash
npm ci
npm run tauri dev
```

The browser-only Vite view is useful for UI work but cannot pick folders or launch native applications:

```bash
npm run dev
```

On Windows, native actions expect Git, VS Code's `code` command, and Windows Terminal (`wt.exe`) on `PATH`.

## Verify it

```bash
npm run test:coverage
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build
```

## Architecture

```text
React UI
  └── src/platform/desktop.ts       typed native boundary
        └── Tauri commands
              ├── project inspection and launchers
              └── SQLite repository + migrations
```

The frontend never receives arbitrary filesystem authority: it invokes a small set of commands, while Rust canonicalizes paths and resolves saved projects by numeric ID before launching anything.

## Next product slice

1. Add an explicit trusted-action model for selected package scripts.
2. Track a real work session from **Continue → work → checkpoint**.
3. Surface latest commit and modified-file count.
4. Add database backup/export and recovery UX.

GitHub integration, AI features, and productivity scoring stay out until the core resume loop is dependable.
