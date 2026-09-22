//! Request context shared by every command: the root that relative paths
//! resolve from, the store, and error shaping.

use griz_core::Op;
use griz_store::{Store, StoreError};
use incurs::command::TypedResult;
use std::path::{Component, Path, PathBuf};

/// A command failure with a stable code.
#[derive(Debug)]
pub struct CmdError {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Human-readable explanation.
    pub message: String,
}

impl CmdError {
    /// A validation failure.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "VALIDATION_ERROR",
            message: message.into(),
        }
    }

    /// Converts into the command result.
    pub fn result<T>(self) -> TypedResult<T> {
        TypedResult::error(self.code, self.message)
    }
}

impl From<StoreError> for CmdError {
    fn from(error: StoreError) -> Self {
        let code = match error {
            StoreError::NotFound(_) => "NOT_FOUND",
            StoreError::Invalid(_) => "VALIDATION_ERROR",
            _ => "STORE_ERROR",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

/// The directory relative paths resolve from: the option, else the invoking
/// process's current directory.
///
/// # Errors
/// Returns an error when the current directory or a traversed parent is unavailable.
pub fn root(explicit: Option<&str>) -> Result<PathBuf, CmdError> {
    normalize(&input_root(explicit)?)
}

/// Anchors a request root without resolving filesystem-dependent parent paths.
///
/// # Errors
/// Returns an error when the current directory is unavailable.
pub fn input_root(explicit: Option<&str>) -> Result<PathBuf, CmdError> {
    let cwd = std::env::current_dir().map_err(|e| CmdError::invalid(e.to_string()))?;
    Ok(match explicit {
        Some(root) => input_path(&cwd, Path::new(root)),
        None => cwd,
    })
}

/// Anchors a submitted path while retaining parent traversal in its identity.
#[must_use]
pub fn input_path(root: &Path, path: &Path) -> PathBuf {
    root.join(path).components().collect()
}

/// Resolves already anchored paths when a new mutation executes.
///
/// # Errors
/// Returns an error when a parent traversal cannot resolve its directory.
pub fn resolve_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, CmdError> {
    paths.iter().map(|path| normalize(path)).collect()
}

/// Resolves `path` against `root`, following directory links before `..`.
///
/// # Errors
/// Returns an error when a parent traversal cannot resolve its directory.
pub fn resolve(root: &Path, path: &Path) -> Result<PathBuf, CmdError> {
    normalize(&root.join(path))
}

fn normalize(path: &Path) -> Result<PathBuf, CmdError> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => out = parent_directory(&out)?,
            other => out.push(other),
        }
    }
    Ok(out)
}

fn parent_directory(path: &Path) -> Result<PathBuf, CmdError> {
    let invalid =
        |error| CmdError::invalid(format!("resolving parent of {}: {error}", path.display()));
    let mut directory = std::fs::canonicalize(path).map_err(invalid)?;
    if !std::fs::metadata(&directory).map_err(invalid)?.is_dir() {
        return Err(CmdError::invalid(format!(
            "parent traversal requires a directory: {}",
            path.display()
        )));
    }
    directory.pop();
    Ok(directory)
}

/// Resolves every path in an operation against `root`.
///
/// # Errors
/// Returns an error when a parent traversal cannot resolve its directory.
pub fn resolve_op(root: &Path, mut op: Op) -> Result<Op, CmdError> {
    match &mut op {
        Op::Replace { path, .. }
        | Op::Insert { path, .. }
        | Op::Create { path, .. }
        | Op::Delete { path, .. } => *path = resolve(root, path)?,
        Op::Move { path, to, .. } => {
            *path = resolve(root, path)?;
            *to = resolve(root, to)?;
        }
    }
    Ok(op)
}

/// Runs blocking store work off the async runtime.
///
/// # Errors
/// Returns the work's error, or an error when the store cannot open.
pub async fn with_store<T: Send + 'static>(
    work: impl FnOnce(&Store) -> Result<T, CmdError> + Send + 'static,
) -> Result<T, CmdError> {
    tokio::task::spawn_blocking(move || work(&Store::open_default()?))
        .await
        .map_err(|error| CmdError {
            code: "INTERNAL_ERROR",
            message: error.to_string(),
        })?
}
