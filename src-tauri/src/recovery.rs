use rusqlite::{backup::Progress, Connection, OpenFlags, MAIN_DB};
use std::path::Path;

const SUPPORTED_SCHEMA_VERSION: i64 = 2;

fn recovery_error(message: &str, error: rusqlite::Error) -> String {
    eprintln!("Launchpad recovery error: {error}");
    message.to_string()
}

pub(crate) fn validate_backup_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("That backup file does not exist.".to_string());
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| recovery_error("That file is not a readable Launchpad backup.", error))?;

    let integrity = connection
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
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

    for table in ["projects", "preferences"] {
        let count = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| recovery_error("That backup structure could not be verified.", error))?;
        if count != 1 {
            return Err("That file is not a valid Launchpad library backup.".to_string());
        }
    }

    Ok(())
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
        .map_err(|error| recovery_error("The restored library could not be reopened safely.", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

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

    fn create_valid_backup(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (id INTEGER PRIMARY KEY, marker TEXT);
                 CREATE TABLE preferences (singleton INTEGER PRIMARY KEY);
                 INSERT INTO projects (id, marker) VALUES (1, 'restored');
                 INSERT INTO preferences (singleton) VALUES (1);
                 PRAGMA user_version = 2;",
            )
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
    fn validates_and_restores_current_launchpad_backups() {
        let files = TestFiles::new("valid");
        let source = files.0.join("source.sqlite3");
        let destination = files.0.join("destination.sqlite3");
        create_valid_backup(&source);
        validate_backup_file(&source).unwrap();

        let mut connection = Connection::open(&destination).unwrap();
        restore_database(&mut connection, &source).unwrap();
        let marker = connection
            .query_row("SELECT marker FROM projects WHERE id = 1", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap();
        assert_eq!(marker, "restored");
    }
}
