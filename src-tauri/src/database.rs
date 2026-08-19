use crate::inspection::ProjectSnapshot;
use rusqlite::{ffi::ErrorCode, params, Connection, OptionalExtension, Row, MAIN_DB};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const SCHEMA_VERSION: i64 = 2;
const DEFAULT_QUEST: &str = "Choose the next concrete step";
const DEFAULT_CHECKPOINT: &str = "Start by deciding what done looks like.";
const PROJECT_COLUMNS: &str = "
    id, name, path, branch, git_status, metadata_status, scripts_json, quest, checkpoint,
    created_at, updated_at, last_opened_at, metadata_refreshed_at
";

enum DatabaseState {
    Ready(Connection),
    Unavailable(String),
}

#[derive(Clone)]
pub(crate) struct Database {
    path: Arc<PathBuf>,
    state: Arc<Mutex<DatabaseState>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Project {
    pub(crate) id: i64,
    pub(crate) name: String,
    #[serde(skip_serializing)]
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) git_status: String,
    pub(crate) metadata_status: String,
    pub(crate) scripts: Vec<String>,
    pub(crate) quest: String,
    pub(crate) checkpoint: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) last_opened_at: Option<String>,
    pub(crate) metadata_refreshed_at: Option<String>,
    pub(crate) available: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyProject {
    pub(crate) legacy_id: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) quest: String,
    #[serde(default)]
    pub(crate) checkpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryState {
    pub(crate) projects: Vec<Project>,
    pub(crate) active_project_id: Option<i64>,
}

fn open_connection(path: &Path) -> Result<Connection, String> {
    let mut connection = Connection::open(path).map_err(database_error)?;
    migrate(&mut connection)?;
    Ok(connection)
}

impl Database {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let connection = open_connection(path)?;
        Ok(Self {
            path: Arc::new(path.to_path_buf()),
            state: Arc::new(Mutex::new(DatabaseState::Ready(connection))),
        })
    }

    pub(crate) fn unavailable(path: &Path, error: String) -> Self {
        Self {
            path: Arc::new(path.to_path_buf()),
            state: Arc::new(Mutex::new(DatabaseState::Unavailable(error))),
        }
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.path.as_ref().clone()
    }

    pub(crate) fn is_available(&self) -> bool {
        self.state
            .lock()
            .map(|state| matches!(&*state, DatabaseState::Ready(_)))
            .unwrap_or(false)
    }

    pub(crate) fn unavailable_error(&self) -> Option<String> {
        self.state.lock().ok().and_then(|state| match &*state {
            DatabaseState::Ready(_) => None,
            DatabaseState::Unavailable(error) => Some(error.clone()),
        })
    }

    pub(crate) fn mark_unavailable(&self, error: String) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Launchpad's local library is temporarily unavailable.".to_string())?;
        *state = DatabaseState::Unavailable(error);
        Ok(())
    }

    pub(crate) fn reopen(&self) -> Result<(), String> {
        let connection = open_connection(self.path.as_ref())?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Launchpad's local library is temporarily unavailable.".to_string())?;
        *state = DatabaseState::Ready(connection);
        Ok(())
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        let mut connection = Connection::open_in_memory().expect("in-memory database should open");
        migrate(&mut connection).expect("in-memory database should migrate");
        Self {
            path: Arc::new(PathBuf::from(":memory:")),
            state: Arc::new(Mutex::new(DatabaseState::Ready(connection))),
        }
    }

    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Launchpad's local library is temporarily unavailable.".to_string())?;
        match &mut *state {
            DatabaseState::Ready(connection) => operation(connection),
            DatabaseState::Unavailable(error) => Err(error.clone()),
        }
    }
}

fn database_error(error: rusqlite::Error) -> String {
    eprintln!("Launchpad database error: {error}");
    match error {
        rusqlite::Error::SqliteFailure(details, _) => match details.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => {
                "Launchpad's library is busy. Close any other Launchpad window and try again."
            }
            ErrorCode::PermissionDenied | ErrorCode::ReadOnly | ErrorCode::CannotOpen => {
                "Launchpad cannot write to its app-data folder. Check its permissions and free space."
            }
            ErrorCode::DiskFull => {
                "The device is out of space. Free some storage before Launchpad saves again."
            }
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                "Launchpad's library appears damaged. Restore a recent Launchpad backup."
            }
            ErrorCode::SystemIoFailure => {
                "Launchpad encountered a disk I/O error while accessing its library."
            }
            _ => "Launchpad could not access its local library.",
        },
        _ => "Launchpad could not access its local library.",
    }
    .to_string()
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

    let mut version = connection
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
            .pragma_update(None, "user_version", 1)
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        version = 1;
    }

    if version < 2 {
        let transaction = connection.transaction().map_err(database_error)?;
        transaction
            .execute_batch(
                "ALTER TABLE projects ADD COLUMN metadata_status TEXT NOT NULL DEFAULT 'unknown';
                 ALTER TABLE projects ADD COLUMN metadata_refreshed_at TEXT;
                 ALTER TABLE preferences ADD COLUMN legacy_migration_complete INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(database_error)?;
        transaction
            .pragma_update(None, "user_version", 2)
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
    }
    Ok(())
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let path = row.get::<_, String>(2)?;
    let scripts_json = row.get::<_, String>(6)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        // Filesystem availability is hydrated after the SQLite mutex is released.
        available: false,
        path,
        branch: row.get(3)?,
        git_status: row.get(4)?,
        metadata_status: row.get(5)?,
        scripts: serde_json::from_str(&scripts_json).unwrap_or_default(),
        quest: row.get(7)?,
        checkpoint: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        last_opened_at: row.get(11)?,
        metadata_refreshed_at: row.get(12)?,
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

fn validate_focus(quest: &str, checkpoint: &str) -> Result<(String, String), String> {
    let quest = quest.trim();
    let checkpoint = checkpoint.trim();
    if quest.is_empty() || quest.chars().count() > 120 {
        return Err("Keep the current quest between 1 and 120 characters.".to_string());
    }
    if checkpoint.is_empty() || checkpoint.chars().count() > 180 {
        return Err("Keep the checkpoint between 1 and 180 characters.".to_string());
    }
    Ok((quest.to_string(), checkpoint.to_string()))
}

pub(crate) fn validate_legacy_project(project: &LegacyProject) -> Result<(), String> {
    let quest = if project.quest.trim().is_empty() {
        DEFAULT_QUEST
    } else {
        &project.quest
    };
    let checkpoint = if project.checkpoint.trim().is_empty() {
        DEFAULT_CHECKPOINT
    } else {
        &project.checkpoint
    };
    validate_focus(quest, checkpoint).map(|_| ())
}

pub(crate) fn upsert_project(
    connection: &mut Connection,
    snapshot: &ProjectSnapshot,
    legacy: Option<&LegacyProject>,
    make_active: bool,
) -> Result<Project, String> {
    let scripts_json = serde_json::to_string(&snapshot.scripts)
        .map_err(|_| "Launchpad could not store the detected scripts.".to_string())?;
    let (quest, checkpoint) = if let Some(project) = legacy {
        let quest = if project.quest.trim().is_empty() {
            DEFAULT_QUEST
        } else {
            &project.quest
        };
        let checkpoint = if project.checkpoint.trim().is_empty() {
            DEFAULT_CHECKPOINT
        } else {
            &project.checkpoint
        };
        validate_focus(quest, checkpoint)?
    } else {
        (DEFAULT_QUEST.to_string(), DEFAULT_CHECKPOINT.to_string())
    };
    let transaction = connection.transaction().map_err(database_error)?;
    transaction
        .execute(
            "INSERT INTO projects (
                name, path, branch, git_status, metadata_status, scripts_json, quest, checkpoint,
                metadata_refreshed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                branch = excluded.branch,
                git_status = excluded.git_status,
                metadata_status = excluded.metadata_status,
                scripts_json = excluded.scripts_json,
                metadata_refreshed_at = excluded.metadata_refreshed_at,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                snapshot.name,
                snapshot.path,
                snapshot.branch,
                snapshot.git_status,
                snapshot.metadata_status,
                scripts_json,
                quest,
                checkpoint,
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

    if legacy.is_some() {
        transaction
            .execute(
                "UPDATE projects SET quest = ?2, checkpoint = ?3,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![id, quest, checkpoint],
            )
            .map_err(database_error)?;
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

pub(crate) fn update_metadata(
    connection: &mut Connection,
    id: i64,
    snapshot: &ProjectSnapshot,
    make_active: bool,
    mark_as_opened: bool,
) -> Result<Project, String> {
    let scripts_json = serde_json::to_string(&snapshot.scripts)
        .map_err(|_| "Launchpad could not store the detected scripts.".to_string())?;
    let transaction = connection.transaction().map_err(database_error)?;
    let changed = transaction
        .execute(
            "UPDATE projects SET name = ?2, branch = ?3, git_status = ?4,
                metadata_status = ?5, scripts_json = ?6,
                metadata_refreshed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                last_opened_at = CASE WHEN ?7 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE last_opened_at END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![
                id,
                snapshot.name,
                snapshot.branch,
                snapshot.git_status,
                snapshot.metadata_status,
                scripts_json,
                mark_as_opened,
            ],
        )
        .map_err(database_error)?;
    if changed == 0 {
        return Err("That project is no longer in your collection.".to_string());
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

pub(crate) fn relink_project(
    connection: &mut Connection,
    id: i64,
    snapshot: &ProjectSnapshot,
) -> Result<Project, String> {
    let scripts_json = serde_json::to_string(&snapshot.scripts)
        .map_err(|_| "Launchpad could not store the detected scripts.".to_string())?;
    let changed = connection
        .execute(
            "UPDATE projects SET name = ?2, path = ?3, branch = ?4, git_status = ?5,
                metadata_status = ?6, scripts_json = ?7,
                metadata_refreshed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            params![
                id,
                snapshot.name,
                snapshot.path,
                snapshot.branch,
                snapshot.git_status,
                snapshot.metadata_status,
                scripts_json,
            ],
        )
        .map_err(|error| match &error {
            rusqlite::Error::SqliteFailure(details, _)
                if details.code == ErrorCode::ConstraintViolation =>
            {
                "That folder already belongs to another project in Launchpad.".to_string()
            }
            _ => database_error(error),
        })?;
    if changed == 0 {
        return Err("That project is no longer in your collection.".to_string());
    }
    project_by_id(connection, id)
}

pub(crate) fn remove_project(connection: &mut Connection, id: i64) -> Result<LibraryState, String> {
    let transaction = connection.transaction().map_err(database_error)?;
    let changed = transaction
        .execute("DELETE FROM projects WHERE id = ?1", [id])
        .map_err(database_error)?;
    if changed == 0 {
        return Err("That project is no longer in your collection.".to_string());
    }
    let active: Option<i64> = transaction
        .query_row(
            "SELECT active_project_id FROM preferences WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(database_error)?;
    if active.is_none() {
        let next = transaction
            .query_row(
                "SELECT id FROM projects ORDER BY COALESCE(last_opened_at, created_at) DESC, name COLLATE NOCASE LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_error)?;
        transaction
            .execute(
                "UPDATE preferences SET active_project_id = ?1 WHERE singleton = 1",
                [next],
            )
            .map_err(database_error)?;
    }
    transaction.commit().map_err(database_error)?;
    load_library(connection)
}

pub(crate) fn update_focus(
    connection: &Connection,
    id: i64,
    quest: &str,
    checkpoint: &str,
) -> Result<Project, String> {
    let (quest, checkpoint) = validate_focus(quest, checkpoint)?;
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

pub(crate) fn legacy_migration_complete(connection: &Connection) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT legacy_migration_complete FROM preferences WHERE singleton = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(database_error)
}

pub(crate) fn finish_legacy_migration(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE preferences SET legacy_migration_complete = 1 WHERE singleton = 1",
            [],
        )
        .map_err(database_error)?;
    Ok(())
}

pub(crate) fn backup_database(connection: &Connection, path: &Path) -> Result<(), String> {
    connection
        .backup(MAIN_DB, path, None)
        .map_err(database_error)
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
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "launchpad-database-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&directory).unwrap();
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
            metadata_status: "fresh".to_string(),
            scripts: vec!["build".to_string(), "test".to_string()],
        }
    }

    #[test]
    fn new_database_has_the_current_schema() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                assert_eq!(
                    connection
                        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                        .unwrap(),
                    SCHEMA_VERSION
                );
                assert!(load_library(connection)?.projects.is_empty());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn unavailable_database_keeps_its_error_until_reopened() {
        let storage = TestDatabase::new("unavailable");
        let database = Database::unavailable(&storage.path, "broken library".to_string());
        assert!(!database.is_available());
        assert_eq!(database.unavailable_error().as_deref(), Some("broken library"));
        assert!(database.with_connection(|_| Ok(())).is_err());

        let healthy = Database::open(&storage.path).unwrap();
        drop(healthy);
        database.reopen().unwrap();
        assert!(database.is_available());
        assert!(database.unavailable_error().is_none());
    }

    #[test]
    fn version_one_database_migrates_without_losing_focus() {
        let storage = TestDatabase::new("v1-migration");
        let connection = Connection::open(&storage.path).unwrap();
        connection.execute_batch(
            "CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, path TEXT NOT NULL UNIQUE,
                branch TEXT NOT NULL, git_status TEXT NOT NULL, scripts_json TEXT NOT NULL DEFAULT '[]',
                quest TEXT NOT NULL, checkpoint TEXT NOT NULL, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, last_opened_at TEXT
             );
             CREATE TABLE preferences (
                singleton INTEGER PRIMARY KEY, active_project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL
             );
             INSERT INTO projects VALUES (1, 'Legacy', 'C:\\legacy', 'main', 'clean', '[]', 'Keep quest', 'Keep note', 'now', 'now', NULL);
             INSERT INTO preferences VALUES (1, 1);
             PRAGMA user_version = 1;"
        ).unwrap();
        drop(connection);

        let database = Database::open(&storage.path).unwrap();
        database
            .with_connection(|connection| {
                let library = load_library(connection)?;
                assert_eq!(library.projects[0].quest, "Keep quest");
                assert_eq!(library.projects[0].metadata_status, "unknown");
                assert!(!legacy_migration_complete(connection)?);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn upsert_deduplicates_paths_and_preserves_focus() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let original = upsert_project(connection, &snapshot("C:\\repo"), None, true)?;
                update_focus(
                    connection,
                    original.id,
                    "Ship safely",
                    "Add a regression test.",
                )?;
                let duplicate = upsert_project(connection, &snapshot("C:\\repo"), None, false)?;
                assert_eq!(original.id, duplicate.id);
                assert_eq!(duplicate.quest, "Ship safely");
                assert_eq!(load_library(connection)?.projects.len(), 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn serialized_projects_do_not_expose_absolute_paths() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let project =
                    upsert_project(connection, &snapshot("C:\\private\\repo"), None, true)?;
                let serialized = serde_json::to_value(project)
                    .map_err(|error| format!("Could not serialize test project: {error}"))?;
                assert!(serialized.get("path").is_none());
                assert_eq!(
                    serialized.get("name").and_then(|value| value.as_str()),
                    Some("Rate Limiter")
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn legacy_focus_is_validated_before_writing() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let legacy = LegacyProject {
                    legacy_id: "legacy".to_string(),
                    path: "C:\\repo".to_string(),
                    quest: "x".repeat(121),
                    checkpoint: "valid".to_string(),
                };
                assert!(
                    upsert_project(connection, &snapshot("C:\\repo"), Some(&legacy), true).is_err()
                );
                assert!(load_library(connection)?.projects.is_empty());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn remove_active_project_selects_a_surviving_project() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let first = upsert_project(connection, &snapshot("C:\\one"), None, true)?;
                let second = upsert_project(connection, &snapshot("C:\\two"), None, false)?;
                let library = remove_project(connection, first.id)?;
                assert_eq!(library.active_project_id, Some(second.id));
                assert_eq!(library.projects.len(), 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn relink_preserves_focus_and_rejects_duplicate_paths() {
        let database = Database::in_memory();
        database
            .with_connection(|connection| {
                let first = upsert_project(connection, &snapshot("C:\\one"), None, true)?;
                let second = upsert_project(connection, &snapshot("C:\\two"), None, false)?;
                update_focus(connection, first.id, "Preserve me", "Still here")?;
                let relinked = relink_project(connection, first.id, &snapshot("C:\\three"))?;
                assert_eq!(relinked.quest, "Preserve me");
                assert!(relink_project(connection, first.id, &snapshot("C:\\two")).is_err());
                assert_eq!(project_by_id(connection, second.id)?.path, "C:\\two");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn online_backup_contains_the_library() {
        let storage = TestDatabase::new("backup");
        let backup_path = storage.directory.join("backup.sqlite3");
        let database = Database::open(&storage.path).unwrap();
        database
            .with_connection(|connection| {
                upsert_project(connection, &snapshot("C:\\durable"), None, true)?;
                backup_database(connection, &backup_path)
            })
            .unwrap();
        let backup = Database::open(&backup_path).unwrap();
        assert_eq!(
            backup
                .with_connection(|connection| Ok(load_library(connection)?.projects.len()))
                .unwrap(),
            1
        );
    }

    #[test]
    fn newer_database_versions_are_rejected_without_mutation() {
        let storage = TestDatabase::new("future");
        let connection = Connection::open(&storage.path).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);
        assert!(Database::open(&storage.path).is_err());
        let connection = Connection::open(&storage.path).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION + 1
        );
    }
}
