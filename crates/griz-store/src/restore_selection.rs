//! Select restore membership and lock paths without retaining full records.

use crate::{Store, StoreError};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};

pub(crate) struct RestoreSelection {
    pub(crate) ids: Vec<String>,
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct LockRecord {
    id: String,
    files: Vec<LockFile>,
}

#[derive(Deserialize)]
struct LockFile {
    path: PathBuf,
}

impl Store {
    pub(crate) fn select_restore(
        &self,
        since: &str,
        paths: &[PathBuf],
    ) -> Result<RestoreSelection, StoreError> {
        self.with(|conn| select(conn, since, paths))
    }
}

fn select(
    conn: &Connection,
    since: &str,
    paths: &[PathBuf],
) -> Result<RestoreSelection, StoreError> {
    let sequence: i64 = conn
        .query_row(
            "SELECT sequence FROM operations WHERE id = ?1",
            [since],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("operation {since}")))?;
    let mut statement = conn.prepare(
        "SELECT body FROM operations WHERE sequence >= ?1 AND state = 'applied' ORDER BY sequence",
    )?;
    let mut rows = statement.query([sequence])?;
    let mut ids = Vec::new();
    let mut locked = BTreeSet::new();
    while let Some(row) = rows.next()? {
        let body: String = row.get(0)?;
        let record: LockRecord = serde_json::from_str(&body)?;
        ids.push(record.id);
        locked.extend(
            record
                .files
                .into_iter()
                .map(|file| file.path)
                .filter(|path| paths.is_empty() || paths.contains(path)),
        );
    }
    Ok(RestoreSelection {
        ids,
        paths: locked.into_iter().collect(),
    })
}
