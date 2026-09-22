//! Absorb: fold later changes to an operation's files into the operation.
//!
//! A formatter run after `apply` rewrites files the operation wrote, and undo
//! then refuses them because they no longer match what was written. Absorbing
//! records each file's current text as the operation's result, so undo restores
//! the text from before the apply, formatter changes included.

use crate::{FileWrite, Operation, OperationState, Store, StoreError, execute::read_current};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// What an absorb did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Absorbed {
    /// The operation, with its absorbed results.
    pub operation: Operation,
    /// Files whose later changes were absorbed.
    pub absorbed: Vec<PathBuf>,
    /// Files that could not be absorbed because they were deleted or are gone.
    pub skipped: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileResult {
    Unchanged,
    Absorbed,
    Skipped,
}

impl Store {
    /// Records the current text of an applied operation's files as what it
    /// wrote. Only files the operation wrote are touched, and none is changed on
    /// disk.
    ///
    /// # Errors
    /// Returns an error when the operation or a file cannot be read, or the
    /// operation is not applied.
    pub fn absorb(
        &self,
        id: &str,
        paths: &[PathBuf],
        purpose: &str,
    ) -> Result<Absorbed, StoreError> {
        let mut operation = self.operation(id)?;
        if operation.state != OperationState::Applied {
            return Err(StoreError::Invalid(
                "only an applied operation can absorb later changes".into(),
            ));
        }
        let written: Vec<PathBuf> = operation
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect();
        validate_paths(paths, &written)?;
        let _locks = self.lock_paths(&written)?;
        operation = self.operation(id)?;
        let chosen: BTreeSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let results: Vec<(PathBuf, FileResult)> = operation
            .files
            .iter_mut()
            .filter(|file| chosen.is_empty() || chosen.contains(file.path.as_path()))
            .map(|file| Ok((file.path.clone(), self.absorb_file(file)?)))
            .collect::<Result<_, StoreError>>()?;
        let pick = |wanted: FileResult| -> Vec<PathBuf> {
            results
                .iter()
                .filter(|(_, result)| *result == wanted)
                .map(|(path, _)| path.clone())
                .collect()
        };
        let (absorbed, skipped) = (pick(FileResult::Absorbed), pick(FileResult::Skipped));
        record(&mut operation, &absorbed, purpose);
        self.save_operation(&operation)?;
        Ok(Absorbed {
            operation,
            absorbed,
            skipped,
        })
    }

    fn absorb_file(&self, file: &mut FileWrite) -> Result<FileResult, StoreError> {
        let (current, current_hash) = read_current(&file.path)?;
        let (Some(text), Some(_)) = (current, &file.after_hash) else {
            return Ok(FileResult::Skipped);
        };
        if current_hash == file.after_hash {
            return Ok(FileResult::Unchanged);
        }
        file.after_hash = Some(self.put_blob(&text)?);
        Ok(FileResult::Absorbed)
    }
}

fn validate_paths(paths: &[PathBuf], written: &[PathBuf]) -> Result<(), StoreError> {
    if paths.is_empty() {
        return Ok(());
    }
    let known: BTreeSet<&Path> = written.iter().map(PathBuf::as_path).collect();
    if let Some(unknown) = paths.iter().find(|path| !known.contains(path.as_path())) {
        return Err(StoreError::Invalid(format!(
            "absorb: `{}` is not a file this operation wrote",
            unknown.display()
        )));
    }
    Ok(())
}

fn record(operation: &mut Operation, absorbed: &[PathBuf], purpose: &str) {
    if absorbed.is_empty() {
        return;
    }
    let mut known: BTreeSet<&Path> = operation.absorbed.iter().map(PathBuf::as_path).collect();
    let added: Vec<_> = absorbed
        .iter()
        .filter(|path| known.insert(path.as_path()))
        .cloned()
        .collect();
    operation.absorbed.extend(added);
    operation.absorb_purpose = Some(purpose.to_string());
}
