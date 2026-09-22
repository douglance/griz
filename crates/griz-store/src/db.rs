//! Schema, migration, and connection setup.

use crate::StoreError;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::path::Path;

/// Schema version this build writes.
pub const SCHEMA_VERSION: i64 = 2;

const OPERATIONS: &str = "
CREATE TABLE IF NOT EXISTS operations (
    sequence INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    state TEXT NOT NULL,
    body TEXT NOT NULL
);
";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    body TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS operations_state ON operations(state);
CREATE TABLE IF NOT EXISTS receipts (
    key TEXT PRIMARY KEY,
    command TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    state TEXT NOT NULL,
    result TEXT,
    created_at INTEGER NOT NULL
);
";

/// Opens the database, backing it up before any schema upgrade.
///
/// # Errors
/// Returns an error when the file cannot be opened, backed up, or migrated.
pub fn open(path: &Path) -> Result<Connection, StoreError> {
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    enable_wal(&conn, path)?;
    if version(&conn)? < SCHEMA_VERSION {
        upgrade(&mut conn, path)?;
    }
    Ok(conn)
}

fn enable_wal(conn: &Connection, path: &Path) -> Result<(), StoreError> {
    let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if mode == "wal" {
        return Ok(());
    }
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("init.lock"))?;
    lock.lock()?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(())
}

fn version(conn: &Connection) -> Result<i64, StoreError> {
    let version = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::Invalid(format!(
            "store schema {version} is newer than this griz ({SCHEMA_VERSION})"
        )));
    }
    Ok(version)
}

fn upgrade(conn: &mut Connection, path: &Path) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = version(&tx)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if has_table(&tx, None)? {
        backup(path, version)?;
    }
    if has_table(&tx, Some("operations"))? {
        tx.execute_batch("ALTER TABLE operations RENAME TO operations_v1")?;
        tx.execute_batch(OPERATIONS)?;
        tx.execute_batch(
            "INSERT INTO operations (sequence, id, created_at, state, body)
             SELECT rowid, id, created_at, state, body FROM operations_v1 ORDER BY rowid;
             DROP TABLE operations_v1;",
        )?;
    } else {
        tx.execute_batch(OPERATIONS)?;
    }
    tx.execute_batch(SCHEMA)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

fn has_table(conn: &Connection, name: Option<&str>) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND (?1 IS NULL OR name = ?1)",
        [name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn backup(path: &Path, version: i64) -> Result<(), StoreError> {
    let target = path.with_extension(format!(
        "pre-v{SCHEMA_VERSION}-from-v{version}-{}.sqlite3",
        crate::new_id("backup")
    ));
    let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    source.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    Ok(())
}
