//! `plan` and `select`: record changes without writing a source file.

use crate::{
    annotations,
    context::{CmdError, resolve, root, with_store},
    plan_input::{PlanInput, expect_clean, position_encoding},
    plan_schema::plan_input_schema,
    receipt::{Mutation, run_mutation},
    render::{self, PlanExpect},
    respond,
    verdict::{Outcome, Verbosity},
};
use griz_core::{Confidence, DiskSource, build_plan};
use griz_store::Selection;
use incurs::command::{CommandDef, TypedContext};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Deserialize, incurs::Options)]
struct PlanOptions {
    /// Operations as JSON objects, applied in order:
    /// `{op:"replace", path, find:{text, after?, whole_lines?} | range:{start,end}, replace, occurrence?:"unique"|"all"|{nth:N}, expect_hash?}`,
    /// `{op:"insert", path, anchor:{text}, after?:bool, text, expect_hash?}`,
    /// `{op:"create", path, text}`, `{op:"delete", path, expect_hash?}`, `{op:"move", path, to, expect_hash?}`.
    /// A `@path` string reads the operations from that JSON file.
    ops: Option<Vec<Value>>,
    /// Codex patch text, or `@path` to read the patch from a file.
    patch: Option<String>,
    /// LSP `WorkspaceEdit` as JSON: `changes` (a URI-keyed map of
    /// `TextEdit`s) or `documentChanges` (`TextDocumentEdit`, `CreateFile`,
    /// `RenameFile`, `DeleteFile`). Combines with `ops` and `patch`; applied
    /// last, after patch hunks and typed operations. A `@path` string reads
    /// it from that JSON file.
    workspace_edit: Option<Value>,
    /// How `workspace_edit` positions count into a line: utf-8, utf-16
    /// (default), or utf-32.
    position_encoding: Option<String>,
    /// Directory relative paths resolve from. Defaults to the current directory.
    root: Option<String>,
    /// Why this plan is being made.
    purpose: String,
    /// Key that makes a retry return the original plan instead of a new one.
    idempotency_key: String,
    /// Number of edits the plan must contain.
    expect_edits: Option<usize>,
    /// Number of files the plan must change.
    expect_files: Option<usize>,
    /// Require the plan to leave every file in a supported language parsing:
    /// `clean` fails a plan that introduces a syntax error. Files griz has no
    /// parser for are unaffected.
    expect_syntax: Option<String>,
    /// Response detail: off, error, warn, info, debug, or trace.
    verbosity: Option<String>,
}

/// The `plan` command.
pub fn plan_command() -> CommandDef {
    CommandDef::typed::<(), PlanOptions, (), Value, _, _>(
        "plan",
        |ctx: TypedContext<(), PlanOptions, ()>| async move { respond(plan(ctx.options).await) },
    )
    .description("Plan edits from operations or patch text. Writes no source file; returns a plan id for diff, select, and apply.")
    .mcp(annotations::records())
    .mcp_input_schema(plan_input_schema())
    .done()
}

async fn plan(options: PlanOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = root(options.root.as_deref())?;
    let encoding = position_encoding(options.position_encoding.as_deref())?;
    let input = PlanInput::parse(
        &root,
        options.ops,
        options.patch.as_deref(),
        options.workspace_edit,
        encoding,
    )?;
    let expect = PlanExpect {
        edits: options.expect_edits,
        files: options.expect_files,
    };
    let expect_clean = expect_clean(options.expect_syntax.as_deref())?;
    let mutation = Mutation {
        command: "plan",
        key: options.idempotency_key,
        input: input.identity(expect, expect_clean),
    };
    let purpose = options.purpose;
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| {
                let ops = input.operations()?;
                let mut plan = build_plan(&ops, &DiskSource);
                let syntax = plan.syntax;
                let file_syntax = std::mem::take(&mut plan.file_syntax);
                let record = store.save_plan(&purpose, ops.clone(), plan)?;
                let rendered =
                    render::with_syntax(render::plan(&record, expect), syntax, &file_syntax);
                Ok(render::expect_clean_syntax(rendered, syntax, expect_clean))
            },
            |store, id, outcome| {
                let record = store.plan(id)?;
                let changes = store.plan_changes(&record)?;
                let file_syntax = griz_core::annotate(&changes);
                let syntax = griz_core::plan_syntax(&file_syntax);
                let rendered =
                    render::with_syntax(render::plan(&record, expect), syntax, &file_syntax);
                let rendered = render::expect_clean_syntax(rendered, syntax, expect_clean);
                Ok(rendered.with_outcome(outcome))
            },
        )
    })
    .await
}

#[derive(Deserialize, incurs::Args)]
struct PlanArgs {
    /// Plan identifier.
    plan: String,
}

#[derive(Deserialize, incurs::Options)]
struct SelectOptions {
    /// Keep operations touching these paths.
    paths: Option<Vec<String>>,
    /// Keep operations that produced these edit ids, such as e0 or e2.1.
    edits: Option<Vec<String>>,
    /// Keep operations whose every edit is at least this confident: machine or maybe.
    min_confidence: Option<String>,
    /// Directory relative paths resolve from. Defaults to the current directory.
    root: Option<String>,
    /// Why this selection is being made.
    purpose: String,
    /// Key that makes a retry return the original selection.
    idempotency_key: String,
    /// Response detail: off, error, warn, info, debug, or trace.
    verbosity: Option<String>,
}

/// The `select` command.
pub fn select_command() -> CommandDef {
    CommandDef::typed::<PlanArgs, SelectOptions, (), Value, _, _>(
        "select",
        |ctx: TypedContext<PlanArgs, SelectOptions, ()>| async move {
            respond(select(ctx.args.plan, ctx.options).await)
        },
    )
    .description("Make a new plan from part of another: by path, edit id, or confidence. Enables partial apply and partial undo.")
    .mcp(annotations::records())
    .done()
}

async fn select(id: String, options: SelectOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = root(options.root.as_deref())?;
    let selection = Selection {
        paths: resolve_all(&root, options.paths),
        edits: options.edits.unwrap_or_default(),
        min_confidence: options
            .min_confidence
            .as_deref()
            .map(confidence)
            .transpose()?,
    };
    let mutation = Mutation {
        command: "select",
        key: options.idempotency_key,
        input: json!({ "plan": id, "selection": selection }),
    };
    let purpose = options.purpose;
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| {
                Ok(render::plan(
                    &store.select(&id, &selection, &purpose)?,
                    PlanExpect::default(),
                ))
            },
            |store, id, outcome| {
                Ok(render::plan(&store.plan(id)?, PlanExpect::default()).with_outcome(outcome))
            },
        )
    })
    .await
}

/// Resolves optional path options against `root`.
pub fn resolve_all(root: &Path, paths: Option<Vec<String>>) -> Vec<PathBuf> {
    paths
        .unwrap_or_default()
        .iter()
        .map(|path| resolve(root, Path::new(path)))
        .collect()
}

/// Parses a confidence level.
///
/// # Errors
/// Returns a validation error naming the accepted levels.
pub fn confidence(text: &str) -> Result<Confidence, CmdError> {
    match text {
        "machine" => Ok(Confidence::Machine),
        "maybe" => Ok(Confidence::Maybe),
        other => Err(CmdError::invalid(format!(
            "unknown confidence `{other}`; use machine or maybe"
        ))),
    }
}
