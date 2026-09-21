//! Turning a `plan` call's inputs into operations: patch text, typed
//! operations, and an LSP workspace edit, each also readable from `@path`.

use crate::context::{CmdError, resolve, resolve_op};
use griz_core::{
    DiskSource, Op, PositionEncoding, WorkspaceEdit, parse_patch, workspace_edit_to_ops,
};
use serde_json::Value;
use std::path::Path;

/// Parses `text` as `utf-8`, `utf-16`, or `utf-32`; `None` defaults to
/// Whether the caller requires the plan's result to still parse. Only
/// `clean` is accepted; nothing declared means no expectation.
pub fn expect_clean(text: Option<&str>) -> Result<bool, CmdError> {
    match text {
        None => Ok(false),
        Some("clean") => Ok(true),
        Some(other) => Err(CmdError::invalid(format!(
            "expect_syntax must be `clean`, not `{other}`"
        ))),
    }
}

/// Parses `text` as `utf-8`, `utf-16`, or `utf-32`; `None` defaults to
/// `utf-16`, the LSP default.
pub fn position_encoding(text: Option<&str>) -> Result<PositionEncoding, CmdError> {
    match text {
        Some(text) => PositionEncoding::parse(text).map_err(CmdError::invalid),
        None => Ok(PositionEncoding::default()),
    }
}

/// Collects operations from `patch`, `ops`, and `workspace_edit`, in that
/// order: patch hunks first, then typed operations, then a workspace edit's
/// text edits and resource operations last. All three may be given in one
/// call; later operations see the effect of earlier ones against the same
/// file, exactly as several patch hunks for one file already do.
pub fn collect_ops(
    root: &Path,
    ops: Option<Vec<Value>>,
    patch: Option<&str>,
    workspace_edit: Option<Value>,
    encoding: PositionEncoding,
) -> Result<Vec<Op>, CmdError> {
    let mut all = Vec::new();
    if let Some(patch) = patch {
        let text = at_file(root, patch)?.unwrap_or_else(|| patch.to_string());
        let resolver = |path: &str| resolve(root, Path::new(path));
        all.extend(parse_patch(&text, &resolver).map_err(|e| CmdError::invalid(e.to_string()))?);
    }
    for (index, value) in normalize(root, ops.unwrap_or_default())?
        .into_iter()
        .enumerate()
    {
        let op: Op = serde_json::from_value(value)
            .map_err(|e| CmdError::invalid(format!("ops[{index}]: {e}")))?;
        all.push(resolve_op(root, op));
    }
    all.extend(collect_workspace_edit_ops(root, workspace_edit, encoding)?);
    if all.is_empty() {
        return Err(CmdError::invalid(
            "give `ops`, `patch`, or `workspace_edit`",
        ));
    }
    Ok(all)
}

/// The text of a `@path` input, or `None` when the string is the input
/// itself. Every JSON input a command takes accepts both forms, so a large
/// document never has to go on the command line.
fn at_file(root: &Path, text: &str) -> Result<Option<String>, CmdError> {
    let Some(file) = text.strip_prefix('@') else {
        return Ok(None);
    };
    std::fs::read_to_string(resolve(root, Path::new(file)))
        .map(Some)
        .map_err(|e| CmdError::invalid(format!("reading {file}: {e}")))
}

/// Parses `workspace_edit` (a JSON object, JSON text, or `@path` for CLI
/// callers) and converts it to operations against the files it names.
fn collect_workspace_edit_ops(
    root: &Path,
    workspace_edit: Option<Value>,
    encoding: PositionEncoding,
) -> Result<Vec<Op>, CmdError> {
    let Some(value) = workspace_edit else {
        return Ok(Vec::new());
    };
    let value = match value {
        Value::String(text) => {
            let text = at_file(root, &text)?.unwrap_or(text);
            serde_json::from_str(&text)
                .map_err(|e| CmdError::invalid(format!("workspace_edit must be JSON: {e}")))?
        }
        other => other,
    };
    let edit: WorkspaceEdit = serde_json::from_value(value)
        .map_err(|e| CmdError::invalid(format!("workspace_edit: {e}")))?;
    workspace_edit_to_ops(&edit, encoding, &DiskSource)
        .map_err(|e| CmdError::invalid(e.to_string()))
}

/// Accepts operations as structured values (MCP, Code Mode) or as JSON text
/// from the command line: one JSON array, repeated JSON objects, or `@path`.
fn normalize(root: &Path, values: Vec<Value>) -> Result<Vec<Value>, CmdError> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let parsed = match value {
            Value::String(text) => {
                let text = at_file(root, &text)?.unwrap_or(text);
                serde_json::from_str(&text)
                    .map_err(|e| CmdError::invalid(format!("ops must be JSON: {e}")))?
            }
            other => other,
        };
        match parsed {
            Value::Array(items) => out.extend(items),
            item => out.push(item),
        }
    }
    Ok(out)
}
