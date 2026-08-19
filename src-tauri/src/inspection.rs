use serde::Serialize;
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_COMMAND_OUTPUT: usize = 1024 * 1024;
const MAX_PACKAGE_JSON_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSnapshot {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) git_status: String,
    pub(crate) metadata_status: String,
    pub(crate) scripts: Vec<String>,
}

#[derive(Debug)]
struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum CommandFailure {
    Unavailable,
    TimedOut,
    Io,
}

fn drain_bounded<R: Read + Send + 'static>(mut reader: R) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let remaining = MAX_COMMAND_OUTPUT.saturating_sub(captured.len());
                    captured.extend_from_slice(&chunk[..read.min(remaining)]);
                }
            }
        }
        let _ = sender.send(captured);
    });
    receiver
}

fn receive_output(reader: Option<Receiver<Vec<u8>>>) -> Result<Vec<u8>, CommandFailure> {
    match reader {
        Some(reader) => reader
            .recv_timeout(OUTPUT_DRAIN_TIMEOUT)
            .map_err(|_| CommandFailure::Io),
        None => Ok(Vec::new()),
    }
}

fn run_bounded(command: &mut Command, timeout: Duration) -> Result<CommandOutput, CommandFailure> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CommandFailure::Unavailable
        } else {
            CommandFailure::Io
        }
    })?;
    let stdout_reader = child.stdout.take().map(drain_bounded);
    let stderr_reader = child.stderr.take().map(drain_bounded);
    let deadline = Instant::now() + timeout;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CommandFailure::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                return Err(CommandFailure::Io);
            }
        }
    };

    let stdout = receive_output(stdout_reader)?;
    let stderr = receive_output(stderr_reader)?;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn run_git(project_path: &Path, args: &[&str]) -> Result<CommandOutput, CommandFailure> {
    let mut command = Command::new("git");
    command.arg("-C").arg(project_path).args(args);
    run_bounded(&mut command, GIT_TIMEOUT)
}

fn command_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn git_failure_snapshot(
    status: &str,
    scripts: Vec<String>,
) -> (String, String, String, Vec<String>) {
    let branch = match status {
        "git-unavailable" => "Git unavailable",
        "timeout" => "Git inspection timed out",
        "invalid-repository" => "Git metadata unavailable",
        _ => "not a Git repository",
    };
    (
        branch.to_string(),
        "unknown".to_string(),
        status.to_string(),
        scripts,
    )
}

fn inspect_git(project_path: &Path, scripts: Vec<String>) -> (String, String, String, Vec<String>) {
    let repository = match run_git(
        project_path,
        &["rev-parse", "--is-inside-work-tree", "--show-toplevel"],
    ) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let error = command_text(&output.stderr).to_lowercase();
            let status = if error.contains("not a git repository") {
                "not-a-repository"
            } else {
                "invalid-repository"
            };
            return git_failure_snapshot(status, scripts);
        }
        Err(CommandFailure::Unavailable) => {
            return git_failure_snapshot("git-unavailable", scripts)
        }
        Err(CommandFailure::TimedOut) => return git_failure_snapshot("timeout", scripts),
        Err(CommandFailure::Io) => return git_failure_snapshot("invalid-repository", scripts),
    };
    if !command_text(&repository.stdout)
        .lines()
        .next()
        .is_some_and(|line| line == "true")
    {
        return git_failure_snapshot("not-a-repository", scripts);
    }

    let branch = match run_git(project_path, &["symbolic-ref", "--short", "HEAD"]) {
        Ok(output) if output.status.success() => {
            let value = command_text(&output.stdout);
            if value.is_empty() {
                "detached".to_string()
            } else {
                value
            }
        }
        Ok(output) if output.status.code() == Some(1) => "detached".to_string(),
        Ok(_) => return git_failure_snapshot("invalid-repository", scripts),
        Err(CommandFailure::Unavailable) => {
            return git_failure_snapshot("git-unavailable", scripts)
        }
        Err(CommandFailure::TimedOut) => return git_failure_snapshot("timeout", scripts),
        Err(CommandFailure::Io) => return git_failure_snapshot("invalid-repository", scripts),
    };

    match run_git(project_path, &["status", "--porcelain=v1"]) {
        Ok(output) if output.status.success() => {
            let git_status = if output.stdout.is_empty() {
                "clean"
            } else {
                "dirty"
            };
            (branch, git_status.to_string(), "fresh".to_string(), scripts)
        }
        Ok(_) => git_failure_snapshot("invalid-repository", scripts),
        Err(CommandFailure::Unavailable) => git_failure_snapshot("git-unavailable", scripts),
        Err(CommandFailure::TimedOut) => git_failure_snapshot("timeout", scripts),
        Err(CommandFailure::Io) => git_failure_snapshot("invalid-repository", scripts),
    }
}

fn read_scripts(project_path: &Path) -> Vec<String> {
    let package_path = project_path.join("package.json");
    if fs::metadata(&package_path)
        .map(|metadata| metadata.len() > MAX_PACKAGE_JSON_BYTES)
        .unwrap_or(false)
    {
        return vec![];
    }
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

pub(crate) fn canonical_project_path(path: String) -> Result<PathBuf, String> {
    let project_path = PathBuf::from(path);
    if !project_path.is_dir() {
        return Err("That project folder does not exist.".into());
    }
    project_path
        .canonicalize()
        .map_err(|_| "Could not resolve that project folder.".to_string())
}

pub(crate) fn inspect_project_path(path: String) -> Result<ProjectSnapshot, String> {
    let canonical = canonical_project_path(path)?;
    let name = canonical
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("Project")
        .to_string();
    let scripts = read_scripts(&canonical);
    let (branch, git_status, metadata_status, scripts) = inspect_git(&canonical, scripts);
    Ok(ProjectSnapshot {
        name,
        path: canonical.to_string_lossy().to_string(),
        branch,
        git_status,
        metadata_status,
        scripts,
    })
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
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "launchpad-inspection-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(project: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(project)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn inspects_nested_projects_from_the_parent_repository() {
        let project = TestProject::new("nested");
        git(&project.0, &["init", "--quiet"]);
        let nested = project.0.join("apps").join("web");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();

        let snapshot = inspect_project_path(nested.to_string_lossy().to_string()).unwrap();
        assert_ne!(snapshot.branch, "not a Git repository");
        assert_eq!(snapshot.metadata_status, "fresh");
        assert_eq!(snapshot.scripts, ["dev"]);
    }

    #[test]
    fn classifies_plain_folders_without_claiming_git_is_detached() {
        let project = TestProject::new("plain");
        let snapshot = inspect_project_path(project.0.to_string_lossy().to_string()).unwrap();
        assert_eq!(snapshot.branch, "not a Git repository");
        assert_eq!(snapshot.metadata_status, "not-a-repository");
        assert_eq!(snapshot.git_status, "unknown");
    }

    #[test]
    fn bounded_commands_are_killed_after_the_deadline() {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 2"]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 2"]);
            command
        };
        let started = Instant::now();
        let result = run_bounded(&mut command, Duration::from_millis(80));
        assert!(matches!(result, Err(CommandFailure::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn missing_executables_are_distinguished_from_timeouts() {
        let mut command = Command::new("launchpad-command-that-does-not-exist");
        assert!(matches!(
            run_bounded(&mut command, Duration::from_millis(50)),
            Err(CommandFailure::Unavailable)
        ));
    }
}
