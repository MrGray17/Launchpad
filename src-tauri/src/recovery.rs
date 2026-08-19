use crate::database::{legacy_migration_complete, load_library, LibraryState};
use rusqlite::{backup::Progress, Connection, OpenFlags, MAIN_DB};
use std::path::Path;

const SUPPORTED_SCHEMA_VERSION: i64 = 2;

fn recovery_error(message: &str, error: impl std::fmt::Display) -> String {
    eprintln!("Launchpad recovery error: {error}");
    message.to_string()
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
        return Err("That backup contains broken project references and was not restored.".to_string());
    }

    let library = load_library(connection).map_err(|error| {
        recovery_error(
            "That file is not a valid Launchpad library backup.",
            error,
        )
    })?;

    legacy_migration_complete(connection).map_err(|error| {
        recovery_error(
            "That file is not a valid Launchpad library backup.",
            error,
        )
    })?;

    let preference_rows = connection
        .query_row("SELECT COUNT(*) FROM preferences", [], |row| row.get::<_, i64>(0))
        .map_err(|error| {
            recovery_error(
                "That file is not a valid Launchpad library backup.",
                error,
            )
        })?;
    if preference_rows != 1 {
        return Err("That file is not a valid Launchpad library backup.".to_string());
    }

    Ok(library)
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
