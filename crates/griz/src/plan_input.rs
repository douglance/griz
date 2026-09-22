//! Turning a `plan` call's inputs into operations: patch text, typed
//! operations, and an LSP workspace edit, each also readable from `@path`.

use crate::{
    context::{CmdError, input_path, resolve, resolve_ops},
    render::PlanExpect,
};
use griz_core::{
    DiskSource, Op, PositionEncoding, WorkspaceEdit, parse_patch, workspace_edit_to_ops,
};
use serde_json::{Value, json};
use std::path::Path;

/// A normalized request, before any workspace-dependent conversion.
pub struct PlanInput {
    ops: Vec<Op>,
    workspace_edit: Option<Value>,
    encoding: PositionEncoding,
}

impl PlanInput {
    /// Resolves explicit input files and paths without reading edit targets.
    pub fn parse(
        root: &Path,
        ops: Option<Vec<Value>>,
        patch: Option<&str>,
        workspace_edit: Option<Value>,
        encoding: PositionEncoding,
    ) -> Result<Self, CmdError> {
        Ok(Self {
            ops: collect_ops(root, ops, patch)?,
            workspace_edit: workspace_edit
                .map(|value| json_input(root, value))
                .transpose()?,
            encoding,
        })
    }

    /// Identifies the submitted request, independently of later file contents.
    pub fn identity(&self, expect: PlanExpect, clean: bool) -> Value {
        let mut input = json!({
            "ops": self.ops, "expect": [expect.edits, expect.files], "syntax": clean
        });
        if let Some(edit) = &self.workspace_edit {
            input["workspace_edit"] = json!({
                "edit": edit, "position_encoding": encoding_name(self.encoding)
            });
        }
        input
    }

    /// Converts positions only after the request has a new idempotency claim.
    pub fn operations(self) -> Result<Vec<Op>, CmdError> {
        let mut ops = resolve_ops(Path::new(""), self.ops)?;
        ops.extend(collect_workspace_edit_ops(
            self.workspace_edit,
            self.encoding,
        )?);
        if ops.is_empty() {
            return Err(CmdError::invalid(
                "give `ops`, `patch`, or `workspace_edit`",
            ));
        }
        Ok(ops)
    }
}

fn encoding_name(encoding: PositionEncoding) -> &'static str {
    match encoding {
        PositionEncoding::Utf8 => "utf-8",
        PositionEncoding::Utf16 => "utf-16",
        PositionEncoding::Utf32 => "utf-32",
    }
}

fn json_input(root: &Path, value: Value) -> Result<Value, CmdError> {
    match value {
        Value::String(text) => {
            let text = at_file(root, &text)?.unwrap_or(text);
            serde_json::from_str(&text)
                .map_err(|e| CmdError::invalid(format!("workspace_edit must be JSON: {e}")))
        }
        other => Ok(other),
    }
}

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

fn input_op(root: &Path, mut op: Op) -> Op {
    match &mut op {
        Op::Replace { path, .. }
        | Op::Insert { path, .. }
        | Op::Create { path, .. }
        | Op::Delete { path, .. } => *path = input_path(root, path),
        Op::Move { path, to, .. } => {
            *path = input_path(root, path);
            *to = input_path(root, to);
        }
    }
    op
}

/// Collects patch hunks first, then typed operations, without reading targets.
fn collect_ops(
    root: &Path,
    ops: Option<Vec<Value>>,
    patch: Option<&str>,
) -> Result<Vec<Op>, CmdError> {
    let mut all = Vec::new();
    if let Some(patch) = patch {
        let text = at_file(root, patch)?.unwrap_or_else(|| patch.to_string());
        let resolver = |path: &str| input_path(root, Path::new(path));
        all.extend(parse_patch(&text, &resolver).map_err(|e| CmdError::invalid(e.to_string()))?);
    }
    for (index, value) in normalize(root, ops.unwrap_or_default())?
        .into_iter()
        .enumerate()
    {
        let op: Op = serde_json::from_value(value)
            .map_err(|e| CmdError::invalid(format!("ops[{index}]: {e}")))?;
        all.push(op);
    }
    Ok(all.into_iter().map(|op| input_op(root, op)).collect())
}

/// The text of a `@path` input, or `None` when the string is the input
/// itself. Every JSON input a command takes accepts both forms, so a large
/// document never has to go on the command line.
fn at_file(root: &Path, text: &str) -> Result<Option<String>, CmdError> {
    let Some(file) = text.strip_prefix('@') else {
        return Ok(None);
    };
    std::fs::read_to_string(resolve(root, Path::new(file))?)
        .map(Some)
        .map_err(|e| CmdError::invalid(format!("reading {file}: {e}")))
}

/// Converts a parsed workspace edit against the files it names.
fn collect_workspace_edit_ops(
    workspace_edit: Option<Value>,
    encoding: PositionEncoding,
) -> Result<Vec<Op>, CmdError> {
    let Some(value) = workspace_edit else {
        return Ok(Vec::new());
    };
    let edit: WorkspaceEdit = serde_json::from_value(value)
        .map_err(|e| CmdError::invalid(format!("workspace_edit: {e}")))?;
    let base = crate::context::root(None)?;
    let ops = workspace_edit_to_ops(&edit, encoding, &DiskSource)
        .map_err(|e| CmdError::invalid(e.to_string()))?;
    resolve_ops(&base, ops)
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
