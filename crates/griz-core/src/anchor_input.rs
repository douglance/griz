//! Accepts an anchor as a plain string or as a full object.

use crate::Anchor;
use schemars::JsonSchema;
use serde::Deserialize;

/// How callers may write an anchor.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AnchorInput {
    /// Shorthand for `{ text }`.
    Text(String),
    /// The full form.
    Full {
        /// Text to find.
        text: String,
        /// Only consider matches after the first occurrence of this text.
        #[serde(default)]
        after: Option<String>,
        /// Match whole lines only.
        #[serde(default)]
        whole_lines: bool,
    },
}

impl From<AnchorInput> for Anchor {
    fn from(input: AnchorInput) -> Self {
        match input {
            AnchorInput::Text(text) => Self {
                text,
                after: None,
                whole_lines: false,
            },
            AnchorInput::Full {
                text,
                after,
                whole_lines,
            } => Self {
                text,
                after,
                whole_lines,
            },
        }
    }
}
