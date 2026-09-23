//! The `pattern` locator: an alternative to `find`/`range` that locates a
//! [`crate::Op::Replace`] by syntax shape instead of by text.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A structural pattern that locates an edit inside one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternLocator {
    /// Structural pattern, e.g. `foo($A, $$$REST)`.
    pub pattern: String,
    /// Language to parse with; defaults to the file's extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}
