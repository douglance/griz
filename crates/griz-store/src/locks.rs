//! Cross-process locks, one per file path.

use crate::{Store, StoreError};
use griz_core::content_hash;
use std::{fs::File, path::PathBuf};

/// Held locks; dropping them releases every path.
#[derive(Debug)]
pub struct PathLocks {
    _files: Vec<File>,
}

impl Store {
    /// Blocks until every path is locked. Paths are locked in sorted order, so
    /// two callers with overlapping sets cannot deadlock.
    ///
    /// # Errors
    /// Returns an error when a lock file cannot be opened or locked.
    pub fn lock_paths(&self, paths: &[PathBuf]) -> Result<PathLocks, StoreError> {
        let mut resolver = crate::PathResolver::default();
        let mut keys: Vec<String> = paths
            .iter()
            .map(|path| {
                resolver
                    .resolve(path)
                    .map(|path| content_hash(&path.to_string_lossy()))
            })
            .collect::<Result<_, _>>()?;
        keys.sort();
        keys.dedup();
        let dir = self.home().join("locks");
        std::fs::create_dir_all(&dir)?;
        let files = keys
            .iter()
            .map(|key| {
                let file = File::options()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(dir.join(format!("{key}.lock")))?;
                file.lock()?;
                Ok(file)
            })
            .collect::<Result<_, StoreError>>()?;
        Ok(PathLocks { _files: files })
    }
}
