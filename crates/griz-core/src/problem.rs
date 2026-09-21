//! Why an operation could not be planned.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A line-numbered excerpt of real file text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Window {
    /// 1-based first line.
    pub line: usize,
    /// The excerpt.
    pub text: String,
}

/// Why an operation could not be planned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProblemKind {
    /// The anchor matched nothing; `nearest` is the closest real text.
    Missing {
        /// Closest window, when the file has any text.
        nearest: Option<Window>,
    },
    /// The anchor matched more than once.
    Ambiguous {
        /// 1-based line of every candidate.
        lines: Vec<usize>,
    },
    /// The file no longer has the fingerprint the caller expected.
    Stale {
        /// Fingerprint the caller expected.
        expected: String,
        /// Fingerprint found, absent when the file is gone.
        actual: Option<String>,
    },
    /// A create or move target already exists.
    Exists,
    /// The file does not exist.
    NotFound,
    /// A byte range does not fit the text or overlaps an earlier edit.
    BadRange,
    /// The operation is malformed.
    Invalid {
        /// What is wrong.
        message: String,
    },
}

/// One operation that could not be planned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Problem {
    /// Index of the operation.
    pub op: usize,
    /// File it targeted.
    pub path: PathBuf,
    /// What went wrong.
    #[serde(flatten)]
    pub kind: ProblemKind,
}
