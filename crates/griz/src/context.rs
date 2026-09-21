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
/// Returns an error when the current directory is unavailable.
pub fn root(explicit: Option<&str>) -> Result<PathBuf, CmdError> {
    let cwd = std::env::current_dir().map_err(|e| CmdError::invalid(e.to_string()))?;
    Ok(match explicit {
        Some(root) => normalize(&cwd.join(root)),
        None => cwd,
    })
}

/// Resolves `path` against `root` and removes `.` and `..` segments.
#[must_use]
pub fn resolve(root: &Path, path: &Path) -> PathBuf {
    normalize(&root.join(path))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Resolves every path in an operation against `root`.
#[must_use]
pub fn resolve_op(root: &Path, mut op: Op) -> Op {
    match &mut op {
        Op::Replace { path, .. }
        | Op::Insert { path, .. }
        | Op::Create { path, .. }
        | Op::Delete { path, .. } => *path = resolve(root, path),
        Op::Move { path, to, .. } => {
            *path = resolve(root, path);
            *to = resolve(root, to);
        }
    }
    op
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
