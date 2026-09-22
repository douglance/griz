//! Upgrading journal order without losing records or WAL contents.

use crate::common::TestResult;
use griz_store::{Operation, OperationKind, OperationState, Store};
use rusqlite::{Connection, params};
use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};

fn legacy(home: &Path) -> Result<Connection, Box<dyn Error>> {
    let conn = Connection::open(home.join("griz.sqlite3"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA user_version=1;
         CREATE TABLE operations (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL,
             state TEXT NOT NULL, body TEXT NOT NULL);
         CREATE INDEX operations_state ON operations(state);
         CREATE TABLE plans (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, body TEXT NOT NULL);
         CREATE TABLE receipts (key TEXT PRIMARY KEY, command TEXT NOT NULL,
             fingerprint TEXT NOT NULL, state TEXT NOT NULL, result TEXT, created_at INTEGER NOT NULL);
         INSERT INTO plans VALUES ('plan_saved', 1, 'saved plan');
         INSERT INTO receipts VALUES ('key', 'apply', 'fp', 'done', 'saved result', 1);"
    )?;
    Ok(conn)
}

fn save(conn: &Connection, op: &Operation) -> TestResult {
    conn.execute(
        "INSERT INTO operations VALUES (?1, ?2, 'applied', ?3)",
        params![op.id, op.created_at, serde_json::to_string(op)?],
    )?;
    Ok(())
}

fn backups(home: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let paths = std::fs::read_dir(home)?.collect::<Result<Vec<_>, _>>()?;
    Ok(paths
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "sqlite3")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("pre-v2-from-v1"))
        })
        .collect())
}

fn assert_saved(conn: &Connection, version: i64) -> TestResult {
    let actual: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(actual, version);
    let plan: String = conn.query_row("SELECT body FROM plans", [], |row| row.get(0))?;
    let receipt: String = conn.query_row("SELECT result FROM receipts", [], |row| row.get(0))?;
    assert_eq!(plan, "saved plan");
    assert_eq!(receipt, "saved result");
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    assert_eq!(integrity, "ok");
    Ok(())
}

#[test]
fn migration_preserves_insertion_order_and_backs_up_wal_contents() -> TestResult {
    let home = tempfile::tempdir()?;
    let conn = legacy(home.path())?;
    let mut delayed = Operation::new(OperationKind::Apply, "delayed");
    let mut earlier = Operation::new(OperationKind::Apply, "earlier");
    delayed.state = OperationState::Applied;
    earlier.state = OperationState::Applied;
    assert!(delayed.id < earlier.id);
    save(&conn, &earlier)?;
    save(&conn, &delayed)?;
    assert!(std::fs::metadata(home.path().join("griz.sqlite3-wal"))?.len() > 0);
    let store = Store::open(home.path())?;
    assert_eq!(
        store.operations(10, None)?,
        vec![delayed.clone(), earlier.clone()]
    );
    assert_saved(&conn, 2)?;
    let paths = backups(home.path())?;
    assert_eq!(paths.len(), 1);
    let backup = Connection::open(&paths[0])?;
    assert_saved(&backup, 1)?;
    let ids = backup
        .prepare("SELECT id FROM operations ORDER BY rowid")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(ids, vec![earlier.id.clone(), delayed.id.clone()]);
    conn.execute_batch("VACUUM")?;
    drop(store);
    let store = Store::open(home.path())?;
    assert_eq!(
        store.operations(10, None)?,
        vec![delayed.clone(), earlier.clone()]
    );
    assert_eq!(store.operations(10, Some(&delayed.id))?, vec![earlier]);
    assert_eq!(backups(home.path())?.len(), 1);
    Ok(())
}

#[test]
fn concurrent_openers_upgrade_once() -> TestResult {
    let home = tempfile::tempdir()?;
    let conn = legacy(home.path())?;
    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let home = home.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                Store::open(&home)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
        })
        .collect();
    for handle in handles {
        handle.join().map_err(|_| "opener panicked")??;
    }
    assert_saved(&conn, 2)?;
    assert_eq!(backups(home.path())?.len(), 1);
    Ok(())
}

#[test]
fn a_newer_schema_is_refused_without_migration() -> TestResult {
    let home = tempfile::tempdir()?;
    let conn = legacy(home.path())?;
    conn.pragma_update(None, "user_version", 3)?;
    assert!(Store::open(home.path()).is_err());
    assert_saved(&conn, 3)?;
    assert!(backups(home.path())?.is_empty());
    Ok(())
}
