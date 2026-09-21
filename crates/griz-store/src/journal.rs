//! Operations: every apply and undo, recorded before and after it writes.

use crate::{Store, StoreError, now_ms};
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What an operation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Wrote a plan.
    Apply,
    /// Restored the files of an earlier operation.
    Undo,
}

/// Where an operation is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Journaled; renames may be partly done. Recovery finishes it.
    Applying,
    /// Every file write is done.
    Applied,
    /// Refused before writing anything.
    Failed,
}

impl OperationState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }
}

/// One file an operation writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileWrite {
    /// File path.
    pub path: PathBuf,
    /// Fingerprint on disk just before the write; absent when the file did not exist.
    pub before_hash: Option<String>,
    /// Fingerprint written; absent when the write deletes the file.
    pub after_hash: Option<String>,
    /// Written through a three-way merge onto a file that had moved on.
    #[serde(default)]
    pub merged: bool,
}

/// One apply or undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Operation {
    /// Identifier.
    pub id: String,
    /// Apply or undo.
    pub kind: OperationKind,
    /// Plan applied, for an apply.
    pub plan: Option<String>,
    /// Operation restored, for an undo.
    pub undoes: Option<String>,
    /// Life-cycle state.
    pub state: OperationState,
    /// Why the caller asked for it.
    pub purpose: String,
    /// Every file written, in write order.
    pub files: Vec<FileWrite>,
    /// Files left alone because they no longer matched what was expected.
    pub conflicts: Vec<PathBuf>,
    /// Why the operation failed, when it did.
    pub reason: Option<String>,
    /// Finished by recovery after an interruption.
    #[serde(default)]
    pub recovered: bool,
    /// Files whose later changes, such as a formatter's, were absorbed.
    #[serde(default)]
    pub absorbed: Vec<PathBuf>,
    /// Why the latest absorb happened.
    #[serde(default)]
    pub absorb_purpose: Option<String>,
    /// Creation time in Unix milliseconds.
    pub created_at: i64,
}

impl Operation {
    /// A new operation with no files yet.
    #[must_use]
    pub fn new(kind: OperationKind, purpose: &str) -> Self {
        Self {
            id: crate::new_id("op"),
            kind,
            plan: None,
            undoes: None,
            state: OperationState::Applying,
            purpose: purpose.to_string(),
            files: Vec::new(),
            conflicts: Vec::new(),
            reason: None,
            recovered: false,
            absorbed: Vec::new(),
            absorb_purpose: None,
            created_at: now_ms(),
        }
    }
}

impl Store {
    /// Inserts or replaces an operation record.
    ///
    /// # Errors
    /// Returns an error when the record cannot be written.
    pub fn save_operation(&self, op: &Operation) -> Result<(), StoreError> {
        let body = serde_json::to_string(op)?;
        self.with(|conn| {
            conn.execute(
                "INSERT INTO operations (id, created_at, state, body) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET state = excluded.state, body = excluded.body",
                params![op.id, op.created_at, op.state.as_str(), body],
            )?;
            Ok(())
        })
    }

    /// Reads one operation.
    ///
    /// # Errors
    /// Returns `NotFound` when no operation has this identifier.
    pub fn operation(&self, id: &str) -> Result<Operation, StoreError> {
        let sql = "SELECT body FROM operations WHERE id = ?1";
        let body = self.with(|conn| crate::body_by_id(conn, sql, id))?;
        let body = body.ok_or_else(|| StoreError::NotFound(format!("operation {id}")))?;
        Ok(serde_json::from_str(&body)?)
    }

    /// Operations newest first, starting below `before` when given.
    ///
    /// # Errors
    /// Returns an error when the records cannot be read.
    pub fn operations(
        &self,
        limit: usize,
        before: Option<&str>,
    ) -> Result<Vec<Operation>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let bodies = self.with(|conn| {
            crate::bodies(
                conn,
                "SELECT body FROM operations WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2",
                params![before, limit],
            )
        })?;
        bodies
            .iter()
            .map(|body| Ok(serde_json::from_str(body)?))
            .collect()
    }

    /// Operations left `applying` by an interrupted process.
    ///
    /// # Errors
    /// Returns an error when the records cannot be read.
    pub fn unfinished_operations(&self) -> Result<Vec<Operation>, StoreError> {
        let bodies = self.with(|conn| {
            crate::bodies(
                conn,
                "SELECT body FROM operations WHERE state = 'applying' ORDER BY id",
                [],
            )
        })?;
        bodies
            .iter()
            .map(|body| Ok(serde_json::from_str(body)?))
            .collect()
    }
}
