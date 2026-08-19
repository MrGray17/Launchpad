# Launchpad

**A local-first Windows desktop home for everything you build.**

Launchpad treats projects as a collection of living worlds. It remembers the next concrete quest, the checkpoint you left for Future You, and current repository context so you can resume work safely.

## Production foundation

- Real local projects backed by versioned, transactional SQLite storage
- Canonical-path de-duplication, relinking, removal, and deterministic active-project recovery
- Persisted active project, quest, checkpoint, and last-opened time
- Fresh, bounded Git inspection at startup and before native launch actions
- Accurate unavailable, non-repository, invalid-repository, Git-unavailable, and timeout states
- Nested-repository detection and native `package.json` script discovery
- Safe VS Code and terminal launchers that resolve projects by database ID
- One global operation guard across project-changing UI actions
- Lossless, validated, active-aware migration from prototype browser storage
- Single-instance enforcement that focuses the existing window
- Restrictive CSP and an explicit allowlist of native commands
- No absolute project paths exposed to React
- On-demand, consistent SQLite backups in the app-data `backups` directory
- The original warm-white interface plus an optional persisted dark-grey theme
- Rust domain tests, React workflow tests, and a Windows release smoke gate

Arbitrary command execution is intentionally absent. It requires a separate trust model and is outside this hardening milestone.

## Supported toolchain

- Windows 10 or later
- Node.js 22 (`>=22.12 <23`; CI uses 22.22.0)
- npm 11.6.0
- Rust 1.97.1 with `rustfmt` and `clippy`
- Tauri 2 prerequisites, including Microsoft C++ Build Tools and WebView2

The Node/npm and Rust versions are declared in `package.json`, `rust-toolchain.toml`, and the Windows CI workflow.

## Run it

Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), then:

```powershell
npm ci
npm run tauri dev
```

The browser-only Vite view is read-only and useful for UI work, but it cannot pick folders, mutate the library, create backups, or launch native applications:

```powershell
npm run dev
```

Launchpad prefers VS Code's `code` command and Windows Terminal's `wt.exe`, with bounded Windows fallbacks. Git metadata reports an explicit unavailable state when Git cannot run.

## Recovery and backups

When a saved folder moves or is deleted, Launchpad keeps its quest and checkpoint. Use **Relink folder** to point the same record at its new location, or **Remove** to delete the library record. Removing a project does not delete its folder or source files.

Use **Back up library** to create a timestamped, online SQLite backup. Launchpad reports the backup filename without exposing its absolute app-data path to the frontend. Backups are kept under the Launchpad app-data directory in `backups`.

## Verification

The critical Windows workflow runs this gate on every push and pull request:

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

The smoke test starts the exact release executable, verifies that it remains alive and responsive through its startup window, and terminates only that process ID. Before a release, also complete this manual data-flow check with a disposable real repository:

```text
Add -> restart -> still present -> switch -> Continue -> terminal
-> edit quest/checkpoint -> restart -> values survive
-> move folder -> Relink -> metadata refreshes
-> Back up library -> backup succeeds
-> Remove -> source folder remains untouched
```

Coverage thresholds protect the tested lifecycle surface without treating a percentage as a substitute for scenario-based tests. Persistence, migration, missing folders, duplicate paths, Git failures/timeouts, launch failures, and state consistency are the priority.

## Architecture

```text
React UI
  `-- src/platform/desktop.ts       typed, narrow native boundary
        `-- Tauri commands
              |-- project inspection and launchers
              `-- SQLite repository and migrations/backups
```

Filesystem and Git inspection run outside the database mutex. Native commands canonicalize paths and resolve saved projects by numeric ID before launching anything.

## Scope boundary

This milestone deliberately stops at a dependable local project library. Trusted commands, sessions, GitHub integration, MCP, AI features, and broader visual polish are not part of this change.
