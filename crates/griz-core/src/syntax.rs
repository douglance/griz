//! Parse facts: whether a planned file's before and after text parses
//! cleanly, and where the first newly introduced error sits.
//!
//! Fact only. This never refuses a plan; it only reports what a parse of the
//! planned text sees, for an enabled language.

use crate::{FileChange, structural};
use ast_grep_language::{LanguageExt, SupportLang};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// 1-based line and column of a syntax error or missing node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SyntaxPosition {
    /// 1-based line.
    pub line: usize,
    /// 1-based column in characters.
    pub column: usize,
}

/// Parse facts for one file's before and after text, in an enabled language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileSyntax {
    /// Language the file was parsed as.
    pub language: String,
    /// Error and missing nodes found parsing the text before the plan.
    pub before_errors: usize,
    /// Error and missing nodes found parsing the text after the plan.
    pub after_errors: usize,
    /// Position of the first error node introduced by the plan, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_new_error: Option<SyntaxPosition>,
}

/// A plan's overall syntax verdict, derived from every file's [`FileSyntax`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanSyntax {
    /// No changed file was in an enabled language.
    #[default]
    Unknown,
    /// Every assessed file parses with no more errors than before.
    Clean,
    /// At least one file has more errors after the plan than before.
    IntroducedErrors,
    /// No file gained errors, but at least one already had some.
    PreexistingErrors,
}

/// Parse facts for every changed file in an enabled language.
#[must_use]
pub fn annotate(files: &[FileChange]) -> BTreeMap<PathBuf, FileSyntax> {
    files
        .iter()
        .filter_map(|file| Some((file.path.clone(), file_syntax(file)?)))
        .collect()
}

/// The plan-wide verdict over already-computed per-file facts.
#[must_use]
pub fn plan_syntax(facts: &BTreeMap<PathBuf, FileSyntax>) -> PlanSyntax {
    if facts.is_empty() {
        return PlanSyntax::Unknown;
    }
    if facts
        .values()
        .any(|fact| fact.after_errors > fact.before_errors)
    {
        return PlanSyntax::IntroducedErrors;
    }
    if facts.values().any(|fact| fact.before_errors > 0) {
        return PlanSyntax::PreexistingErrors;
    }
    PlanSyntax::Clean
}

fn file_syntax(file: &FileChange) -> Option<FileSyntax> {
    let language = structural::enabled_language(&file.path)?;
    let before_errors = file
        .before
        .as_deref()
        .map_or(0, |text| errors(language, text).0);
    let after = file.after.as_deref().map(|text| errors(language, text));
    let after_errors = after.as_ref().map_or(0, |(count, _)| *count);
    let first_new_error = (after_errors > before_errors)
        .then(|| after.and_then(|(_, pos)| pos))
        .flatten();
    Some(FileSyntax {
        language: structural::language_name(language).to_string(),
        before_errors,
        after_errors,
        first_new_error,
    })
}

/// Error and missing node count in `text`, and the first such node's
/// 1-based position, when `language` parses it.
fn errors(language: SupportLang, text: &str) -> (usize, Option<SyntaxPosition>) {
    let tree = language.ast_grep(text);
    let mut count = 0;
    let mut first = None;
    for node in tree.root().dfs() {
        if !(node.is_error() || node.is_missing()) {
            continue;
        }
        count += 1;
        if first.is_none() {
            let pos = node.start_pos();
            first = Some(SyntaxPosition {
                line: pos.line() + 1,
                column: pos.column(&node) + 1,
            });
        }
    }
    (count, first)
}
