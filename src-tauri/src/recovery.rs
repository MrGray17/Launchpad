use crate::database::{legacy_migration_complete, load_library, LibraryState};
use rusqlite::{backup::Progress, Connection, OpenFlags, MAIN_DB};
use std::path::Path;

const SUPPORTED_SCHEMA_VERSION: i64 = 2;
const EXPECTED_PROJECT_COLUMNS: &[&str] = &[
    "id",
    "name",
    "path",
    "branch",
    "git_status",
    "scripts_json",
    "quest",
    "checkpoint",
    "created_at",
    "updated_at",
    "last_opened_at",
    "metadata_status",
    "metadata_refreshed_at",
];
const EXPECTED_PREFERENCE_COLUMNS: &[&str] = &[
    "singleton",
    "active_project_id",
    "legacy_migration_complete",
];

fn recovery_error(message: &str, error: impl std::fmt::Display) -> String {
    eprintln!("Launchpad recovery error: {error}");
    message.to_string()
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<(String, bool, i64)>, String> {
    let pragma = match table {
        "projects" => "PRAGMA table_info('projects')",
        "preferences" => "PRAGMA table_info('preferences')",
        _ => return Err("Launchpad recovery attempted to inspect an unknown table.".to_string()),
    };
    let mut statement = connection
        .prepare(pragma)
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))
}

fn validate_table_shapes(connection: &Connection) -> Result<(), String> {
    let projects = table_columns(connection, "projects")?;
    let project_names = projects
        .iter()
        .map(|(name, _, _)| name.as_str())
        .collect::<Vec<_>>();
    if project_names != EXPECTED_PROJECT_COLUMNS {
        return Err("That file is not a valid Launchpad library backup.".to_string());
    }
    if projects.first().map(|(_, _, primary_key)| *primary_key) != Some(1) {
        return Err("That file is not a valid Launchpad library backup.".to_string());
    }
    for required in [
        "name",
        "path",
        "branch",
        "git_status",
        "scripts_json",
        "quest",
        "checkpoint",
        "created_at",
        "updated_at",
        "metadata_status",
    ] {
        if !projects
            .iter()
            .any(|(name, not_null, _)| name == required && *not_null)
        {
            return Err("That file is not a valid Launchpad library backup.".to_string());
        }
    }

    let preferences = table_columns(connection, "preferences")?;
    let preference_names = preferences
        .iter()
        .map(|(name, _, _)| name.as_str())
        .collect::<Vec<_>>();
    if preference_names != EXPECTED_PREFERENCE_COLUMNS
        || preferences.first().map(|(_, _, primary_key)| *primary_key) != Some(1)
        || !preferences.iter().any(|(name, not_null, _)| {
            name == "legacy_migration_complete" && *not_null
        })
    {
        return Err("That file is not a valid Launchpad library backup.".to_string());
    }
    Ok(())
}

fn has_unique_project_path(connection: &Connection) -> Result<bool, String> {
    let mut statement = connection
        .prepare("PRAGMA index_list('projects')")
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
    let indexes = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0))
        })
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;

    for (index_name, unique) in indexes {
        if !unique {
            continue;
        }
        let escaped = index_name.replace('\'', "''");
        let mut index_statement = connection
            .prepare(&format!("PRAGMA index_info('{escaped}')"))
            .map_err(|error| {
                recovery_error("That backup structure could not be verified.", error)
            })?;
        let columns = index_statement
            .query_map([], |row| row.get::<_, Option<String>>(2))
            .map_err(|error| {
                recovery_error("That backup structure could not be verified.", error)
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                recovery_error("That backup structure could not be verified.", error)
            })?;
        if columns == [Some("path".to_string())] {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_expected_preference_foreign_key(connection: &Connection) -> Result<bool, String> {
    let mut statement = connection
        .prepare("PRAGMA foreign_key_list('preferences')")
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
    let keys = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
    Ok(keys.iter().any(|(table, from, to, on_delete)| {
        table == "projects"
            && from == "active_project_id"
            && to == "id"
            && on_delete.eq_ignore_ascii_case("SET NULL")
    }))
}

fn validate_project_rows(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            "SELECT path, git_status, metadata_status, scripts_json, quest, checkpoint FROM projects",
        )
        .map_err(|error| recovery_error("That backup data could not be verified.", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|error| recovery_error("That backup data could not be verified.", error))?;

    for row in rows {
        let (path, git_status, metadata_status, scripts_json, quest, checkpoint) =
            row.map_err(|error| recovery_error("That backup data could not be verified.", error))?;
        if path.trim().is_empty()
            || !matches!(git_status.as_str(), "clean" | "dirty" | "unknown")
            || !matches!(
                metadata_status.as_str(),
                "fresh"
                    | "unknown"
                    | "not-a-repository"
                    | "git-unavailable"
                    | "invalid-repository"
                    | "timeout"
            )
            || serde_json::from_str::<Vec<String>>(&scripts_json).is_err()
            || quest.trim().is_empty()
            || quest.chars().count() > 120
            || checkpoint.trim().is_empty()
            || checkpoint.chars().count() > 180
        {
            return Err("That backup contains invalid Launchpad project data.".to_string());
        }
    }
    Ok(())
}

fn validate_preferences(connection: &Connection) -> Result<(), String> {
    let preference_rows = connection
        .query_row("SELECT COUNT(*) FROM preferences", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| {
            recovery_error("That file is not a valid Launchpad library backup.", error)
        })?;
    if preference_rows != 1 {
        return Err("That file is not a valid Launchpad library backup.".to_string());
    }

    legacy_migration_complete(connection).map_err(|error| {
        recovery_error("That file is not a valid Launchpad library backup.", error)
    })?;

    let dangling_active = connection
        .query_row(
            "SELECT COUNT(*)
             FROM preferences AS preference
             LEFT JOIN projects AS project ON project.id = preference.active_project_id
             WHERE preference.singleton = 1
               AND preference.active_project_id IS NOT NULL
               AND project.id IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| recovery_error("That backup data could not be verified.", error))?;
    if dangling_active != 0 {
        return Err("That backup contains an invalid active project reference.".to_string());
    }
    Ok(())
}

pub(crate) fn validate_library_connection(connection: &Connection) -> Result<LibraryState, String> {
    let integrity = connection
        .query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))
        .map_err(|error| recovery_error("That backup could not be checked safely.", error))?;
    if integrity != "ok" {
        return Err("That backup appears damaged and was not restored.".to_string());
    }

    let version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| recovery_error("That backup has no readable schema version.", error))?;
    if version != SUPPORTED_SCHEMA_VERSION {
        return Err(if version > SUPPORTED_SCHEMA_VERSION {
            "That backup was created by a newer Launchpad version.".to_string()
        } else {
            "That backup is from an older unsupported Launchpad version.".to_string()
        });
    }

    validate_table_shapes(connection)?;
    if !has_unique_project_path(connection)? || !has_expected_preference_foreign_key(connection)? {
        return Err("That file is not a valid Launchpad library backup.".to_string());
    }

    let mut foreign_key_check = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
    let mut violations = foreign_key_check
        .query([])
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
    if violations
        .next()
        .map_err(|error| recovery_error("That backup structure could not be verified.", error))?
        .is_some()
    {
        return Err(
            "That backup contains broken project references and was not restored.".to_string(),
        );
    }

    validate_project_rows(connection)?;
    validate_preferences(connection)?;
    load_library(connection)
        .map_err(|error| recovery_error("That file is not a valid Launchpad library backup.", error))
}

pub(crate) fn validate_backup_file(path: &Path) -> Result<LibraryState, String> {
    if !path.is_file() {
        return Err("That backup file does not exist.".to_string());
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| recovery_error("That file is not a readable Launchpad backup.", error))?;
    validate_library_connection(&connection)
}

pub(crate) fn restore_database(connection: &mut Connection, path: &Path) -> Result<(), String> {
    connection
        .restore(MAIN_DB, path, None::<fn(Progress)>)
        .map_err(|error| recovery_error("Launchpad could not restore that backup.", error))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|error| {
            recovery_error("The restored library could not be reopened safely.", error)
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::{upsert_project, Database},
        inspection::ProjectSnapshot,
    };
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestFiles(PathBuf);

    impl TestFiles {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "launchpad-recovery-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestFiles {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn create_launchpad_library(path: &Path, name: &str) {
        let database = Database::open(path).unwrap();
        database
            .with_connection(|connection| {
                let snapshot = ProjectSnapshot {
                    name: name.to_string(),
                    path: format!("C:\\Repos\\{name}"),
                    branch: "main".to_string(),
                    git_status: "clean".to_string(),
                    metadata_status: "fresh".to_string(),
                    scripts: vec!["build".to_string(), "test".to_string()],
                };
                upsert_project(connection, &snapshot, None, true)?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn rejects_non_sqlite_files() {
        let files = TestFiles::new("invalid");
        let path = files.0.join("not-a-backup.sqlite3");
        fs::write(&path, "not sqlite").unwrap();
        assert!(validate_backup_file(&path).is_err());
    }

    #[test]
    fn rejects_same_version_sqlite_with_incompatible_launchpad_schema() {
        let files = TestFiles::new("wrong-schema");
        let path = files.0.join("wrong-schema.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (id INTEGER PRIMARY KEY, marker TEXT);
                 CREATE TABLE preferences (
                    singleton INTEGER PRIMARY KEY,
                    active_project_id INTEGER,
                    legacy_migration_complete INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO projects (id, marker) VALUES (1, 'looks-valid');
                 INSERT INTO preferences (singleton, active_project_id) VALUES (1, 1);
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        drop(connection);

        let error = validate_backup_file(&path).unwrap_err();
        assert!(error.contains("not a valid Launchpad library backup"));
    }

    #[test]
    fn rejects_lookalike_schema_without_launchpad_constraints() {
        let files = TestFiles::new("missing-constraints");
        let path = files.0.join("missing-constraints.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    path TEXT NOT NULL,
                    branch TEXT NOT NULL,
                    git_status TEXT NOT NULL,
                    scripts_json TEXT NOT NULL,
                    quest TEXT NOT NULL,
                    checkpoint TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    last_opened_at TEXT,
                    metadata_status TEXT NOT NULL,
                    metadata_refreshed_at TEXT
                 );
                 CREATE TABLE preferences (
                    singleton INTEGER PRIMARY KEY,
                    active_project_id INTEGER,
                    legacy_migration_complete INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO preferences VALUES (1, NULL, 1);
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        drop(connection);

        assert!(validate_backup_file(&path).is_err());
    }

    #[test]
    fn rejects_invalid_serialized_project_data() {
        let files = TestFiles::new("invalid-data");
        let path = files.0.join("invalid-data.sqlite3");
        create_launchpad_library(&path, "Bad Scripts");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE projects SET scripts_json = 'not-json'", [])
            .unwrap();
        drop(connection);

        assert!(validate_backup_file(&path).is_err());
    }

    #[test]
    fn validates_and_restores_a_real_launchpad_library() {
        let files = TestFiles::new("valid");
        let source = files.0.join("source.sqlite3");
        let destination = files.0.join("destination.sqlite3");
        create_launchpad_library(&source, "Restored Project");

        let validated = validate_backup_file(&source).unwrap();
        assert_eq!(validated.projects.len(), 1);
        assert_eq!(validated.projects[0].name, "Restored Project");

        let mut destination_connection = Connection::open(&destination).unwrap();
        restore_database(&mut destination_connection, &source).unwrap();
        let restored = validate_library_connection(&destination_connection).unwrap();
        assert_eq!(restored.projects.len(), 1);
        assert_eq!(restored.projects[0].name, "Restored Project");
    }
}
