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

fn temp_sibling(path: &Path, batch: uuid::Uuid) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("griz")),
    );
    name.push(format!(".griz-{}.tmp", batch.simple()));
    path.with_file_name(name)
}

/// Owns a temporary file until rename, with best-effort cleanup on drop.
pub struct StagedFile {
    path: PathBuf,
    committed: bool,
}

impl StagedFile {
    /// Renames the staged contents into place.
    ///
    /// # Errors
    /// Returns the rename error; dropping the owner attempts temporary cleanup.
    pub fn commit(mut self, path: &Path) -> std::io::Result<()> {
        std::fs::rename(&self.path, path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Writes and syncs a temporary sibling, returning its cleanup owner.
///
/// # Errors
/// Returns an error when the directory or file cannot be written.
pub fn stage(path: &Path, text: &str) -> std::io::Result<StagedFile> {
    stage_with_id(path, text, uuid::Uuid::now_v7())
}

pub(crate) fn stage_with_id(
    path: &Path,
    text: &str,
    batch: uuid::Uuid,
) -> std::io::Result<StagedFile> {
    let permissions = existing_permissions(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_sibling(path, batch);
    let mut file =
        create_staged(&temp, permissions.as_ref()).map_err(|error| stage_error(path, error))?;
    let staged = StagedFile {
        path: temp,
        committed: false,
    };
    failpoint("after_stage_create");
    let result = write_staged(&mut file, text, permissions);
    drop(file);
    result?;
    Ok(staged)
}

fn stage_error(path: &Path, error: std::io::Error) -> std::io::Error {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        return std::io::Error::new(
            error.kind(),
            format!(
                "staging collision for {}; check destination names",
                path.display()
            ),
        );
    }
    error
}

fn write_staged(
    file: &mut std::fs::File,
    text: &str,
    permissions: Option<std::fs::Permissions>,
) -> std::io::Result<()> {
    file.write_all(text.as_bytes())?;
    failpoint_result("after_stage_write")?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    file.sync_all()
}

fn existing_permissions(path: &Path) -> std::io::Result<Option<std::fs::Permissions>> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn create_staged(
    path: &Path,
    permissions: Option<&std::fs::Permissions>,
) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(permissions) = permissions {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(permissions.mode());
    }
    #[cfg(not(unix))]
    let _ = permissions;
    options.open(path)
}

/// Replaces `path` with `text` through a temporary sibling and a rename.
///
/// # Errors
/// Returns an error when the file cannot be written or renamed.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    stage(path, text)?.commit(path)
}
