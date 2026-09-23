//! Journaled, all-or-nothing file writes shared by apply, undo, and recovery.

use crate::{
    FileWrite, Operation, OperationState, Store, StoreError,
    staging::stage_all,
    write::{StagedFile, failpoint, failpoint_result},
};
use std::path::{Path, PathBuf};

/// A file's intended end state.
pub struct Target {
    /// File path.
    pub path: PathBuf,
    /// Fingerprint on disk now; absent when the file does not exist.
    pub current_hash: Option<String>,
    /// Text on disk now, kept so the write can be undone.
    pub current: Option<String>,
    /// Text to write; absent to delete.
    pub text: Option<String>,
    /// Produced by a three-way merge.
    pub merged: bool,
}

impl Store {
    /// Stages every write as a temporary sibling, journals `op` as applying,
    /// renames each file into place, then marks it applied. A staging failure
    /// removes the temporaries and records the operation as failed, so the
    /// journal only ever says `applying` about writes that are ready to land.
    ///
    /// # Errors
    /// Returns an error when staging or renaming fails. After a rename
    /// failure the journal keeps the operation `applying` for recovery.
    pub fn execute(&self, op: &mut Operation, targets: &[Target]) -> Result<(), StoreError> {
        validate_destinations(targets)?;
        op.files = targets
            .iter()
            .map(|target| self.file_write(target))
            .collect::<Result<_, _>>()?;
        let staged = match stage_all(targets) {
            Ok(staged) => staged,
            Err(error) => {
                op.state = OperationState::Failed;
                op.reason = Some(format!("could not stage writes: {error}"));
                self.save_operation(op)?;
                return Err(error.into());
            }
        };
        op.state = OperationState::Applying;
        self.save_operation(op)?;
        failpoint("after_journal");
        for (index, (target, temp)) in targets.iter().zip(staged).enumerate() {
            commit(&target.path, temp)?;
            failpoint_after(index);
        }
        op.state = OperationState::Applied;
        self.save_operation(op)
    }

    fn file_write(&self, target: &Target) -> Result<FileWrite, StoreError> {
        if let Some(text) = &target.current {
            self.put_blob(text)?;
        }
        let after_hash = target
            .text
            .as_deref()
            .map(|text| self.put_blob(text))
            .transpose()?;
        Ok(FileWrite {
            path: target.path.clone(),
            before_hash: target.current_hash.clone(),
            after_hash,
            merged: target.merged,
        })
    }
}

fn validate_destinations(targets: &[Target]) -> Result<(), StoreError> {
    let mut paths = std::collections::BTreeSet::new();
    let mut resolver = crate::PathResolver::default();
    for target in targets {
        let entry = resolver.resolve(&target.path)?;
        if crate::paths::file_link(&entry)?.is_some() {
            return Err(StoreError::Invalid(
                "planned path names a file symlink; plan again to record its target".into(),
            ));
        }
        if !paths.insert(resolver.resolve_file(&entry)?) {
            return Err(StoreError::Invalid(
                "multiple planned paths refer to the same destination; plan again".into(),
            ));
        }
    }
    Ok(())
}

/// Stops after the first rename when the crash-recovery failpoint is armed.
fn failpoint_after(index: usize) {
    if index == 0 {
        failpoint("after_first_rename");
    }
}

/// Moves a staged file into place, or deletes the file when nothing is staged.
fn commit(path: &Path, staged: Option<StagedFile>) -> std::io::Result<()> {
    failpoint_result("before_commit")?;
    match staged {
        Some(temp) => temp.commit(path),
        None => match std::fs::remove_file(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    }
}

/// Reads a file's text and fingerprint, or `None` when it does not exist.
///
/// # Errors
/// Returns an error when the file exists but cannot be read.
pub fn read_current(path: &Path) -> Result<(Option<String>, Option<String>), StoreError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let hash = griz_core::content_hash(&text);
            Ok((Some(text), Some(hash)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((None, None)),
        Err(error) => Err(error.into()),
    }
}
