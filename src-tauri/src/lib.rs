mod database;

use database::{
    load_library as load_library_from_database, mark_opened, project_path, set_active_project,
    update_focus, upsert_project, Database, LegacyProject, LibraryState, Project,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSnapshot {
    name: String,
    path: String,
    branch: String,
    git_status: String,
    scripts: Vec<String>,
}

fn read_branch(project_path: &Path) -> String {
    if !project_path.join(".git").exists() {
        return "not-a-git-repo".into();
    }
    match Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if branch.is_empty() {
                "detached".into()
            } else {
                branch
            }
        }
        _ => "detached".into(),
    }
}

fn read_scripts(project_path: &Path) -> Vec<String> {
    let package_path = project_path.join("package.json");
    let Ok(package_json) = fs::read_to_string(package_path) else {
        return vec![];
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&package_json) else {
        return vec![];
    };
    value
        .get("scripts")
        .and_then(|scripts| scripts.as_object())
        .map(|scripts| {
            let mut names = scripts.keys().cloned().collect::<Vec<_>>();
            names.sort();
            names
        })
        .unwrap_or_default()
}

fn read_git_status(project_path: &Path) -> String {
    if !project_path.join(".git").exists() {
        return "unknown".into();
    }
    match Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["status", "--porcelain"])
        .output()
    {
        Ok(output) if output.status.success() => {
            if output.stdout.is_empty() {
                "clean".into()
            } else {
                "dirty".into()
            }
        }
        _ => "unknown".into(),
    }
}

fn canonical_project_path(path: String) -> Result<PathBuf, String> {
    let project_path = PathBuf::from(path);
    if !project_path.is_dir() {
        return Err("That project folder does not exist.".into());
    }
    project_path
        .canonicalize()
        .map_err(|_| "Could not resolve that project folder.".to_string())
}

fn inspect_project_path(path: String) -> Result<ProjectSnapshot, String> {
    let canonical = canonical_project_path(path)?;
    let name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Project")
        .to_string();
    Ok(ProjectSnapshot {
        name,
        path: canonical.to_string_lossy().to_string(),
        branch: read_branch(&canonical),
        git_status: read_git_status(&canonical),
        scripts: read_scripts(&canonical),
    })
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

#[tauri::command]
async fn inspect_project(path: String) -> Result<ProjectSnapshot, String> {
    run_blocking(move || inspect_project_path(path)).await
}

#[tauri::command]
async fn load_library(database: State<'_, Database>) -> Result<LibraryState, String> {
    let database = database.inner().clone();
    run_database(database, |connection| {
        load_library_from_database(connection)
    })
    .await
}

#[tauri::command]
async fn add_project(path: String, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        let snapshot = inspect_project_path(path)?;
        upsert_project(connection, &snapshot, None, true)
    })
    .await
}

#[tauri::command]
async fn import_legacy_projects(
    projects: Vec<LegacyProject>,
    database: State<'_, Database>,
) -> Result<LibraryState, String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        for project in projects {
            match inspect_project_path(project.path.clone()) {
                Ok(snapshot) => {
                    upsert_project(connection, &snapshot, Some(&project), false)?;
                }
                Err(error) => {
                    eprintln!(
                        "Skipping legacy Launchpad project '{}': {error}",
                        project.path
                    );
                }
            }
        }
        load_library_from_database(connection)
    })
    .await
}

#[tauri::command]
async fn refresh_project(id: i64, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        let path = project_path(connection, id)?;
        let snapshot = inspect_project_path(path)?;
        upsert_project(connection, &snapshot, None, false)
    })
    .await
}

#[tauri::command]
async fn select_project(id: Option<i64>, database: State<'_, Database>) -> Result<(), String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        set_active_project(connection, id)
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

#[tauri::command]
async fn open_in_vscode(id: i64, database: State<'_, Database>) -> Result<Project, String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        let path = project_path(connection, id)?;
        let project_path = canonical_project_path(path)?;
        Command::new("code")
            .arg(project_path)
            .spawn()
            .map_err(|_| {
                "VS Code could not be opened. Make sure the `code` command is on PATH.".to_string()
            })?;
        mark_opened(connection, id)
    })
    .await
}

#[tauri::command]
async fn open_terminal(id: i64, database: State<'_, Database>) -> Result<(), String> {
    let database = database.inner().clone();
    run_database(database, move |connection| {
        let path = project_path(connection, id)?;
        let project_path = canonical_project_path(path)?;
        #[cfg(target_os = "windows")]
        {
            Command::new("wt.exe")
                .arg("-d")
                .arg(project_path)
                .spawn()
                .map(|_| ())
                .map_err(|_| "Windows Terminal could not be opened.".to_string())
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .args(["-a", "Terminal"])
                .arg(project_path)
                .spawn()
                .map(|_| ())
                .map_err(|_| "Terminal could not be opened.".to_string())
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Command::new("x-terminal-emulator")
                .current_dir(project_path)
                .spawn()
                .map(|_| ())
                .map_err(|_| "Terminal could not be opened.".to_string())
        }
    })
    .await
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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let database = initialize_database(app.handle())?;
            app.manage(database);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            inspect_project,
            load_library,
            add_project,
            import_legacy_projects,
            refresh_project,
            select_project,
            save_project_focus,
            open_in_vscode,
            open_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running Launchpad");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestProject(PathBuf);
    impl TestProject {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("launchpad-{name}-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path).expect("test project directory should be created");
            Self(path)
        }
    }
    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn inspect_project_reads_git_and_package_metadata() {
        let project = TestProject::new("metadata");
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .arg(&project.0)
            .status()
            .expect("git should be available for project inspection");
        assert!(status.success());
        fs::write(
            project.0.join("package.json"),
            r#"{"scripts":{"test":"vitest","build":"vite build","dev":"vite"}}"#,
        )
        .expect("package.json should be written");
        let snapshot = inspect_project_path(project.0.to_string_lossy().to_string())
            .expect("project inspection should succeed");
        assert_eq!(
            snapshot.name,
            project.0.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(
            snapshot.path,
            project.0.canonicalize().unwrap().to_string_lossy()
        );
        assert_ne!(snapshot.branch, "not-a-git-repo");
        assert_eq!(snapshot.git_status, "dirty");
        assert_eq!(snapshot.scripts, ["build", "dev", "test"]);
    }

    #[test]
    fn inspect_project_handles_a_non_git_folder_without_package_json() {
        let project = TestProject::new("plain-folder");
        let snapshot = inspect_project_path(project.0.to_string_lossy().to_string())
            .expect("plain folders should still be inspectable");
        assert_eq!(snapshot.branch, "not-a-git-repo");
        assert_eq!(snapshot.git_status, "unknown");
        assert!(snapshot.scripts.is_empty());
    }

    #[test]
    fn inspect_project_rejects_a_missing_folder() {
        let missing = std::env::temp_dir().join("launchpad-folder-that-does-not-exist");
        let error = inspect_project_path(missing.to_string_lossy().to_string())
            .expect_err("missing folders should be rejected");
        assert_eq!(error, "That project folder does not exist.");
    }

    #[test]
    fn inspect_project_reads_the_branch_from_a_linked_git_worktree() {
        let repository = TestProject::new("worktree-source");
        let linked = TestProject::new("worktree-linked");
        fs::remove_dir(&linked.0).expect("linked worktree target should start absent");

        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .arg(&repository.0)
            .status()
            .unwrap()
            .success());
        fs::write(repository.0.join("README.md"), "worktree fixture")
            .expect("fixture should be written");
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repository.0)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repository.0)
            .args([
                "-c",
                "user.name=Launchpad Test",
                "-c",
                "user.email=launchpad@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repository.0)
            .args(["worktree", "add", "--quiet", "-b", "feat/linked"])
            .arg(&linked.0)
            .status()
            .unwrap()
            .success());

        let snapshot = inspect_project_path(linked.0.to_string_lossy().to_string())
            .expect("linked worktrees should be inspectable");
        assert_eq!(snapshot.branch, "feat/linked");
        assert!(linked.0.join(".git").is_file());
    }
}
