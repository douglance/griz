//! Filters keep submitted spellings so older saved alias paths remain usable.

use crate::context::{CmdError, resolve_paths};
use griz_store::Store;
use std::{collections::BTreeSet, path::PathBuf};

/// Matches both current physical destinations and older submitted paths.
pub fn resolve(paths: &[PathBuf]) -> Result<Vec<PathBuf>, CmdError> {
    let mut resolved = resolve_paths(paths)?;
    resolved.extend_from_slice(paths);
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

/// Absorb validates each requested path, so choose its exact saved spelling.
pub fn absorb(store: &Store, id: &str, paths: &[PathBuf]) -> Result<Vec<PathBuf>, CmdError> {
    let resolved = resolve_paths(paths)?;
    if paths == resolved {
        return Ok(resolved);
    }
    let operation = store.operation(id)?;
    let known: BTreeSet<_> = operation.files.iter().map(|file| &file.path).collect();
    Ok(paths
        .iter()
        .zip(resolved)
        .map(|(raw, physical)| {
            if known.contains(raw) {
                raw.clone()
            } else {
                physical
            }
        })
        .collect())
}
