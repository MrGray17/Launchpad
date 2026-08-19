mod database;
mod inspection;
mod recovery;

use database::{
    backup_database, finish_legacy_migration, legacy_migration_complete,
    load_library as load_library_from_database, project_path, relink_project as relink_in_database,
    remove_project as remove_from_database, update_focus, update_metadata, upsert_project,
    validate_legacy_project, Database, LegacyProject, LibraryState, Project,
};
use inspection::{canonical_project_path, inspect_project_path};
use recovery::{restore_database, validate_backup_file, validate_library_connection};
use serde::Serialize;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

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

#[derive(Clone, Copy)]
struct RawDatabasePresence {
    main: bool,
    wal: bool,
    shm: bool,
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

fn hydrate_project_availability(mut project: Project) -> Project {
    project.available = Path::new(&project.path).is_dir();
    project
}

fn hydrate_library_availability(mut library: LibraryState) -> LibraryState {
    for project in &mut library.projects {
        project.available = Path::new(&project.path).is_dir();
    }
    library
}

fn refresh_project_record(
    database: &Database,
    id: i64,
    make_active: bool,
    mark_as_opened: bool,
) -> Result<Project, String> {
    let path = database.with_connection(|connection| project_path(connection, id))?;
    let snapshot = inspect_project_path(path)?;
    let project = database.with_connection(|connection| {
        update_metadata(connection, id, &snapshot, make_active, mark_as_opened)
    })?;
    Ok(hydrate_project_availability(project))
}

fn refreshed_library(database: &Database) -> Result<LibraryState, String> {
    let library = database.with_connection(|connection| load_library_from_database(connection))?;
    let library = hydrate_library_availability(library);
    if let Some(active_id) = library.active_project_id {
        let _ = refresh_project_record(database, active_id, true, false);
    }
    let library = database.with_connection(|connection| load_library_from_database(connection))?;
    Ok(hydrate_library_availability(library))
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

    let library = refreshed_library(database)?;
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
    let project = run_database(database, move |connection| {
        upsert_project(connection, &snapshot, None, true)
    })
    .await?;
    run_blocking(move || Ok(hydrate_project_availability(project))).await
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
    let project = run_database(database, move |connection| {
        relink_in_database(connection, id, &snapshot)
    })
    .await?;
    run_blocking(move || Ok(hydrate_project_availability(project))).await
}

#[tauri::command]
async fn remove_project(id: i64, database: State<'_, Database>) -> Result<LibraryState, String> {
    let database = database.inner().clone();
    let library = run_database(database, move |connection| {
        remove_from_database(connection, id)
    })
    .await?;
    run_blocking(move || Ok(hydrate_library_availability(library))).await
}

#[tauri::command]
async fn save_project_focus(
    id: i64,
    quest: String,
    checkpoint: String,
    database: State<'_, Database>,
) -> Result<Project, String> {
    let database = database.inner().clone();
    let project = run_database(database, move |connection| {
        update_focus(connection, id, &quest, &checkpoint)
    })
    .await?;
    run_blocking(move || Ok(hydrate_project_availability(project))).await
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

#[cfg(target_os = "windows")]
fn is_windows_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

fn editor_candidates(project_path: &Path) -> Vec<Command> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(explicit) = std::env::var_os("LAUNCHPAD_VSCODE") {
            let explicit = PathBuf::from(explicit);
            if is_windows_executable(&explicit) {
                let mut command = Command::new(explicit);
                command.arg(project_path);
                candidates.push(command);
            }
        }

        let mut from_path = Command::new("Code.exe");
        from_path.arg(project_path);
        candidates.push(from_path);

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

        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(program_files) = std::env::var_os(variable) {
                let mut command = Command::new(
                    PathBuf::from(program_files)
                        .join("Microsoft VS Code")
                        .join("Code.exe"),
                );
                command.arg(project_path);
                candidates.push(command);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new("code");
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
            "No supported terminal could be opened. Install Windows Terminal or use PowerShell.",
        )?;
    } else {
        spawn_first_available(
            editor_candidates(&canonical),
            "VS Code could not be opened. Install VS Code or set LAUNCHPAD_VSCODE to Code.exe.",
        )?;
    }
    let project = database
        .with_connection(|connection| update_metadata(connection, id, &snapshot, false, true))?;
    Ok(hydrate_project_availability(project))
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

fn backup_directory(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "Launchpad could not locate its app-data folder.".to_string())
        .map(|path| path.join("backups"))
}

fn timestamped_backup_name(prefix: &str) -> Result<String, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Launchpad could not timestamp the backup.".to_string())?
        .as_nanos();
    Ok(format!("{prefix}-{timestamp}.sqlite3"))
}

fn write_backup(database: &Database, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Launchpad could not create the backup destination.".to_string())?;
    }
    database.with_connection(|connection| backup_database(connection, path))
}

fn same_file_path(first: &Path, second: &Path) -> bool {
    if first == second {
        return true;
    }
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

fn database_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

fn copy_if_exists(source: &Path, destination: &Path) -> Result<bool, String> {
    if !source.exists() {
        return Ok(false);
    }
    fs::copy(source, destination)
        .map(|_| true)
        .map_err(|error| {
            eprintln!("Launchpad recovery copy failed: {error}");
            "Launchpad could not preserve its current library before recovery.".to_string()
        })
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(path).map_err(|error| {
        eprintln!("Launchpad recovery cleanup failed: {error}");
        "Launchpad could not prepare its library files for recovery.".to_string()
    })
}

fn copy_raw_database(source: &Path, destination: &Path) -> Result<RawDatabasePresence, String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Launchpad could not create a safety backup.".to_string())?;
    }
    let main = copy_if_exists(source, destination)?;
    let wal = copy_if_exists(
        &database_sidecar(source, "-wal"),
        &database_sidecar(destination, "-wal"),
    )?;
    let shm = copy_if_exists(
        &database_sidecar(source, "-shm"),
        &database_sidecar(destination, "-shm"),
    )?;
    Ok(RawDatabasePresence { main, wal, shm })
}

fn clear_raw_database(path: &Path) -> Result<(), String> {
    remove_if_exists(&database_sidecar(path, "-wal"))?;
    remove_if_exists(&database_sidecar(path, "-shm"))?;
    remove_if_exists(path)
}

fn restore_raw_database(
    safety_path: &Path,
    live_path: &Path,
    presence: RawDatabasePresence,
) -> Result<(), String> {
    clear_raw_database(live_path)?;
    if presence.main {
        fs::copy(safety_path, live_path).map_err(|error| {
            eprintln!("Launchpad raw rollback failed: {error}");
            "Launchpad could not restore its original library file.".to_string()
        })?;
    }
    if presence.wal {
        fs::copy(
            database_sidecar(safety_path, "-wal"),
            database_sidecar(live_path, "-wal"),
        )
        .map_err(|error| {
            eprintln!("Launchpad WAL rollback failed: {error}");
            "Launchpad could not restore its original library WAL file.".to_string()
        })?;
    }
    if presence.shm {
        fs::copy(
            database_sidecar(safety_path, "-shm"),
            database_sidecar(live_path, "-shm"),
        )
        .map_err(|error| {
            eprintln!("Launchpad SHM rollback failed: {error}");
            "Launchpad could not restore its original library SHM file.".to_string()
        })?;
    }
    Ok(())
}

fn restore_with_rollback<F>(
    connection: &mut rusqlite::Connection,
    source: &Path,
    safety_path: &Path,
    verify_restored: F,
) -> Result<LibraryState, String>
where
    F: FnOnce(&rusqlite::Connection) -> Result<LibraryState, String>,
{
    let attempt = restore_database(connection, source).and_then(|_| verify_restored(connection));
    match attempt {
        Ok(library) => Ok(library),
        Err(restore_error) => {
            let rollback = restore_database(connection, safety_path)
                .and_then(|_| validate_library_connection(connection).map(|_| ()));
            if let Err(rollback_error) = rollback {
                eprintln!("Launchpad restore rollback failed: {rollback_error}");
                return Err(
                    "Restore failed and Launchpad could not automatically roll back. Keep the safety backup and restart Launchpad."
                        .to_string(),
                );
            }
            Err(restore_error)
        }
    }
}

fn restore_unavailable_database_with_verify<F>(
    database: &Database,
    source: &Path,
    safety_path: &Path,
    verify_restored: F,
) -> Result<LibraryState, String>
where
    F: FnOnce(&Database) -> Result<LibraryState, String>,
{
    let original_error = database
        .unavailable_error()
        .ok_or_else(|| "Launchpad's library is already available.".to_string())?;
    let live_path = database.path();
    if same_file_path(source, &live_path) {
        return Err("Choose a backup file other than Launchpad's live library.".to_string());
    }
    validate_backup_file(source)?;

    let live_parent = live_path
        .parent()
        .ok_or_else(|| "Launchpad could not locate its library folder.".to_string())?;
    fs::create_dir_all(live_parent)
        .map_err(|_| "Launchpad could not prepare its library folder for recovery.".to_string())?;
    if let Some(parent) = safety_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Launchpad could not create a safety backup.".to_string())?;
    }

    let stage_path = live_parent.join(timestamped_backup_name("launchpad-restore-stage")?);
    fs::copy(source, &stage_path).map_err(|error| {
        eprintln!("Launchpad restore staging failed: {error}");
        "Launchpad could not stage that backup safely.".to_string()
    })?;
    if let Err(error) = validate_backup_file(&stage_path) {
        let _ = fs::remove_file(&stage_path);
        return Err(error);
    }

    let presence = copy_raw_database(&live_path, safety_path)?;
    let attempt = (|| {
        database.mark_unavailable(original_error.clone())?;
        clear_raw_database(&live_path)?;
        fs::rename(&stage_path, &live_path).map_err(|error| {
            eprintln!("Launchpad restore install failed: {error}");
            "Launchpad could not install that backup safely.".to_string()
        })?;
        database.reopen()?;
        verify_restored(database)
    })();

    match attempt {
        Ok(library) => Ok(library),
        Err(restore_error) => {
            let _ = database.mark_unavailable(original_error.clone());
            let rollback = restore_raw_database(safety_path, &live_path, presence);
            let _ = fs::remove_file(&stage_path);
            if let Err(rollback_error) = rollback {
                eprintln!("Launchpad raw recovery rollback failed: {rollback_error}");
                return Err(
                    "Restore failed and Launchpad could not put the original library files back automatically. Keep the safety backup and restart Launchpad."
                        .to_string(),
                );
            }
            Err(restore_error)
        }
    }
}

fn restore_library_from_path(
    database: &Database,
    source: &Path,
    safety_path: &Path,
) -> Result<LibraryState, String> {
    if same_file_path(source, &database.path()) {
        return Err("Choose a backup file other than Launchpad's live library.".to_string());
    }
    validate_backup_file(source)?;
    if let Some(parent) = safety_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "Launchpad could not create a safety backup.".to_string())?;
    }

    if !database.is_available() {
        let restored = restore_unavailable_database_with_verify(
            database,
            source,
            safety_path,
            |database| {
                database.with_connection(|connection| validate_library_connection(connection))
            },
        )?;
        return Ok(hydrate_library_availability(restored));
    }

    let restored = database.with_connection(|connection| {
        backup_database(connection, safety_path)?;
        validate_backup_file(safety_path)?;
        restore_with_rollback(connection, source, safety_path, validate_library_connection)
    })?;
    Ok(hydrate_library_availability(restored))
}

#[tauri::command]
async fn backup_library(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<BackupResult, String> {
    if !database.is_available() {
        return Err(database
            .unavailable_error()
            .unwrap_or_else(|| "Launchpad's library is unavailable.".to_string()));
    }
    let backup_dir = backup_directory(&app)?;
    let file_name = timestamped_backup_name("launchpad-backup")?;
    let path = backup_dir.join(&file_name);
    let database = database.inner().clone();
    run_blocking(move || write_backup(&database, &path)).await?;
    Ok(BackupResult { file_name })
}

#[tauri::command]
async fn export_library(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<Option<BackupResult>, String> {
    if !database.is_available() {
        return Err(database
            .unavailable_error()
            .unwrap_or_else(|| "Launchpad's library is unavailable.".to_string()));
    }
    let selected = app
        .dialog()
        .file()
        .set_title("Export Launchpad backup")
        .set_file_name("launchpad-backup.sqlite3")
        .add_filter("Launchpad backup", &["sqlite3"])
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let destination = selected
        .into_path()
        .map_err(|_| "Launchpad could not use that backup destination.".to_string())?;
    if destination.is_dir() {
        return Err("Choose a backup file, not a folder.".to_string());
    }
    if same_file_path(&destination, &database.path()) {
        return Err("Choose a destination other than Launchpad's live library.".to_string());
    }
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("launchpad-backup.sqlite3")
        .to_string();
    let database = database.inner().clone();
    run_blocking(move || write_backup(&database, &destination)).await?;
    Ok(Some(BackupResult { file_name }))
}

#[tauri::command]
async fn restore_library(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<Option<LibraryState>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Restore Launchpad backup")
        .add_filter("Launchpad backup", &["sqlite3"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected
        .into_path()
        .map_err(|_| "Launchpad could not use that backup file.".to_string())?;
    let backup_dir = backup_directory(&app)?;
    let safety_name = timestamped_backup_name("launchpad-before-restore")?;
    let safety_path = backup_dir.join(safety_name);
    let database = database.inner().clone();
    let restored = run_blocking({
        let database = database.clone();
        move || restore_library_from_path(&database, &source, &safety_path)
    })
    .await?;
    Ok(Some(restored))
}

fn initialize_database(app: &tauri::AppHandle) -> Result<Database, Box<dyn std::error::Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&app_data_dir)?;
    let database_path = app_data_dir.join("launchpad.sqlite3");
    Ok(match Database::open(&database_path) {
        Ok(database) => database,
        Err(error) => {
            eprintln!("Launchpad entered library recovery mode: {error}");
            Database::unavailable(&database_path, error)
        }
    })
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
            backup_library,
            export_library,
            restore_library
        ])
        .run(tauri::generate_context!())
        .expect("error while running Launchpad");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspection::ProjectSnapshot;
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

    fn insert_project(database: &Database, name: &str, path: &str, make_active: bool) {
        database
            .with_connection(|connection| {
                let snapshot = ProjectSnapshot {
                    name: name.to_string(),
                    path: path.to_string(),
                    branch: "main".to_string(),
                    git_status: "clean".to_string(),
                    metadata_status: "fresh".to_string(),
                    scripts: vec!["build".to_string()],
                };
                upsert_project(connection, &snapshot, None, make_active)?;
                Ok(())
            })
            .unwrap();
    }

    fn create_library_file(path: &Path, name: &str) {
        let database = Database::open(path).unwrap();
        insert_project(&database, name, &format!("C:\\Repos\\{name}"), true);
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

    #[test]
    fn database_reads_do_not_probe_the_filesystem_while_locked() {
        let existing = TestDirectory::new("availability-existing");
        let storage = TestDirectory::new("availability-database");
        let database = Database::open(&storage.0.join("library.sqlite3")).unwrap();
        insert_project(&database, "Existing", &existing.0.to_string_lossy(), true);
        insert_project(
            &database,
            "Missing",
            &storage.0.join("missing").to_string_lossy(),
            false,
        );

        let persisted = database
            .with_connection(|connection| load_library_from_database(connection))
            .unwrap();
        assert!(persisted.projects.iter().all(|project| !project.available));

        let hydrated = hydrate_library_availability(persisted);
        assert!(hydrated
            .projects
            .iter()
            .find(|project| project.name == "Existing")
            .is_some_and(|project| project.available));
        assert!(hydrated
            .projects
            .iter()
            .find(|project| project.name == "Missing")
            .is_some_and(|project| !project.available));
    }

    #[test]
    fn verified_restore_replaces_the_live_library() {
        let storage = TestDirectory::new("restore-success");
        let source = storage.0.join("source.sqlite3");
        let live = storage.0.join("live.sqlite3");
        let safety = storage.0.join("safety.sqlite3");
        create_library_file(&source, "Restored Project");
        let database = Database::open(&live).unwrap();
        insert_project(&database, "Original Project", "C:\\Repos\\Original", true);

        let restored = restore_library_from_path(&database, &source, &safety).unwrap();
        assert_eq!(restored.projects.len(), 1);
        assert_eq!(restored.projects[0].name, "Restored Project");
        let persisted = database
            .with_connection(|connection| load_library_from_database(connection))
            .unwrap();
        assert_eq!(persisted.projects[0].name, "Restored Project");
    }

    #[test]
    fn post_restore_verification_failure_rolls_back_the_live_library() {
        let storage = TestDirectory::new("restore-rollback");
        let source = storage.0.join("source.sqlite3");
        let live = storage.0.join("live.sqlite3");
        let safety = storage.0.join("safety.sqlite3");
        create_library_file(&source, "Restored Project");
        let database = Database::open(&live).unwrap();
        insert_project(&database, "Original Project", "C:\\Repos\\Original", true);

        let error = database
            .with_connection(|connection| {
                backup_database(connection, &safety)?;
                validate_backup_file(&safety)?;
                restore_with_rollback(connection, &source, &safety, |_| {
                    Err("forced post-restore verification failure".to_string())
                })
            })
            .unwrap_err();
        assert!(error.contains("forced post-restore verification failure"));

        let rolled_back = database
            .with_connection(|connection| load_library_from_database(connection))
            .unwrap();
        assert_eq!(rolled_back.projects.len(), 1);
        assert_eq!(rolled_back.projects[0].name, "Original Project");
    }

    #[test]
    fn unavailable_restore_preserves_raw_files_and_recovers() {
        let storage = TestDirectory::new("unavailable-restore");
        let source = storage.0.join("source.sqlite3");
        let live = storage.0.join("live.sqlite3");
        let safety = storage.0.join("safety.sqlite3");
        create_library_file(&source, "Recovered Project");
        fs::write(&live, b"broken live database").unwrap();
        fs::write(database_sidecar(&live, "-wal"), b"broken wal").unwrap();
        fs::write(database_sidecar(&live, "-shm"), b"broken shm").unwrap();
        let database = Database::unavailable(&live, "damaged library".to_string());

        let restored = restore_library_from_path(&database, &source, &safety).unwrap();
        assert!(database.is_available());
        assert_eq!(restored.projects[0].name, "Recovered Project");
        assert_eq!(fs::read(&safety).unwrap(), b"broken live database");
        assert_eq!(
            fs::read(database_sidecar(&safety, "-wal")).unwrap(),
            b"broken wal"
        );
        assert_eq!(
            fs::read(database_sidecar(&safety, "-shm")).unwrap(),
            b"broken shm"
        );
    }

    #[test]
    fn unavailable_restore_rolls_back_raw_files_after_post_install_failure() {
        let storage = TestDirectory::new("unavailable-rollback");
        let source = storage.0.join("source.sqlite3");
        let live = storage.0.join("live.sqlite3");
        let safety = storage.0.join("safety.sqlite3");
        create_library_file(&source, "Recovered Project");
        fs::write(&live, b"original broken database").unwrap();
        fs::write(database_sidecar(&live, "-wal"), b"original wal").unwrap();
        fs::write(database_sidecar(&live, "-shm"), b"original shm").unwrap();
        let database = Database::unavailable(&live, "damaged library".to_string());

        let error = restore_unavailable_database_with_verify(
            &database,
            &source,
            &safety,
            |_| Err("forced post-install verification failure".to_string()),
        )
        .unwrap_err();
        assert!(error.contains("forced post-install verification failure"));
        assert!(!database.is_available());
        assert_eq!(fs::read(&live).unwrap(), b"original broken database");
        assert_eq!(
            fs::read(database_sidecar(&live, "-wal")).unwrap(),
            b"original wal"
        );
        assert_eq!(
            fs::read(database_sidecar(&live, "-shm")).unwrap(),
            b"original shm"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn vscode_candidates_never_use_shell_script_shims() {
        let project = Path::new("C:\\repo");
        assert!(editor_candidates(project).iter().all(|command| {
            !command
                .get_program()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".cmd")
        }));
    }
}
