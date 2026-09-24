//! Apply a stored plan, or undo an earlier operation.

use crate::{
    FileWrite, Operation, OperationKind, OperationState, Store, StoreError,
    merge::{MergePair, Resolved, blob_text, record_resolved, resolve_target},
    plans::FileRecord,
};
use griz_core::Confidence;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What to do when a file changed after it was planned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnStale {
    /// Write nothing.
    #[default]
    Refuse,
    /// Merge the planned change onto the newer text; write nothing on conflict.
    Merge,
}

/// How to apply a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyRequest {
    /// Plan to apply.
    pub plan: String,
    /// Least confidence every edit must have.
    pub min_confidence: Confidence,
    /// Stale-file policy.
    pub on_stale: OnStale,
    /// Why the caller asked.
    pub purpose: String,
}

/// How to undo an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRequest {
    /// Operation to undo.
    pub operation: String,
    /// The caller's tree. A file the operation wrote outside it is never
    /// touched: the undo is refused instead of writing there.
    pub root: PathBuf,
    /// Restore only these files; empty restores every file the operation wrote.
    pub paths: Vec<PathBuf>,
    /// Stale-file policy for files changed since the operation wrote them.
    pub on_stale: OnStale,
    /// Why the caller asked.
    pub purpose: String,
}

impl Store {
    /// Applies a stored plan all-or-nothing.
    ///
    /// The returned operation is `applied`, or `failed` with a reason and no
    /// file changed.
    ///
    /// # Errors
    /// Returns an error when the plan cannot be read or a write fails.
    pub fn apply(&self, request: &ApplyRequest) -> Result<Operation, StoreError> {
        let record = self.plan(&request.plan)?;
        let mut op = Operation::new(OperationKind::Apply, &request.purpose);
        op.plan = Some(record.id.clone());
        if !record.problems.is_empty() {
            return self.refuse(
                op,
                "the plan has problems; fix the operations and plan again",
            );
        }
        if record.confidence < request.min_confidence {
            return self.refuse(op, "the plan has edits below the minimum confidence; select the confident edits or lower the minimum");
        }
        let paths: Vec<PathBuf> = record.files.iter().map(|file| file.path.clone()).collect();
        let _locks = self.lock_paths(&paths)?;
        let mut targets = Vec::with_capacity(record.files.len());
        for change in &record.files {
            let resolved = self.resolve_change(change, request.on_stale)?;
            record_resolved(&mut op, &mut targets, change.path.clone(), resolved);
        }
        if !op.conflicts.is_empty() {
            return self.refuse(
                op,
                "files changed after planning; plan again or apply with on_stale merge",
            );
        }
        self.execute(&mut op, &targets)?;
        Ok(op)
    }

    fn resolve_change(
        &self,
        change: &FileRecord,
        on_stale: OnStale,
    ) -> Result<Resolved, StoreError> {
        let pair = MergePair {
            base_hash: change.before_hash.as_deref(),
            base: blob_text(self, change.before_hash.as_deref())?,
            planned_hash: change.after_hash.as_deref(),
            planned: blob_text(self, change.after_hash.as_deref())?,
        };
        resolve_target(self, &change.path, pair, on_stale)
    }

    /// Restores the files an operation wrote, for files still exactly as it
    /// left them, or merged onto a newer text when `request.on_stale` allows
    /// it. Files left changed are left alone and listed as conflicts.
    ///
    /// One store holds every repository's history, so a file the operation
    /// wrote outside `request.root` is never a candidate: the undo refuses
    /// rather than write outside the caller's tree.
    ///
    /// # Errors
    /// Returns an error when the operation cannot be read or a write fails.
    pub fn undo(&self, request: &UndoRequest) -> Result<Operation, StoreError> {
        let original = self.operation(&request.operation)?;
        let mut op = Operation::new(OperationKind::Undo, &request.purpose);
        op.undoes = Some(original.id.clone());
        if original.state != OperationState::Applied {
            return self.refuse(op, "only an applied operation can be undone");
        }
        let chosen: Vec<&FileWrite> = original
            .files
            .iter()
            .filter(|file| request.paths.is_empty() || request.paths.contains(&file.path))
            .collect();
        let outside: Vec<String> = chosen
            .iter()
            .filter(|file| !file.path.starts_with(&request.root))
            .map(|file| file.path.display().to_string())
            .collect();
        if !outside.is_empty() {
            let reason = format!(
                "{} lie outside root {}; undo never writes outside the caller's tree",
                outside.join(", "),
                request.root.display()
            );
            return self.refuse(op, &reason);
        }
        let locked: Vec<PathBuf> = chosen.iter().map(|file| file.path.clone()).collect();
        let _locks = self.lock_paths(&locked)?;
        let original = self.operation(&request.operation)?;
        let chosen = original
            .files
            .iter()
            .filter(|file| request.paths.is_empty() || request.paths.contains(&file.path));
        let mut targets = Vec::new();
        for file in chosen {
            let resolved = self.undo_target(file, request.on_stale)?;
            record_resolved(&mut op, &mut targets, file.path.clone(), resolved);
        }
        if targets.is_empty() {
            return self.refuse(op, "no file is still as the operation left it");
        }
        self.execute(&mut op, &targets)?;
        Ok(op)
    }

    /// The restore candidate for one file an operation wrote: the merge
    /// undoes, from the text it left (`after`) back to the text it replaced
    /// (`before`).
    fn undo_target(&self, file: &FileWrite, on_stale: OnStale) -> Result<Resolved, StoreError> {
        let pair = MergePair {
            base_hash: file.after_hash.as_deref(),
            base: blob_text(self, file.after_hash.as_deref())?,
            planned_hash: file.before_hash.as_deref(),
            planned: blob_text(self, file.before_hash.as_deref())?,
        };
        resolve_target(self, &file.path, pair, on_stale)
    }

    /// Marks `op` failed with `reason`, saves it, and hands it back.
    pub(crate) fn refuse(&self, mut op: Operation, reason: &str) -> Result<Operation, StoreError> {
        op.state = OperationState::Failed;
        op.reason = Some(reason.to_string());
        self.save_operation(&op)?;
        Ok(op)
    }
}
