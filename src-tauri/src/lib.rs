mod database;
mod inspection;

use database::{
    backup_database, finish_legacy_migration, legacy_migration_complete,
    load_library as load_library_from_database, project_path, relink_project as relink_in_database,
    remove_project as remove_from_database, update_focus, update_metadata, upsert_project,
    validate_legacy_project, Database, LegacyProject, LibraryState, Project,
};
use inspection::{canonical_project_path, inspect_project_path};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapState {
    projects: Vec<Project>,
    active_project_id: Option<i64>,
    pending_legacy_ids: Vec<String>,
    legacy_migration_complete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupResult {
    file_name: String,
}

async fn run_blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| {
            eprintln!("Launchpad background task failed: {error}");
            "Launchpad could not finish that operation.".to_string()
        })?
}

async fn run_database<T, F>(database: Database, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut rusqlite::Connection) -> Result<T, String> + Send + 'static,
{
    run_blocking(move || database.with_connection(operation)).await
}

fn refresh_project_record(
    database: &Database,
    id: i64,
    make_active: bool,
    mark_as_opened: bool,
) -> Result<Project, String> {
    let path = database.with_connection(|connection| project_path(connection, id))?;
    let snapshot = inspect_project_path(path)?;
    database.with_connection(|connection| {
        update_metadata(connection, id, &snapshot, make_active, mark_as_opened)
    })
}

fn bootstrap_library(
    database: &Database,
    projects: Vec<LegacyProject>,
    active_legacy_id: Option<String>,
) -> Result<BootstrapState, String> {
    let already_complete =
        database.with_connection(|connection| legacy_migration_complete(connection))?;
    let mut pending_legacy_ids = Vec::new();

    if !already_complete {
        for project in projects {
            let result = validate_legacy_project(&project)
                .and_then(|_| inspect_project_path(project.path.clone()))
                .and_then(|snapshot| {
                    let make_active = active_legacy_id.as_deref() == Some(&project.legacy_id);
                    database.with_connection(|connection| {
                        upsert_project(connection, &snapshot, Some(&project), make_active)
                    })
                });
            if let Err(error) = result {
                eprintln!(
                    "Legacy Launchpad project '{}' remains pending: {error}",
                    project.legacy_id
                );
                pending_legacy_ids.push(project.legacy_id);
            }
        }
        if pending_legacy_ids.is_empty() {
            database.with_connection(|connection| finish_legacy_migration(connection))?;
        }
    }

    let library = database.with_connection(|connection| load_library_from_database(connection))?;
    if let Some(active_id) = library.active_project_id {
        let _ = refresh_project_record(database, active_id, true, false);
    }
    let library = database.with_connection(|connection| load_library_from_database(connection))?;
    let migration_complete =
        database.with_connection(|connection| legacy_migration_complete(connection))?;
    Ok(BootstrapState {
        projects: library.projects,
        active_project_id: library.active_project_id,
        pending_legacy_ids,
        legacy_migration_complete: migration_complete,
    })
}

#[tauri::command]
async fn bootstrap(
    projects: Vec<LegacyProject>,
    active_legacy_id: Option<String>,
    database: State<'_, Database>,
) -> Result<BootstrapState, String> {
    let database = database.inner().clone();
    run_blocking(move || bootstrap_library(&database, projects, active_legacy_id)).await
}

#[tauri::command]
async fn add_project(path: String, database: State<'_, Database>) -> Result<Project, String> {
    let snapshot = run_blocking(move || inspect_project_path(path)).await?;
    let database = database.inner().clone();
    run_database(database, move |connection| {
        upsert_project(connection, &snapshot, None, true)
    })
    .await
}

#[tauri::command]
async fn activate_project(id: i64, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_blocking(move || refresh_project_record(&database, id, true, false)).await
}

#[tauri::command]
async fn refresh_project(id: i64, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_blocking(move || refresh_project_record(&database, id, false, false)).await
}

#[tauri::command]
async fn relink_project(
    id: i64,
    path: String,
    database: State<'_, Database>,
) -> Result<Project, String> {
    let snapshot = run_blocking(move || inspect_project_path(path)).await?;
    let database = database.inner().clone();
    run_database(database, move |connection| {
        relink_in_database(connection, id, &snapshot)
    })
    .await
}

#[tauri::command]
async fn remove_project(id: i64, database: State<'_, Database>) -> Result<LibraryState, String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        remove_from_database(connection, id)
    })
    .await
}

#[tauri::command]
async fn save_project_focus(
    id: i64,
    quest: String,
    checkpoint: String,
    database: State<'_, Database>,
) -> Result<Project, String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        update_focus(connection, id, &quest, &checkpoint)
    })
    .await
}

fn spawn_first_available(mut candidates: Vec<Command>, failure: &str) -> Result<(), String> {
    for command in &mut candidates {
        match command.spawn() {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                eprintln!("Launchpad launcher failed: {error}");
                return Err(failure.to_string());
            }
        }
    }
    Err(failure.to_string())
}

fn editor_candidates(project_path: &Path) -> Vec<Command> {
    let mut candidates = Vec::new();
    for executable in ["code.cmd", "code"] {
        let mut command = Command::new(executable);
        command.arg(project_path);
        candidates.push(command);
    }
    #[cfg(target_os = "windows")]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let mut command = Command::new(
            PathBuf::from(local_app_data)
                .join("Programs")
                .join("Microsoft VS Code")
                .join("Code.exe"),
        );
        command.arg(project_path);
        candidates.push(command);
    }
    candidates
}

fn terminal_candidates(project_path: &Path) -> Vec<Command> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let mut terminal = Command::new("wt.exe");
        terminal.arg("-d").arg(project_path);
        candidates.push(terminal);
        for executable in ["powershell.exe", "cmd.exe"] {
            let mut command = Command::new(executable);
            command.current_dir(project_path);
            if executable == "powershell.exe" {
                command.arg("-NoExit");
            } else {
                command.arg("/K");
            }
            candidates.push(command);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut terminal = Command::new("open");
        terminal.args(["-a", "Terminal"]).arg(project_path);
        candidates.push(terminal);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    for executable in ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"] {
        let mut command = Command::new(executable);
        command.current_dir(project_path);
        candidates.push(command);
    }
    candidates
}

fn open_project(database: &Database, id: i64, terminal: bool) -> Result<Project, String> {
    let path = database.with_connection(|connection| project_path(connection, id))?;
    let snapshot = inspect_project_path(path.clone())?;
    let canonical = canonical_project_path(path)?;
    if terminal {
        spawn_first_available(
            terminal_candidates(&canonical),
            "No supported terminal could be opened. Install Windows Terminal, PowerShell, or another supported terminal.",
        )?;
    } else {
        spawn_first_available(
            editor_candidates(&canonical),
            "VS Code could not be opened. Install VS Code or enable its `code` command.",
        )?;
    }
    database.with_connection(|connection| update_metadata(connection, id, &snapshot, false, true))
}

#[tauri::command]
async fn open_in_vscode(id: i64, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_blocking(move || open_project(&database, id, false)).await
}

#[tauri::command]
async fn open_terminal(id: i64, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_blocking(move || open_project(&database, id, true)).await
}

#[tauri::command]
async fn backup_library(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<BackupResult, String> {
    let backup_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Launchpad could not locate its app-data folder.".to_string())?
        .join("backups");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Launchpad could not timestamp the backup.".to_string())?
        .as_nanos();
    let file_name = format!("launchpad-backup-{timestamp}.sqlite3");
    let path = backup_dir.join(&file_name);
    let database = database.inner().clone();
    run_blocking(move || {
        fs::create_dir_all(&backup_dir)
            .map_err(|_| "Launchpad could not create its backup folder.".to_string())?;
        database.with_connection(|connection| backup_database(connection, &path))
    })
    .await?;
    Ok(BackupResult { file_name })
}

fn initialize_database(app: &tauri::AppHandle) -> Result<Database, Box<dyn std::error::Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&app_data_dir)?;
    Database::open(&app_data_dir.join("launchpad.sqlite3"))
        .map_err(|error| std::io::Error::other(error).into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let database = initialize_database(app.handle())?;
            app.manage(database);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            add_project,
            activate_project,
            refresh_project,
            relink_project,
            remove_project,
            save_project_focus,
            open_in_vscode,
            open_terminal,
            backup_library
        ])
        .run(tauri::generate_context!())
        .expect("error while running Launchpad");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "launchpad-orchestration-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn failed_activation_does_not_change_the_persisted_active_project() {
        let first = TestDirectory::new("active-first");
        let missing = TestDirectory::new("active-missing");
        let storage = TestDirectory::new("active-database");
        let database = Database::open(&storage.0.join("library.sqlite3")).unwrap();
        let first_snapshot = inspect_project_path(first.0.to_string_lossy().to_string()).unwrap();
        let missing_snapshot =
            inspect_project_path(missing.0.to_string_lossy().to_string()).unwrap();
        let (first_id, missing_id) = database
            .with_connection(|connection| {
                let first_project = upsert_project(connection, &first_snapshot, None, true)?;
                let missing_project = upsert_project(connection, &missing_snapshot, None, false)?;
                Ok((first_project.id, missing_project.id))
            })
            .unwrap();
        fs::remove_dir_all(&missing.0).unwrap();

        assert!(refresh_project_record(&database, missing_id, true, false).is_err());
        let library = database
            .with_connection(|connection| load_library_from_database(connection))
            .unwrap();
        assert_eq!(library.active_project_id, Some(first_id));
    }

    #[test]
    fn partial_legacy_migration_keeps_missing_records_pending_and_restores_active() {
        let available = TestDirectory::new("legacy-available");
        let storage = TestDirectory::new("legacy-database");
        let database = Database::open(&storage.0.join("library.sqlite3")).unwrap();
        let projects = vec![
            LegacyProject {
                legacy_id: "available".to_string(),
                path: available.0.to_string_lossy().to_string(),
                quest: "Keep this quest".to_string(),
                checkpoint: "Keep this note".to_string(),
            },
            LegacyProject {
                legacy_id: "missing".to_string(),
                path: storage.0.join("missing").to_string_lossy().to_string(),
                quest: "Do not lose me".to_string(),
                checkpoint: "Drive is disconnected".to_string(),
            },
        ];
        let state = bootstrap_library(&database, projects, Some("available".to_string())).unwrap();
        assert_eq!(state.pending_legacy_ids, ["missing"]);
        assert!(!state.legacy_migration_complete);
        assert_eq!(state.projects.len(), 1);
        assert_eq!(state.active_project_id, Some(state.projects[0].id));
        assert_eq!(state.projects[0].quest, "Keep this quest");
    }
}
