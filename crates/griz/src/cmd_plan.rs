//! `plan` and `select`: record changes without writing a source file.

use crate::{
    annotations,
    context::{CmdError, resolve, resolve_op, root, with_store},
    receipt::{Mutation, run_mutation},
    render::{self, PlanExpect},
    respond,
    verdict::{Outcome, Verbosity},
};
use griz_core::{Confidence, DiskSource, Op, build_plan, parse_patch};
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
    ops: Option<Vec<Value>>,
    /// Codex patch text, or `@path` to read the patch from a file.
    patch: Option<String>,
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
    .done()
}

async fn plan(options: PlanOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = root(options.root.as_deref())?;
    let ops = collect_ops(&root, options.ops, options.patch.as_deref())?;
    let expect = PlanExpect {
        edits: options.expect_edits,
        files: options.expect_files,
    };
    let mutation = Mutation {
        command: "plan",
        key: options.idempotency_key,
        input: json!({ "ops": ops, "expect": [expect.edits, expect.files] }),
    };
    let purpose = options.purpose;
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| {
                let plan = build_plan(&ops, &DiskSource);
                Ok(render::plan(
                    &store.save_plan(&purpose, ops.clone(), plan)?,
                    expect,
                ))
            },
            |store, id, outcome| Ok(render::plan(&store.plan(id)?, expect).with_outcome(outcome)),
        )
    })
    .await
}

fn collect_ops(
    root: &Path,
    ops: Option<Vec<Value>>,
    patch: Option<&str>,
) -> Result<Vec<Op>, CmdError> {
    let mut all = Vec::new();
    if let Some(patch) = patch {
        let text = match patch.strip_prefix('@') {
            Some(file) => std::fs::read_to_string(resolve(root, Path::new(file)))
                .map_err(|e| CmdError::invalid(format!("reading patch file: {e}")))?,
            None => patch.to_string(),
        };
        let resolver = |path: &str| resolve(root, Path::new(path));
        all.extend(parse_patch(&text, &resolver).map_err(|e| CmdError::invalid(e.to_string()))?);
    }
    for (index, value) in normalize(ops.unwrap_or_default())?.into_iter().enumerate() {
        let op: Op = serde_json::from_value(value)
            .map_err(|e| CmdError::invalid(format!("ops[{index}]: {e}")))?;
        all.push(resolve_op(root, op));
    }
    if all.is_empty() {
        return Err(CmdError::invalid("give `ops`, `patch`, or both"));
    }
    Ok(all)
}

/// Accepts operations as structured values (MCP, Code Mode) or as JSON text
/// from the command line: one JSON array, or repeated JSON objects.
fn normalize(values: Vec<Value>) -> Result<Vec<Value>, CmdError> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let parsed = match value {
            Value::String(text) => serde_json::from_str(&text)
                .map_err(|e| CmdError::invalid(format!("ops must be JSON: {e}")))?,
            other => other,
        };
        match parsed {
            Value::Array(items) => out.extend(items),
            item => out.push(item),
        }
    }
    Ok(out)
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
