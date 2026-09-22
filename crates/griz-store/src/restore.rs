//! Restore every applied operation from `since` through the newest, folded
//! into one new operation.

use crate::{
    FileWrite, Operation, OperationKind, Store, StoreError,
    apply::OnStale,
    journal::Restores,
    merge::{MergePair, Resolved, blob_text, record_resolved, resolve_target},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

impl Store {
    /// Restores every applied operation from `since` (inclusive) through the
    /// newest, as one new operation. Per file touched in the span, the write
    /// targets the text the first touching write found it in and guards on
    /// the text the last touching write left it in; a broken chain, or a file
    /// changed since, follows the same rules as `undo`. Narrow the span with
    /// `paths`; an empty span, or one only `paths` excludes, refuses.
    ///
    /// # Errors
    /// Returns `NotFound` when the starting operation does not exist, or an
    /// error when the span or a file cannot be read, or a write fails.
    pub fn restore_since(
        &self,
        since: &str,
        paths: &[PathBuf],
        on_stale: OnStale,
        purpose: &str,
    ) -> Result<Operation, StoreError> {
        self.operation(since)?;
        let span = self.operations_since(since)?;
        let mut op = Operation::new(OperationKind::Undo, purpose);
        op.restores = Some(Restores {
            since: since.to_string(),
            operations: span.iter().map(|entry| entry.id.clone()).collect(),
        });
        let locked: Vec<PathBuf> = chains_by_path(&span, paths).into_keys().collect();
        let _locks = self.lock_paths(&locked)?;
        let span = self.reload_operations(&span)?;
        let chains = chains_by_path(&span, paths);
        let mut targets = Vec::new();
        for (path, chain) in &chains {
            let resolved = self.resolve_chain(path, chain, on_stale)?;
            record_resolved(&mut op, &mut targets, path.clone(), resolved);
        }
        let broken: Vec<String> = chains
            .iter()
            .filter(|(_, chain)| !unbroken(chain))
            .map(|(path, _)| path.display().to_string())
            .collect();
        if !broken.is_empty() {
            let reason = format!(
                "{} changed outside griz between operations in the span, which a merge cannot bridge; narrow with paths to leave them out",
                broken.join(", ")
            );
            return self.refuse(op, &reason);
        }
        if !op.conflicts.is_empty() {
            return self.refuse(op, "files changed since the span's last write; narrow with paths or apply with on_stale merge");
        }
        if targets.is_empty() {
            return self.refuse(op, "no file needed restoring in this span");
        }
        self.execute(&mut op, &targets)?;
        Ok(op)
    }

    /// The restore candidate for one file's chain of writes across the span:
    /// an unbroken chain merges like `undo`, from the last write's text back
    /// to the first's; a broken chain is left alone without a merge attempt.
    fn resolve_chain(
        &self,
        path: &Path,
        chain: &[&FileWrite],
        on_stale: OnStale,
    ) -> Result<Resolved, StoreError> {
        if !unbroken(chain) {
            return Ok(Resolved::Stale);
        }
        let guard = chain
            .last()
            .ok_or_else(|| StoreError::Invalid("empty restore chain".into()))?;
        let target = chain
            .first()
            .ok_or_else(|| StoreError::Invalid("empty restore chain".into()))?;
        let base = blob_text(self, guard.after_hash.as_deref())?;
        let planned = blob_text(self, target.before_hash.as_deref())?;
        let pair = MergePair {
            base_hash: guard.after_hash.as_deref(),
            base: base.as_deref(),
            planned_hash: target.before_hash.as_deref(),
            planned: planned.as_deref(),
        };
        resolve_target(self, path, &pair, on_stale)
    }
}

/// Every file the span touches, mapped to its writes in order, narrowed to
/// `paths` when given.
fn chains_by_path<'a>(
    span: &'a [Operation],
    paths: &[PathBuf],
) -> BTreeMap<PathBuf, Vec<&'a FileWrite>> {
    let mut chains: BTreeMap<PathBuf, Vec<&FileWrite>> = BTreeMap::new();
    let touches = span
        .iter()
        .flat_map(|entry| &entry.files)
        .filter(|file| paths.is_empty() || paths.contains(&file.path));
    for file in touches {
        chains.entry(file.path.clone()).or_default().push(file);
    }
    chains
}

/// Whether consecutive writes in a chain hand off the same fingerprint.
fn unbroken(chain: &[&FileWrite]) -> bool {
    chain
        .windows(2)
        .all(|pair| pair[0].after_hash == pair[1].before_hash)
}
