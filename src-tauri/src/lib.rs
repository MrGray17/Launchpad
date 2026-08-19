use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSnapshot {
    name: String,
    path: String,
    branch: String,
    git_status: String,
    scripts: Vec<String>,
}

fn read_branch(project_path: &Path) -> String {
    let head_path = project_path.join(".git").join("HEAD");
    let Ok(head) = fs::read_to_string(head_path) else {
        return "not-a-git-repo".into();
    };

    head.trim()
        .strip_prefix("ref: refs/heads/")
        .unwrap_or("detached")
        .to_string()
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

#[tauri::command]
fn inspect_project(path: String) -> Result<ProjectSnapshot, String> {
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

#[tauri::command]
fn open_in_vscode(path: String) -> Result<(), String> {
    let project_path = canonical_project_path(path)?;

    Command::new("code")
        .arg(project_path)
        .spawn()
        .map(|_| ())
        .map_err(|_| "VS Code could not be opened. Make sure the `code` command is on PATH.".into())
}

#[tauri::command]
fn open_terminal(path: String) -> Result<(), String> {
    let project_path = canonical_project_path(path)?;

    #[cfg(target_os = "windows")]
    {
        return Command::new("wt.exe")
            .arg("-d")
            .arg(project_path)
            .spawn()
            .map(|_| ())
            .map_err(|_| "Windows Terminal could not be opened.".into());
    }

    #[cfg(target_os = "macos")]
    {
        return Command::new("open")
            .args(["-a", "Terminal"])
            .arg(project_path)
            .spawn()
            .map(|_| ())
            .map_err(|_| "Terminal could not be opened.".into());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("x-terminal-emulator")
            .current_dir(project_path)
            .spawn()
            .map(|_| ())
            .map_err(|_| "Terminal could not be opened.".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            inspect_project,
            open_in_vscode,
            open_terminal
        ])
        .run(tauri::generate_context!())
        .expect("error while running Launchpad");
}
