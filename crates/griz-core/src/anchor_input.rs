//! Accepts an anchor as a plain string or as a checked full object.

use crate::Anchor;
use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor, value::MapAccessDeserializer},
};

/// How callers may write an anchor.
#[derive(JsonSchema)]
#[serde(untagged)]
pub enum AnchorInput {
    /// Shorthand for `{ text }`.
    Text(String),
    /// The full form.
    Full(AnchorFields),
}

/// Named anchor fields shared by deserialization and the published schema.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnchorFields {
    /// Text to find.
    text: String,
    /// Only consider matches after the first occurrence of this text.
    #[serde(default)]
    after: Option<String>,
    /// Match whole lines only.
    #[serde(default)]
    whole_lines: bool,
}

impl<'de> Deserialize<'de> for AnchorInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(AnchorVisitor)
    }
}

struct AnchorVisitor;

impl<'de> Visitor<'de> for AnchorVisitor {
    type Value = AnchorInput;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a string or an anchor object")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(AnchorInput::Text(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(AnchorInput::Text(value))
    }

    fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        AnchorFields::deserialize(MapAccessDeserializer::new(map)).map(AnchorInput::Full)
    }
}

impl From<AnchorInput> for Anchor {
    fn from(input: AnchorInput) -> Self {
        match input {
            AnchorInput::Text(text) => Self {
                text,
                after: None,
                whole_lines: false,
            },
            AnchorInput::Full(fields) => Self {
                text: fields.text,
                after: fields.after,
                whole_lines: fields.whole_lines,
            },
        }
    }
}
