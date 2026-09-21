//! Atomic file writes.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// Environment variable that stops the process at a named point, for crash
/// recovery tests.
pub const FAILPOINT_ENV: &str = "GRIZ_FAILPOINT";

/// Exits abruptly when the failpoint named `name` is armed.
pub fn failpoint(name: &str) {
    if std::env::var(FAILPOINT_ENV).is_ok_and(|armed| armed == name) {
        std::process::abort();
    }
}

/// Returns an error instead of aborting, for testing a graceful failure at a
/// named point instead of a process crash.
///
/// # Errors
/// Returns an error when the failpoint named `name` is armed.
pub fn failpoint_result(name: &str) -> std::io::Result<()> {
    if std::env::var(FAILPOINT_ENV).is_ok_and(|armed| armed == name) {
        return Err(std::io::Error::other(format!("failpoint: {name}")));
    }
    Ok(())
}

/// A temporary sibling of `path`, unique to this process.
#[must_use]
pub fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "griz".into(), |name| name.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.griz-{}.tmp", std::process::id()))
}

/// Writes `text` to a temporary sibling and syncs it, returning the sibling.
///
/// # Errors
/// Returns an error when the directory or file cannot be written.
pub fn stage(path: &Path, text: &str) -> std::io::Result<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_sibling(path);
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(temp)
}

/// Replaces `path` with `text` through a temporary sibling and a rename.
///
/// # Errors
/// Returns an error when the file cannot be written or renamed.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = stage(path, text)?;
    std::fs::rename(temp, path)
}
