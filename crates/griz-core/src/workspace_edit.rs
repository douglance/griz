//! Converts an LSP `WorkspaceEdit` into griz [`Op`]s.
//!
//! Small serde structs of our own, not `lsp-types`: only the shapes griz
//! needs to turn edits from another tool into operations `plan` already
//! understands. `changes` and `documentChanges` combine `TextDocumentEdit`
//! (a file's text edits) with the resource operations `CreateFile`,
//! `RenameFile`, and `DeleteFile`.

use crate::position::Position;
use schemars::JsonSchema;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

/// A half-open `[start, end)` span of positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct Range {
    /// First position.
    pub start: Position,
    /// One past the last position.
    pub end: Position,
}

/// One text change within a file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    /// Span to replace.
    pub range: Range,
    /// Replacement text.
    pub new_text: String,
}

/// Identifies the file a [`TextDocumentEdit`] changes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct TextDocumentIdentifier {
    /// `file://` URI of the document.
    pub uri: String,
}

/// Every edit to one file, applied together against its original text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentEdit {
    /// File the edits apply to.
    pub text_document: TextDocumentIdentifier,
    /// Edits, each against the file's original text.
    pub edits: Vec<TextEdit>,
}

/// Creates a new, empty file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct CreateFile {
    /// `file://` URI to create.
    pub uri: String,
    /// Create options; only `overwrite` changes what griz does.
    pub options: Option<CreateFileOptions>,
}

/// What a `CreateFile` does when the file already exists.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFileOptions {
    /// Replace the file when it exists, instead of refusing.
    #[serde(default)]
    pub overwrite: bool,
}

/// Renames a file. Becomes [`Op::Move`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameFile {
    /// `file://` URI of the existing file.
    pub old_uri: String,
    /// `file://` URI it becomes.
    pub new_uri: String,
}

/// Deletes a file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct DeleteFile {
    /// `file://` URI to delete.
    pub uri: String,
}

/// One entry of `documentChanges`: edits to a file's text, or a resource
/// operation on it. A resource operation carries a `kind` of `create`,
/// `rename`, or `delete`; a text edit carries none, which is how the two are
/// told apart on the wire.
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(untagged)]
pub enum DocumentChange {
    /// Edits to one file's text.
    Edit(TextDocumentEdit),
    /// A new, empty file.
    Create(CreateFile),
    /// A file rename.
    Rename(RenameFile),
    /// A file deletion.
    Delete(DeleteFile),
}

impl<'de> Deserialize<'de> for DocumentChange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        from_kind(kind.as_deref(), value).map_err(serde::de::Error::custom)
    }
}

fn from_kind(
    kind: Option<&str>,
    value: serde_json::Value,
) -> Result<DocumentChange, serde_json::Error> {
    match kind {
        Some("create") => serde_json::from_value(value).map(DocumentChange::Create),
        Some("rename") => serde_json::from_value(value).map(DocumentChange::Rename),
        Some("delete") => serde_json::from_value(value).map(DocumentChange::Delete),
        _ => serde_json::from_value(value).map(DocumentChange::Edit),
    }
}

/// A workspace-wide edit from another tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEdit {
    /// Per-file edits, keyed by `file://` URI. Ignored when `document_changes`
    /// is given.
    #[serde(default)]
    pub changes: Option<BTreeMap<String, Vec<TextEdit>>>,
    /// Edits and resource operations, in order. Preferred over `changes`.
    #[serde(default)]
    pub document_changes: Option<Vec<DocumentChange>>,
}

/// Why a `WorkspaceEdit` could not become operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceEditError {
    /// A `uri` was not a `file://` URI, or did not decode as valid UTF-8.
    #[error("bad file URI `{uri}`")]
    BadUri {
        /// The URI as given.
        uri: String,
    },
    /// The file could not be read to convert its positions.
    #[error("{}: {message}", path.display())]
    Unreadable {
        /// File that could not be read.
        path: PathBuf,
        /// Why.
        message: String,
    },
    /// A position's line or character falls outside the file's text.
    #[error("{}: position {line}:{character} is out of range", path.display())]
    BadPosition {
        /// File the position was inside.
        path: PathBuf,
        /// 0-based line.
        line: u32,
        /// 0-based character.
        character: u32,
    },
}
