//! `diff`, `log`, and `get`: inspect plans and operations.

use crate::{
    annotations,
    context::{CmdError, root, with_store},
    lines::{Address, address, merge_lines},
};
use griz_core::{ChangeKind, FileChange, render_diff};
use griz_store::{Operation, Store, parse_blob_id};
use incurs::command::{CommandDef, TypedContext, TypedResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize, incurs::Args)]
struct IdArgs {
    /// Plan or operation identifier.
    id: String,
}

#[derive(Deserialize, incurs::Options)]
struct DiffOptions {
    /// Return only diff lines matching this regular expression.
    grep: Option<String>,
    /// Selected of context around each match.
    #[incurs(default = 0)]
    context: usize,
    /// Return only this inclusive line range of the diff, such as 1-80.
    lines: Option<String>,
    /// Directory file names are shown relative to. Defaults to the current directory.
    root: Option<String>,
}

/// The `diff` command.
pub fn diff_command() -> CommandDef {
    CommandDef::typed::<IdArgs, DiffOptions, (), Value, _, _>(
        "diff",
        |ctx: TypedContext<IdArgs, DiffOptions, ()>| async move {
            let address = Address {
                grep: ctx.options.grep,
                context: ctx.options.context,
                lines: ctx.options.lines,
            };
            let id = ctx.args.id;
            let base = match root(ctx.options.root.as_deref()) {
                Ok(base) => base,
                Err(error) => return error.result(),
            };
            let result = with_store(move |store| diff(store, &id, &address, &base)).await;
            result.map_or_else(CmdError::result, TypedResult::ok)
        },
    )
    .description("Unified diff of a plan or an operation, with per-file line counts. Address long diffs with grep or lines.")
    .mcp(annotations::read_only())
    .done()
}

fn diff(store: &Store, id: &str, how: &Address, base: &Path) -> Result<Value, CmdError> {
    let changes = if id.starts_with("op_") {
        operation_changes(store, &store.operation(id)?)?
    } else {
        store.plan_changes(&store.plan(id)?)?
    };
    let diffs = render_diff(&changes, base);
    let text: String = diffs.iter().map(|diff| diff.text.as_str()).collect();
    let selected = address(&text, how).map_err(CmdError::invalid)?;
    let mut body = json!({
        "id": id,
        "files": diffs.iter().map(|d| json!({
            "path": d.path, "kind": d.kind, "added": d.added, "removed": d.removed, "items": d.items,
        })).collect::<Vec<_>>(),
    });
    merge_lines(&mut body, selected);
    Ok(body)
}

fn operation_changes(store: &Store, op: &Operation) -> Result<Vec<FileChange>, CmdError> {
    op.files
        .iter()
        .map(|file| {
            let text =
                |hash: &Option<String>| hash.as_deref().map(|h| store.get_blob(h)).transpose();
            let kind = match (&file.before_hash, &file.after_hash) {
                (None, _) => ChangeKind::Create,
                (Some(_), None) => ChangeKind::Delete,
                _ => ChangeKind::Modify,
            };
            Ok(FileChange {
                path: file.path.clone(),
                kind,
                before: text(&file.before_hash)?,
                before_hash: file.before_hash.clone(),
                after: text(&file.after_hash)?,
                after_hash: file.after_hash.clone(),
            })
        })
        .collect()
}

#[derive(Deserialize, incurs::Options)]
struct LogOptions {
    /// Most operations to return.
    #[incurs(default = 20)]
    limit: usize,
    /// Return operations older than this operation id, for paging.
    before: Option<String>,
}

/// The `log` command.
pub fn log_command() -> CommandDef {
    CommandDef::typed::<(), LogOptions, (), Value, _, _>(
        "log",
        |ctx: TypedContext<(), LogOptions, ()>| async move {
            let LogOptions { limit, before } = ctx.options;
            let result = with_store(move |store| log(store, limit, before.as_deref())).await;
            result.map_or_else(CmdError::result, TypedResult::ok)
        },
    )
    .description("Operations newest first: every apply and undo with its state and file count.")
    .mcp(annotations::read_only())
    .done()
}

fn log(store: &Store, limit: usize, before: Option<&str>) -> Result<Value, CmdError> {
    let ops = store.operations(limit, before)?;
    let next = (ops.len() == limit)
        .then(|| ops.last().map(|op| op.id.clone()))
        .flatten();
    Ok(json!({
        "operations": ops.iter().map(|op| json!({
            "id": op.id, "kind": op.kind, "state": op.state, "plan": op.plan,
            "undoes": op.undoes, "files": op.files.len(), "conflicts": op.conflicts.len(),
            "purpose": op.purpose, "created_at": op.created_at,
        })).collect::<Vec<_>>(),
        "next": next,
    }))
}

/// The `get` command.
pub fn get_command() -> CommandDef {
    CommandDef::typed::<IdArgs, (), (), Value, _, _>(
        "get",
        |ctx: TypedContext<IdArgs, (), ()>| async move {
            let id = ctx.args.id;
            let result = with_store(move |store| get(store, &id)).await;
            result.map_or_else(CmdError::result, TypedResult::ok)
        },
    )
    .description("The complete record of a plan or operation, including problems and the nearest real text for any anchor that missed. A blob_ prefixed id, as a merge conflict's base, planned, or current fields carry, returns that text instead.")
    .mcp(annotations::read_only())
    .done()
}

fn get(store: &Store, id: &str) -> Result<Value, CmdError> {
    if let Some(hash) = parse_blob_id(id) {
        let text = store.get_blob(hash)?;
        return Ok(json!({ "id": id, "text": text }));
    }
    let value = if id.starts_with("op_") {
        serde_json::to_value(store.operation(id)?)
    } else {
        serde_json::to_value(store.plan(id)?)
    };
    value.map_err(|e| CmdError::invalid(e.to_string()))
}
