//! `apply` and `undo`: the only commands that write source files.

use crate::{
    annotations,
    cmd_plan::{confidence, resolve_all},
    context::{CmdError, root, with_store},
    receipt::{Mutation, run_mutation},
    render, respond,
    verdict::{Outcome, Verbosity},
};
use griz_core::Confidence;
use griz_store::{ApplyRequest, OnStale};
use incurs::command::{CommandDef, TypedContext};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, incurs::Args)]
struct PlanArgs {
    /// Plan identifier.
    plan: String,
}

#[derive(Deserialize, incurs::Options)]
struct ApplyOptions {
    /// Least confidence every edit must have: machine (default) or maybe.
    min_confidence: Option<String>,
    /// When a file changed after planning: refuse (default) or merge.
    on_stale: Option<String>,
    /// Why these files are being written.
    purpose: String,
    /// Key that makes a retry return the original operation without writing again.
    idempotency_key: String,
    /// Number of files the apply must write.
    expect_files: Option<usize>,
    /// Response detail: off, error, warn, info, debug, or trace.
    verbosity: Option<String>,
}

/// The `apply` command.
pub fn apply_command() -> CommandDef {
    CommandDef::typed::<PlanArgs, ApplyOptions, (), Value, _, _>(
        "apply",
        |ctx: TypedContext<PlanArgs, ApplyOptions, ()>| async move {
            respond(apply(ctx.args.plan, ctx.options).await)
        },
    )
    .description("Write a plan all-or-nothing. Every file must still have the fingerprint it was planned against; returns an operation id for undo.")
    .mcp(annotations::writes_files())
    .done()
}

async fn apply(plan: String, options: ApplyOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let min_confidence = options
        .min_confidence
        .as_deref()
        .map_or(Ok(Confidence::Machine), confidence)?;
    let on_stale = match options.on_stale.as_deref() {
        None | Some("refuse") => OnStale::Refuse,
        Some("merge") => OnStale::Merge,
        Some(other) => {
            return Err(CmdError::invalid(format!(
                "unknown on_stale `{other}`; use refuse or merge"
            )));
        }
    };
    let expect_files = options.expect_files;
    let mutation = Mutation {
        command: "apply",
        key: options.idempotency_key,
        input: json!({ "plan": plan, "min_confidence": min_confidence, "on_stale": on_stale, "expect_files": expect_files }),
    };
    let request = ApplyRequest {
        plan,
        min_confidence,
        on_stale,
        purpose: options.purpose,
    };
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| Ok(render::operation(&store.apply(&request)?, expect_files)),
            |store, id, outcome| {
                Ok(render::operation(&store.operation(id)?, expect_files).with_outcome(outcome))
            },
        )
    })
    .await
}

#[derive(Deserialize, incurs::Args)]
struct OperationArgs {
    /// Operation identifier, from apply or an earlier undo.
    operation: String,
}

#[derive(Deserialize, incurs::Options)]
struct UndoOptions {
    /// Restore only these files. Defaults to every file the operation wrote.
    paths: Option<Vec<String>>,
    /// Directory relative paths resolve from. Defaults to the current directory.
    root: Option<String>,
    /// Why the files are being restored.
    purpose: String,
    /// Key that makes a retry return the original undo without writing again.
    idempotency_key: String,
    /// Response detail: off, error, warn, info, debug, or trace.
    verbosity: Option<String>,
}

/// The `undo` command.
pub fn undo_command() -> CommandDef {
    CommandDef::typed::<OperationArgs, UndoOptions, (), Value, _, _>(
        "undo",
        |ctx: TypedContext<OperationArgs, UndoOptions, ()>| async move {
            respond(undo(ctx.args.operation, ctx.options).await)
        },
    )
    .description("Restore what an operation replaced, for files still exactly as it left them. An undo is itself an operation and can be undone.")
    .mcp(annotations::writes_files())
    .done()
}

async fn undo(id: String, options: UndoOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = root(options.root.as_deref())?;
    let paths = resolve_all(&root, options.paths);
    let mutation = Mutation {
        command: "undo",
        key: options.idempotency_key,
        input: json!({ "operation": id, "paths": paths }),
    };
    let purpose = options.purpose;
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| Ok(render::operation(&store.undo(&id, &paths, &purpose)?, None)),
            |store, id, outcome| {
                Ok(render::operation(&store.operation(id)?, None).with_outcome(outcome))
            },
        )
    })
    .await
}

#[derive(Deserialize, incurs::Options)]
struct AbsorbOptions {
    /// Absorb only these files. Defaults to every file the operation wrote.
    paths: Option<Vec<String>>,
    /// Directory relative paths resolve from. Defaults to the current directory.
    root: Option<String>,
    /// Why the later changes are being absorbed, such as a formatter run.
    purpose: String,
    /// Key that makes a retry return the original result.
    idempotency_key: String,
    /// Response detail: off, error, warn, info, debug, or trace.
    verbosity: Option<String>,
}

/// The `absorb` command.
pub fn absorb_command() -> CommandDef {
    CommandDef::typed::<OperationArgs, AbsorbOptions, (), Value, _, _>(
        "absorb",
        |ctx: TypedContext<OperationArgs, AbsorbOptions, ()>| async move {
            respond(absorb(ctx.args.operation, ctx.options).await)
        },
    )
    .description("Fold later changes to an operation's files, such as a formatter's, into the operation, so undo still restores the text from before the apply.")
    .mcp(annotations::records())
    .done()
}

async fn absorb(id: String, options: AbsorbOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = root(options.root.as_deref())?;
    let paths = resolve_all(&root, options.paths);
    let mutation = Mutation {
        command: "absorb",
        key: options.idempotency_key,
        input: json!({ "operation": id, "paths": paths }),
    };
    let purpose = options.purpose;
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| Ok(render::absorbed(&store.absorb(&id, &paths, &purpose)?)),
            |store, id, outcome| {
                Ok(render::operation(&store.operation(id)?, None).with_outcome(outcome))
            },
        )
    })
    .await
}
