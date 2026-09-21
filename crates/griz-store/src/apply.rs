//! Apply a stored plan, or undo an earlier operation.

use crate::{
    Operation, OperationKind, OperationState, Store, StoreError,
    execute::{Target, read_current},
};
use griz_core::{Confidence, FileChange, MergeOutcome, three_way};
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
        let changes = self.plan_changes(&record)?;
        let paths: Vec<PathBuf> = changes.iter().map(|change| change.path.clone()).collect();
        let _locks = self.lock_paths(&paths)?;
        let mut targets = Vec::with_capacity(changes.len());
        for change in &changes {
            match target_for(change, request.on_stale)? {
                Some(target) => targets.push(target),
                None => op.conflicts.push(change.path.clone()),
            }
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

    /// Restores the files an operation wrote, for files still exactly as it
    /// left them. Files changed since are left alone and listed as conflicts.
    ///
    /// # Errors
    /// Returns an error when the operation cannot be read or a write fails.
    pub fn undo(
        &self,
        id: &str,
        paths: &[PathBuf],
        purpose: &str,
    ) -> Result<Operation, StoreError> {
        let original = self.operation(id)?;
        let mut op = Operation::new(OperationKind::Undo, purpose);
        op.undoes = Some(original.id.clone());
        if original.state != OperationState::Applied {
            return self.refuse(op, "only an applied operation can be undone");
        }
        let chosen: Vec<_> = original
            .files
            .iter()
            .filter(|file| paths.is_empty() || paths.contains(&file.path))
            .collect();
        let locked: Vec<PathBuf> = chosen.iter().map(|file| file.path.clone()).collect();
        let _locks = self.lock_paths(&locked)?;
        let mut targets = Vec::new();
        for file in chosen {
            match self.undo_target(file)? {
                Some(target) => targets.push(target),
                None => op.conflicts.push(file.path.clone()),
            }
        }
        if targets.is_empty() {
            return self.refuse(op, "no file is still as the operation left it");
        }
        self.execute(&mut op, &targets)?;
        Ok(op)
    }

    /// The restore for one file, or `None` when it changed since the write.
    fn undo_target(&self, file: &crate::FileWrite) -> Result<Option<Target>, StoreError> {
        let (current, current_hash) = read_current(&file.path)?;
        if current_hash != file.after_hash {
            return Ok(None);
        }
        let text = file
            .before_hash
            .as_deref()
            .map(|hash| self.get_blob(hash))
            .transpose()?;
        Ok(Some(Target {
            path: file.path.clone(),
            current_hash,
            current,
            text,
            merged: false,
        }))
    }

    fn refuse(&self, mut op: Operation, reason: &str) -> Result<Operation, StoreError> {
        op.state = OperationState::Failed;
        op.reason = Some(reason.to_string());
        self.save_operation(&op)?;
        Ok(op)
    }
}

/// The write for one planned file, or `None` when it is stale and cannot merge.
fn target_for(change: &FileChange, on_stale: OnStale) -> Result<Option<Target>, StoreError> {
    let (current, current_hash) = read_current(&change.path)?;
    let target = |text: Option<String>, merged| Target {
        path: change.path.clone(),
        current_hash: current_hash.clone(),
        current: current.clone(),
        text,
        merged,
    };
    if current_hash == change.before_hash {
        return Ok(Some(target(change.after.clone(), false)));
    }
    if on_stale == OnStale::Refuse {
        return Ok(None);
    }
    let (Some(base), Some(planned), Some(now)) = (&change.before, &change.after, &current) else {
        return Ok(None);
    };
    Ok(match three_way(base, planned, now) {
        MergeOutcome::Clean { text } => Some(target(Some(text), true)),
        MergeOutcome::Conflict => None,
    })
}
