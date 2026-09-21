//! Quality policy checker for the griz workspace.

mod deps;
mod files;
mod lint_attrs;
mod manifest;
mod syntax;

use anyhow::{Context, Result, bail};
use std::{
    fmt,
    path::{Path, PathBuf},
};

const MAX_FILE_LINES: usize = 300;
const MAX_BLOCK_LINES: usize = 60;

/// One quality policy failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    path: PathBuf,
    line: usize,
    message: String,
}

impl Violation {
    fn new(path: impl Into<PathBuf>, line: usize, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.path.display(), self.message)
        } else {
            write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
        }
    }
}

/// One package manifest included in the workspace policy surface.
#[derive(Debug, Clone)]
pub struct Package {
    /// Cargo package name.
    pub name: String,
    /// Absolute path to the package manifest.
    pub manifest_path: PathBuf,
    /// Absolute package root directory.
    pub root: PathBuf,
}

/// Input for one policy check run.
#[derive(Debug, Clone)]
pub struct CheckInput {
    root: PathBuf,
    rust_files: Option<Vec<PathBuf>>,
}

impl CheckInput {
    /// Create a check input rooted at a workspace directory.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            rust_files: None,
        }
    }

    /// Override Rust files for tests or embedded callers.
    #[must_use]
    pub fn with_rust_files(mut self, rust_files: Vec<PathBuf>) -> Self {
        self.rust_files = Some(rust_files);
        self
    }
}

/// Run an xtask command.
///
/// # Errors
///
/// Returns an error when the command is unknown or the selected check reports violations.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<()> {
    let args: Vec<_> = args.into_iter().collect();
    if args.first().is_some_and(|arg| arg == "check") || args.is_empty() {
        let root = std::env::current_dir().context("resolve current directory")?;
        let violations = check(CheckInput::new(root))?;
        if violations.is_empty() {
            println!("quality policy passed");
            return Ok(());
        }
        for violation in &violations {
            eprintln!("{violation}");
        }
        bail!(
            "quality policy failed with {} violation(s)",
            violations.len()
        );
    }
    bail!("unknown xtask command: {}", args.join(" "));
}

/// Check the configured workspace and return every policy violation.
///
/// # Errors
///
/// Returns an error when manifests or Rust files cannot be read or parsed.
pub fn check(input: CheckInput) -> Result<Vec<Violation>> {
    let root = input.root.canonicalize().unwrap_or(input.root);
    let packages = manifest::workspace_packages(&root)?;
    let rust_files = match input.rust_files {
        Some(files) => files,
        None => files::discovered_rust_files(&root)?,
    };
    let rust_files = files::policy_rust_files(&packages, rust_files);
    if rust_files.is_empty() {
        bail!("quality policy inspected zero Rust files");
    }
    let mut violations = Vec::new();
    violations.extend(manifest::check_manifests(&root, &packages)?);
    violations.extend(files::check_file_lengths(&root, &rust_files)?);
    violations.extend(syntax::check_blocks(&root, &rust_files)?);
    violations.extend(lint_attrs::check_lint_suppressions(&root, &rust_files)?);
    violations.extend(files::check_core_purity(&root, &packages, &rust_files)?);
    violations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.message.cmp(&right.message))
    });
    Ok(violations)
}

fn relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        error::Error,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_root(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("griz-xtask-{name}-{suffix}"));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    #[test]
    fn clean_fixture_passes() -> Result<(), Box<dyn Error>> {
        let root = temp_root("clean")?;
        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
members = ["app"]

[workspace.lints.rust]
warnings = "deny"
"#,
        )?;
        fs::create_dir_all(root.join("app/src"))?;
        fs::write(
            root.join("app/Cargo.toml"),
            r#"[package]
name = "app"
version = "0.1.0"
edition = "2021"

[lints]
workspace = true
"#,
        )?;
        fs::write(
            root.join("app/src/lib.rs"),
            "pub fn ok() -> usize {\n    1\n}\n",
        )?;
        let violations =
            check(CheckInput::new(&root).with_rust_files(vec![root.join("app/src/lib.rs")]))?;
        assert!(violations.is_empty(), "{violations:?}");
        Ok(())
    }
}
