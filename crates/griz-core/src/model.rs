//! Values that cross the engine boundary.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

use crate::syntax::{FileSyntax, PlanSyntax};

use crate::pattern_locator::PatternLocator;
pub use crate::problem::{Problem, ProblemKind, Window};

/// A half-open byte range `[start, end)` in a file's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ByteRange {
    /// First byte of the range.
    pub start: usize,
    /// One past the last byte of the range.
    pub end: usize,
}

/// Which matches of an anchor an operation targets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Occurrence {
    /// Exactly one match; two or more is an error listing every candidate.
    #[default]
    Unique,
    /// Every match.
    All,
    /// The Nth match, counting from 1.
    Nth(usize),
}

/// Text that locates an edit inside a file. A plain string is accepted as
/// shorthand for `{ text }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(from = "crate::anchor_input::AnchorInput")]
pub struct Anchor {
    /// Text to find.
    pub text: String,
    /// Only consider matches after the first occurrence of this text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Match whole lines only. Patch hunks set this.
    #[serde(default)]
    pub whole_lines: bool,
}

/// One requested change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// Replace anchored text, an exact byte range, or a structural pattern
    /// match.
    Replace {
        /// File to change.
        path: PathBuf,
        /// Text to locate, or exact old text when `range` is given.
        /// Required unless `range` or `pattern` is given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        find: Option<Anchor>,
        /// Exact byte range in the file as it was when found.
        /// Requires `expect_hash` or expected old `find` text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<ByteRange>,
        /// Structural pattern match, as an alternative to `find`/`range`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<PatternLocator>,
        /// Replacement text.
        replace: String,
        /// Which matches to replace.
        #[serde(default)]
        occurrence: Occurrence,
        /// With `pattern`, edit only this metavariable's range, e.g. `$NAME`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        /// Fingerprint the file must still have.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_hash: Option<String>,
    },
    /// Insert text before or after a unique anchor.
    Insert {
        /// File to change.
        path: PathBuf,
        /// Text to locate.
        anchor: Anchor,
        /// Insert after the anchor instead of before it.
        #[serde(default)]
        after: bool,
        /// Text to insert.
        text: String,
        /// Fingerprint the file must still have.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_hash: Option<String>,
    },
    /// Create a file, which must not exist unless `overwrite` is set.
    Create {
        /// File to create.
        path: PathBuf,
        /// Full contents.
        text: String,
        /// Replace the file when it already exists, instead of refusing.
        /// Replacing a whole file needs no range and no byte count.
        #[serde(default)]
        overwrite: bool,
        /// Fingerprint the original file must still have; absence is stale.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_hash: Option<String>,
    },
    /// Delete a file that must exist.
    Delete {
        /// File to delete.
        path: PathBuf,
        /// Fingerprint the file must still have.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_hash: Option<String>,
    },
    /// Move a file to a path that must not exist yet.
    Move {
        /// File to move.
        path: PathBuf,
        /// Destination.
        to: PathBuf,
        /// Fingerprint the file must still have.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_hash: Option<String>,
    },
}

impl Op {
    /// The file this operation starts from.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::Replace { path, .. }
            | Self::Insert { path, .. }
            | Self::Create { path, .. }
            | Self::Delete { path, .. }
            | Self::Move { path, .. } => path,
        }
    }
}

/// How sure the engine is that an edit landed where the caller meant.
///
/// Ordered: `Maybe` is below `Machine`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Located by tolerant matching; a person or program should look first.
    Maybe,
    /// Located exactly; safe to apply without review.
    Machine,
}

/// The matching rung that located an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rung {
    /// Byte range supplied by the caller.
    Range,
    /// Exact text.
    Exact,
    /// Lines equal after trimming trailing whitespace.
    TrailingWhitespace,
    /// Lines equal after trimming both ends.
    Trimmed,
    /// Lines equal after removing their shared indentation.
    Indentation,
    /// Whole-file operation such as create, delete, or move.
    File,
    /// A structural pattern match: exact identity on the parse tree, never tolerant.
    Pattern,
}

impl Rung {
    /// Only exact rungs are safe to apply unattended.
    #[must_use]
    pub fn confidence(self) -> Confidence {
        match self {
            Self::Range | Self::Exact | Self::File | Self::Pattern => Confidence::Machine,
            Self::TrailingWhitespace | Self::Trimmed | Self::Indentation => Confidence::Maybe,
        }
    }
}

/// One change the plan makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Edit {
    /// Stable identifier: `e<op>` or `e<op>.<n>` for repeated matches.
    pub id: String,
    /// Index of the operation that produced this edit.
    pub op: usize,
    /// File the edit lands in.
    pub path: PathBuf,
    /// 1-based line where the edit starts, 0 for whole-file edits.
    pub line: usize,
    /// Rung that located it.
    pub rung: Rung,
    /// Confidence derived from the rung.
    pub confidence: Confidence,
}

/// What a file change does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Existing file rewritten.
    Modify,
    /// New file.
    Create,
    /// File removed.
    Delete,
}

/// The planned before and after state of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileChange {
    /// File path.
    pub path: PathBuf,
    /// What happens to it.
    pub kind: ChangeKind,
    /// Text the plan was computed against; absent for a new file.
    pub before: Option<String>,
    /// Fingerprint of `before`.
    pub before_hash: Option<String>,
    /// Text after the plan; absent for a deleted file.
    pub after: Option<String>,
    /// Fingerprint of `after`.
    pub after_hash: Option<String>,
}

/// The complete, unwritten result of a set of operations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    /// Every changed file, sorted by path.
    pub files: Vec<FileChange>,
    /// Original text of read files whose final state is unchanged; `None` records absence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unchanged_inputs: BTreeMap<PathBuf, Option<String>>,
    /// Every edit, in operation order.
    pub edits: Vec<Edit>,
    /// Every operation that could not be planned. A plan with problems must
    /// never be applied.
    pub problems: Vec<Problem>,
    /// Overall syntax verdict, over every changed file's [`FileSyntax`].
    /// Fact only: never `problems`, never refuses.
    #[serde(default)]
    pub syntax: PlanSyntax,
    /// Parse facts for each changed file in an enabled language.
    #[serde(default)]
    pub file_syntax: BTreeMap<PathBuf, FileSyntax>,
}

impl Plan {
    /// Lowest confidence of any edit, or `Machine` for an empty plan.
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        self.edits
            .iter()
            .map(|edit| edit.confidence)
            .min()
            .unwrap_or(Confidence::Machine)
    }
}
