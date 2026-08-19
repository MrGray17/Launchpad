# Launchpad

**A local-first Windows desktop home for everything you build.**

Launchpad turns local repositories into a visual project collection. It keeps one current quest and checkpoint per project, refreshes real Git/package metadata, and gets you back into VS Code or a terminal with minimal friction.

## Current foundation

- Real local projects only — no showcase seed data
- SQLite-backed project library with versioned transactional migrations
- Canonical-path de-duplication, relinking, removal, and deterministic active-project recovery
- Persisted quest, checkpoint, last-opened time, and active project
- Fresh bounded Git inspection with timeout/unavailable/invalid-repository states
- Nested Git repository detection and `package.json` script discovery
- Safe ID-based VS Code and terminal launchers
- Single-instance desktop behavior
- Explicit Tauri command allowlist and restrictive CSP
- Absolute project paths remain native and are not serialized to React
- Internal online backups plus user-selected export and validated restore
- Restore validates SQLite integrity, schema version, foreign keys, and Launchpad's real library read path before mutation
- Restore creates and verifies a safety backup first, then rolls back if restore **or post-restore verification** fails
- Export/restore file dialogs are owned by the native Tauri commands; React cannot provide arbitrary filesystem destinations
- Warm light and neutral dark appearances
- Project-specific visual motifs instead of generic colored placeholder cards
- React workflow tests, Rust domain/recovery tests, and a Windows release smoke gate

Arbitrary project command execution is intentionally absent until Launchpad has an explicit per-project trust model.

## Supported toolchain

- Windows 10 or later
- Node.js 22 (`>=22.12 <23`; CI uses 22.22.2)
- npm 11.6.0
- Rust 1.97.1 with `rustfmt` and `clippy`
- Tauri 2 prerequisites, including Microsoft C++ Build Tools and WebView2

## Run it

```powershell
npm ci
npm run tauri dev
```

The browser-only Vite view is read-only and useful for UI work:

```powershell
npm run dev
```

Launchpad prefers `Code.exe` directly on Windows. If VS Code lives somewhere unusual, set `LAUNCHPAD_VSCODE` to the full `Code.exe` path. Terminal launch prefers Windows Terminal and falls back to PowerShell or Command Prompt.

## Recovery

The app menu contains three maintenance actions:

- **Back up now** — creates an online SQLite backup under Launchpad app-data
- **Export backup…** — the native layer opens the save dialog and writes a backup to the selected location
- **Restore backup…** — the native layer opens the file dialog, validates a current-version Launchpad library using the same read path as the app, creates and verifies a pre-restore safety copy, restores, verifies again, and rolls back on any restore/verification failure

Moved or deleted project folders do not destroy project context. Use **Relink folder…** to point an existing project record at its new location, or remove the Launchpad record without deleting source files.

## Verification

The Windows workflow runs:

```powershell
npm ci
npm run test:coverage
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
npm run tauri build
powershell -ExecutionPolicy Bypass -File scripts/smoke-release.ps1
```

Recovery regression coverage includes:

- rejecting a readable/current-version SQLite file with an incompatible Launchpad schema
- accepting and restoring a database produced by Launchpad itself
- rolling the live database back when post-restore verification fails
- keeping export/restore filesystem paths entirely inside native Tauri commands

Before a release, also run the real data-flow check with a disposable repository:

```text
Add -> restart -> still present -> switch -> Continue -> terminal
-> edit quest/checkpoint -> restart -> values survive
-> move folder -> Relink -> metadata refreshes
-> Export backup -> Restore backup -> values survive
-> Remove -> source folder remains untouched
```

## Architecture

```text
React UI
  `-- src/platform/desktop.ts       narrow typed native boundary
        `-- Tauri commands
              |-- project inspection / launchers
              |-- SQLite repository
              `-- native-dialog backup / restore recovery
```

Filesystem and Git inspection run outside the database mutex. React receives project IDs and display metadata rather than absolute filesystem paths, and recovery destinations never cross the webview boundary.

## Product scope

The main screen is intentionally not a dashboard. The active project, Continue action, checkpoint, and visual collection stay prominent; maintenance lives behind compact menus.

Trusted commands, sessions, GitHub integration, MCP, and AI features come after this local resume loop is proven in daily use.
