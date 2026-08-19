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
- Restore creates a safety backup first and attempts automatic rollback on failure
- Warm light and neutral dark appearances
- Project-specific visual motifs instead of generic colored placeholder cards
- React workflow tests, Rust domain tests, and a Windows release smoke gate

Arbitrary project command execution is intentionally absent until Launchpad has an explicit per-project trust model.

## Supported toolchain

- Windows 10 or later
- Node.js 22 (`>=22.22.2 <23`; CI uses 22.22.2)
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
- **Export backup…** — writes a backup to a user-selected location
- **Restore backup…** — validates a current-version Launchpad backup, creates a pre-restore safety copy, then restores it

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
              `-- backup / restore recovery
```

Filesystem and Git inspection run outside the database mutex. React receives project IDs and display metadata rather than absolute filesystem paths.

## Product scope

The main screen is intentionally not a dashboard. The active project, Continue action, checkpoint, and visual collection stay prominent; maintenance lives behind compact menus.

Trusted commands, sessions, GitHub integration, MCP, and AI features come after this local resume loop is proven in daily use.
