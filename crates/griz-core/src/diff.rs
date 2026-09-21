//! Renders planned changes as unified diffs.

use crate::{ChangeKind, FileChange};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use similar::TextDiff;
use std::path::{Path, PathBuf};

/// The diff of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileDiff {
    /// File path.
    pub path: PathBuf,
    /// What happens to it.
    pub kind: ChangeKind,
    /// Lines added.
    pub added: usize,
    /// Lines removed.
    pub removed: usize,
    /// Unified diff text with three lines of context.
    pub text: String,
}

/// Renders every change as a unified diff, naming files relative to `base`
/// when they are inside it.
#[must_use]
pub fn render_diff(changes: &[FileChange], base: &Path) -> Vec<FileDiff> {
    changes
        .iter()
        .map(|change| render_one(change, base))
        .collect()
}

fn render_one(change: &FileChange, base: &Path) -> FileDiff {
    let before = change.before.as_deref().unwrap_or_default();
    let after = change.after.as_deref().unwrap_or_default();
    let shown = change.path.strip_prefix(base).unwrap_or(&change.path);
    let name = shown.display().to_string();
    let name = name.trim_start_matches('/');
    let old_header = match change.kind {
        ChangeKind::Create => "/dev/null".to_string(),
        _ => format!("a/{name}"),
    };
    let new_header = match change.kind {
        ChangeKind::Delete => "/dev/null".to_string(),
        _ => format!("b/{name}"),
    };
    let diff = TextDiff::from_lines(before, after);
    let (added, removed) =
        diff.iter_all_changes()
            .fold((0, 0), |(add, del), line| match line.tag() {
                similar::ChangeTag::Insert => (add + 1, del),
                similar::ChangeTag::Delete => (add, del + 1),
                similar::ChangeTag::Equal => (add, del),
            });
    let text = diff
        .unified_diff()
        .context_radius(3)
        .header(&old_header, &new_header)
        .to_string();
    FileDiff {
        path: change.path.clone(),
        kind: change.kind,
        added,
        removed,
        text,
    }
}
