//! Durable journal, locks, and recovery for griz edits.
//!
//! The store is the only part of griz that writes source files. Every write is
//! journaled as `applying` before the first rename, so an interrupted apply is
//! finished by the next process that opens the store.

mod absorb;
mod apply;
mod blobs;
mod db;
mod error;
mod execute;
mod journal;
mod listing;
mod locks;
mod merge;
mod plans;
mod receipts;
mod recovery;
mod restore;
mod write;

pub use absorb::Absorbed;
pub use apply::{ApplyRequest, OnStale, UndoRequest};
pub use blobs::{blob_id, parse_blob_id};
pub use error::StoreError;
pub use journal::{
    ConflictRegion, FileWrite, MergeConflict, Operation, OperationKind, OperationState, Restores,
};
pub use listing::Page;
pub use plans::{PlanRecord, Selection};
pub use receipts::Claim;

use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Environment variable naming the state directory.
pub const HOME_ENV: &str = "GRIZ_HOME";

/// Durable griz state for one home directory.
pub struct Store {
    home: PathBuf,
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens the store at `$GRIZ_HOME`, or the user data directory.
    ///
    /// # Errors
    /// Returns an error when the directory or database cannot be opened.
    pub fn open_default() -> Result<Self, StoreError> {
        let home = std::env::var_os(HOME_ENV)
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|dir| dir.join("griz")))
            .ok_or_else(|| StoreError::Invalid("no data directory for griz state".into()))?;
        Self::open(&home)
    }

    /// Opens the store at `home` and finishes any interrupted apply.
    ///
    /// # Errors
    /// Returns an error when the directory or database cannot be opened, or
    /// recovery cannot finish.
    pub fn open(home: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(home)?;
        let conn = db::open(&home.join("griz.sqlite3"))?;
        let store = Self {
            home: home.to_path_buf(),
            conn: Mutex::new(conn),
        };
        recovery::recover(&store)?;
        Ok(store)
    }

    /// The state directory.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    fn with<T>(
        &self,
        run: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| StoreError::Invalid("store connection poisoned".into()))?;
        run(&mut conn)
    }
}

/// Reads one JSON body column by identifier.
fn body_by_id(conn: &Connection, sql: &str, id: &str) -> Result<Option<String>, StoreError> {
    use rusqlite::OptionalExtension;
    Ok(conn.query_row(sql, [id], |row| row.get(0)).optional()?)
}

/// Reads every JSON body column a query returns.
fn bodies(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<String>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(params, |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// A new time-ordered identifier with `prefix`.
fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}
