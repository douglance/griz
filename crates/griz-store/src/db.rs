//! Schema, migration, and connection setup.

use crate::StoreError;
use rusqlite::Connection;
use std::path::Path;

/// Schema version this build writes.
pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    body TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    state TEXT NOT NULL,
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
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::Invalid(format!(
            "store schema {version} is newer than this griz ({SCHEMA_VERSION})"
        )));
    }
    if version < SCHEMA_VERSION && has_tables(&conn)? {
        backup(&conn, path, version)?;
    }
    conn.execute_batch(SCHEMA)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(conn)
}

fn has_tables(conn: &Connection) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn backup(conn: &Connection, path: &Path, version: i64) -> Result<(), StoreError> {
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
    let target = path.with_extension(format!(
        "pre-v{SCHEMA_VERSION}-from-v{version}-{}.sqlite3",
        crate::now_ms()
    ));
    std::fs::copy(path, target)?;
    Ok(())
}
