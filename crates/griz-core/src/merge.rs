//! Three-way merge for files that changed after they were planned.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Result of merging a planned change onto a file that moved on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MergeOutcome {
    /// Both sides combined without overlap.
    Clean {
        /// Merged text.
        text: String,
    },
    /// The plan and the newer text changed the same lines.
    Conflict,
}

/// Merges the plan's change (`base` to `planned`) onto `current`.
#[must_use]
pub fn three_way(base: &str, planned: &str, current: &str) -> MergeOutcome {
    match diffy::merge(base, planned, current) {
        Ok(text) => MergeOutcome::Clean { text },
        Err(_) => MergeOutcome::Conflict,
    }
}
