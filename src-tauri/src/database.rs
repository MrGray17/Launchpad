use crate::ProjectSnapshot;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_QUEST: &str = "Choose the next concrete step";
const DEFAULT_CHECKPOINT: &str = "Start by deciding what done looks like.";
const PROJECT_COLUMNS: &str = "
    id, name, path, branch, git_status, scripts_json, quest, checkpoint,
    created_at, updated_at, last_opened_at
";

#[derive(Clone)]
pub(crate) struct Database(Arc<Mutex<Connection>>);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Project {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) git_status: String,
    pub(crate) scripts: Vec<String>,
    pub(crate) quest: String,
    pub(crate) checkpoint: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) last_opened_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyProject {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) quest: String,
    #[serde(default)]
    pub(crate) checkpoint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryState {
    pub(crate) projects: Vec<Project>,
    pub(crate) active_project_id: Option<i64>,
}

impl Database {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let mut connection = Connection::open(path).map_err(database_error)?;
        migrate(&mut connection)?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        let mut connection = Connection::open_in_memory().expect("in-memory database should open");
        migrate(&mut connection).expect("in-memory database should migrate");
        Self(Arc::new(Mutex::new(connection)))
    }

    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut connection = self
            .0
            .lock()
            .map_err(|_| "Launchpad's local library is temporarily unavailable.".to_string())?;
        operation(&mut connection)
    }
}

fn database_error(error: rusqlite::Error) -> String {
    eprintln!("Launchpad database error: {error}");
    "Launchpad could not access its local library.".to_string()
}

fn migrate(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(database_error)?;

    let version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(database_error)?;
    if version > SCHEMA_VERSION {
        return Err("This Launchpad library was created by a newer app version.".to_string());
    }

    if version < 1 {
        let transaction = connection.transaction().map_err(database_error)?;
        transaction
            .execute_batch(
                "CREATE TABLE projects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    path TEXT NOT NULL UNIQUE,
                    branch TEXT NOT NULL,
                    git_status TEXT NOT NULL CHECK (git_status IN ('clean', 'dirty', 'unknown')),
                    scripts_json TEXT NOT NULL DEFAULT '[]',
                    quest TEXT NOT NULL,
                    checkpoint TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    last_opened_at TEXT
                );
                CREATE TABLE preferences (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    active_project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL
                );
                INSERT INTO preferences (singleton, active_project_id) VALUES (1, NULL);",
            )
            .map_err(database_error)?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
    }
    Ok(())
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let scripts_json = row.get::<_, String>(5)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        branch: row.get(3)?,
        git_status: row.get(4)?,
        scripts: serde_json::from_str(&scripts_json).unwrap_or_default(),
        quest: row.get(6)?,
        checkpoint: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_opened_at: row.get(10)?,
    })
}

pub(crate) fn load_library(connection: &Connection) -> Result<LibraryState, String> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects
             ORDER BY COALESCE(last_opened_at, created_at) DESC, name COLLATE NOCASE"
        ))
        .map_err(database_error)?;
    let projects = statement
        .query_map([], project_from_row)
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    let active_project_id = connection
        .query_row(
            "SELECT active_project_id FROM preferences WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(database_error)?
        .flatten();
    Ok(LibraryState {
        projects,
        active_project_id,
    })
}

pub(crate) fn project_by_id(connection: &Connection, id: i64) -> Result<Project, String> {
    connection
        .query_row(
            &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"),
            [id],
            project_from_row,
        )
        .optional()
        .map_err(database_error)?
        .ok_or_else(|| "That project is no longer in your collection.".to_string())
}

pub(crate) fn project_path(connection: &Connection, id: i64) -> Result<String, String> {
    connection
        .query_row("SELECT path FROM projects WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(database_error)?
        .ok_or_else(|| "That project is no longer in your collection.".to_string())
}

pub(crate) fn upsert_project(
    connection: &mut Connection,
    snapshot: &ProjectSnapshot,
    legacy: Option<&LegacyProject>,
    make_active: bool,
) -> Result<Project, String> {
    let scripts_json = serde_json::to_string(&snapshot.scripts)
        .map_err(|_| "Launchpad could not store the detected scripts.".to_string())?;
    let transaction = connection.transaction().map_err(database_error)?;
    transaction
        .execute(
            "INSERT INTO projects (name, path, branch, git_status, scripts_json, quest, checkpoint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                branch = excluded.branch,
                git_status = excluded.git_status,
                scripts_json = excluded.scripts_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                snapshot.name,
                snapshot.path,
                snapshot.branch,
                snapshot.git_status,
                scripts_json,
                DEFAULT_QUEST,
                DEFAULT_CHECKPOINT,
            ],
        )
        .map_err(database_error)?;
    let id = transaction
        .query_row(
            "SELECT id FROM projects WHERE path = ?1",
            [&snapshot.path],
            |row| row.get::<_, i64>(0),
        )
        .map_err(database_error)?;

    if let Some(legacy) = legacy {
        let quest = legacy.quest.trim();
        let checkpoint = legacy.checkpoint.trim();
        if !quest.is_empty() || !checkpoint.is_empty() {
            transaction
                .execute(
                    "UPDATE projects SET
                        quest = CASE WHEN ?2 = '' THEN quest ELSE ?2 END,
                        checkpoint = CASE WHEN ?3 = '' THEN checkpoint ELSE ?3 END,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![id, quest, checkpoint],
                )
                .map_err(database_error)?;
        }
    }
    if make_active {
        transaction
            .execute(
                "UPDATE preferences SET active_project_id = ?1 WHERE singleton = 1",
                [id],
            )
            .map_err(database_error)?;
    }
    transaction.commit().map_err(database_error)?;
    project_by_id(connection, id)
}

pub(crate) fn update_focus(
    connection: &Connection,
    id: i64,
    quest: &str,
    checkpoint: &str,
) -> Result<Project, String> {
    let quest = quest.trim();
    let checkpoint = checkpoint.trim();
    if quest.is_empty() || quest.chars().count() > 120 {
        return Err("Keep the current quest between 1 and 120 characters.".to_string());
    }
    if checkpoint.is_empty() || checkpoint.chars().count() > 180 {
        return Err("Keep the checkpoint between 1 and 180 characters.".to_string());
    }
    let changed = connection
        .execute(
            "UPDATE projects SET quest = ?2, checkpoint = ?3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            params![id, quest, checkpoint],
        )
        .map_err(database_error)?;
    if changed == 0 {
        return Err("That project is no longer in your collection.".to_string());
    }
    project_by_id(connection, id)
}

pub(crate) fn mark_opened(connection: &Connection, id: i64) -> Result<Project, String> {
    let changed = connection
        .execute(
            "UPDATE projects SET
                last_opened_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [id],
        )
        .map_err(database_error)?;
    if changed == 0 {
        return Err("That project is no longer in your collection.".to_string());
    }
    project_by_id(connection, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDatabase {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TestDatabase {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "launchpad-database-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&directory).expect("database test directory should be created");
            let path = directory.join("launchpad.sqlite3");
            Self { directory, path }
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn snapshot(path: &str) -> ProjectSnapshot {
        ProjectSnapshot {
            name: "Rate Limiter".to_string(),
            path: path.to_string(),
            branch: "main".to_string(),
            git_status: "clean".to_string(),
            scripts: vec!["build".to_string(), "test".to_string()],
        }
    }

    #[test]
    fn migration_creates_an_empty_library() {
        let database = Database::in_memory();
        let library = database
            .with_connection(|connection| load_library(connection))
            .unwrap();
        assert!(library.projects.is_empty());
        assert_eq!(library.active_project_id, None);
    }

    #[test]
    fn upsert_deduplicates_paths_and_preserves_focus() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let original = upsert_project(connection, &snapshot("C:\\repo"), None, true)?;
                let focused = update_focus(
                    connection,
                    original.id,
                    "Ship the token bucket",
                    "Add the capacity regression test.",
                )?;
                let mut refreshed = snapshot("C:\\repo");
                refreshed.branch = "feat/token-bucket".to_string();
                let duplicate = upsert_project(connection, &refreshed, None, false)?;
                assert_eq!(original.id, duplicate.id);
                assert_eq!(duplicate.branch, "feat/token-bucket");
                assert_eq!(duplicate.quest, focused.quest);
                assert_eq!(duplicate.checkpoint, focused.checkpoint);
                let library = load_library(connection)?;
                assert_eq!(library.projects.len(), 1);
                assert_eq!(library.active_project_id, Some(original.id));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn focus_validation_is_enforced_natively() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let project = upsert_project(connection, &snapshot("C:\\repo"), None, true)?;
                assert!(update_focus(connection, project.id, "", "checkpoint").is_err());
                assert!(update_focus(connection, project.id, "quest", "").is_err());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn library_survives_closing_and_reopening_the_database() {
        let storage = TestDatabase::new("reopen");
        let project_id = {
            let database = Database::open(&storage.path).unwrap();
            database
                .with_connection(|connection| {
                    let project =
                        upsert_project(connection, &snapshot("C:\\durable-project"), None, true)?;
                    update_focus(
                        connection,
                        project.id,
                        "Persist the library",
                        "Close and reopen the SQLite connection.",
                    )?;
                    mark_opened(connection, project.id)?;
                    Ok(project.id)
                })
                .unwrap()
        };

        let reopened = Database::open(&storage.path).unwrap();
        let library = reopened
            .with_connection(|connection| load_library(connection))
            .unwrap();
        assert_eq!(library.active_project_id, Some(project_id));
        assert_eq!(library.projects.len(), 1);
        assert_eq!(library.projects[0].quest, "Persist the library");
        assert_eq!(
            library.projects[0].checkpoint,
            "Close and reopen the SQLite connection."
        );
        assert!(library.projects[0].last_opened_at.is_some());
    }

    #[test]
    fn focus_updates_trim_input_and_count_unicode_characters() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let project = upsert_project(connection, &snapshot("C:\\repo"), None, true)?;
                let updated = update_focus(
                    connection,
                    project.id,
                    "  Ship safely  ",
                    "  Preserve the user's context.  ",
                )?;
                assert_eq!(updated.quest, "Ship safely");
                assert_eq!(updated.checkpoint, "Preserve the user's context.");
                assert!(update_focus(connection, project.id, &"🌸".repeat(121), "valid").is_err());
                assert!(update_focus(connection, project.id, "valid", &"🌱".repeat(181)).is_err());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn newer_database_versions_fail_without_mutating_the_file() {
        let storage = TestDatabase::new("future-version");
        let connection = Connection::open(&storage.path).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);

        let error = match Database::open(&storage.path) {
            Ok(_) => panic!("future database versions must be rejected"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            "This Launchpad library was created by a newer app version."
        );
        let connection = Connection::open(&storage.path).unwrap();
        let version = connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION + 1);
    }
}
