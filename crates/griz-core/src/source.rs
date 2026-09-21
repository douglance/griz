//! Where the engine reads file text from.

use std::{
    collections::BTreeMap,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// Reads the current text of a file, or `None` when it does not exist.
pub trait Source {
    /// Reads `path`.
    ///
    /// # Errors
    /// Returns a message when the file exists but cannot be read as UTF-8 text.
    fn read(&self, path: &Path) -> Result<Option<String>, String>;
}

/// Reads files from disk.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskSource;

impl Source for DiskSource {
    fn read(&self, path: &Path) -> Result<Option<String>, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }
}

/// Reads recorded file text, so a plan can be rebuilt against the exact bases
/// it was first computed from.
#[derive(Debug, Clone, Default)]
pub struct MapSource {
    files: BTreeMap<PathBuf, Option<String>>,
}

impl MapSource {
    /// Builds a source from recorded texts; `None` records an absent file.
    #[must_use]
    pub fn new(files: BTreeMap<PathBuf, Option<String>>) -> Self {
        Self { files }
    }
}

impl Source for MapSource {
    fn read(&self, path: &Path) -> Result<Option<String>, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("{}: not recorded in this plan", path.display()))
    }
}
