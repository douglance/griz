//! `apply` and `undo`: the only commands that write source files.

use crate::{
    annotations,
    cmd_plan::{confidence, input_paths},
    context::{CmdError, input_root, resolve_paths, with_store},
    receipt::{Mutation, run_mutation},
    render, respond,
    verdict::{Outcome, Verbosity},
};
use griz_core::Confidence;
use griz_store::{Absorbed, ApplyRequest, OnStale, UndoRequest};
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
    /// Number of files the plan must write. A mismatch writes nothing.
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
    let on_stale = parse_on_stale(options.on_stale.as_deref())?;
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
            |store| crate::apply_expect::apply(store, &request, expect_files),
            |store, id, outcome, _| {
                Ok(render::operation(&store.operation(id)?, expect_files).with_outcome(outcome))
            },
        )
    })
    .await
}

/// Parses `refuse` (default) or `merge`.
///
/// # Errors
/// Returns a validation error naming the accepted values.
fn parse_on_stale(text: Option<&str>) -> Result<OnStale, CmdError> {
    match text {
        None | Some("refuse") => Ok(OnStale::Refuse),
        Some("merge") => Ok(OnStale::Merge),
        Some(other) => Err(CmdError::invalid(format!(
            "unknown on_stale `{other}`; use refuse or merge"
        ))),
    }
}

#[derive(Deserialize, incurs::Args)]
struct OperationArgs {
    /// Operation identifier, from apply or an earlier undo.
    operation: String,
}

#[derive(Deserialize, incurs::Args)]
struct UndoArgs {
    /// Operation identifier, from apply or an earlier undo. Omit when giving `since`.
    operation: Option<String>,
}

#[derive(Deserialize, incurs::Options)]
struct UndoOptions {
    /// Restore every applied operation from this one through the newest, as
    /// one operation. Mutually exclusive with the operation argument.
    since: Option<String>,
    /// Restore only these files. Defaults to every file the operation (or span) wrote.
    paths: Option<Vec<String>>,
    /// When a file changed since it was written: refuse (default) or merge.
    on_stale: Option<String>,
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
    CommandDef::typed::<UndoArgs, UndoOptions, (), Value, _, _>(
        "undo",
        |ctx: TypedContext<UndoArgs, UndoOptions, ()>| async move {
            respond(undo(ctx.args.operation, ctx.options).await)
        },
    )
    .description("Restore what an operation replaced, for files still exactly as it left them, or merged onto a newer text with on_stale merge. Give since instead of the operation argument to restore every applied operation from that one through the newest as one operation. An undo is itself an operation and can be undone.")
    .mcp(annotations::writes_files())
    .done()
}

async fn undo(
    operation: Option<String>,
    options: UndoOptions,
) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = input_root(options.root.as_deref())?;
    let paths = input_paths(&root, options.paths);
    let on_stale = parse_on_stale(options.on_stale.as_deref())?;
    let since = options.since;
    if operation.is_some() == since.is_some() {
        return Err(CmdError::invalid(
            "give exactly one of the operation argument or `since`",
        ));
    }
    let mutation = Mutation {
        command: "undo",
        key: options.idempotency_key,
        input: json!({ "operation": operation, "since": since, "paths": paths, "on_stale": on_stale }),
    };
    let purpose = options.purpose;
    with_store(move |store| {
        run_mutation(
            store,
            &mutation,
            level,
            |store| {
                let paths = resolve_paths(&paths)?;
                let op = match (operation.as_deref(), since.as_deref()) {
                    (_, Some(since)) => store.restore_since(since, &paths, on_stale, &purpose)?,
                    (Some(operation), None) => store.undo(&UndoRequest {
                        operation: operation.to_string(),
                        paths: paths.clone(),
                        on_stale,
                        purpose: purpose.clone(),
                    })?,
                    (None, None) => unreachable!("validated above"),
                };
                Ok(render::operation(&op, None))
            },
            |store, id, outcome, _| {
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

/// Refuses details older receipts never saved instead of inventing a result.
fn replay_absorb(data: Option<&Value>) -> Result<Absorbed, CmdError> {
    let data = data.ok_or_else(|| CmdError {
        code: "IDEMPOTENCY_DETAIL_UNAVAILABLE",
        message: "this absorb receipt has no saved details; retry with verbosity error for its original verdict, or get the operation for its current state".into(),
    })?;
    serde_json::from_value(data.clone()).map_err(|error| griz_store::StoreError::from(error).into())
}

async fn absorb(id: String, options: AbsorbOptions) -> Result<(Value, Outcome), CmdError> {
    let level = Verbosity::resolve(options.verbosity.as_deref()).map_err(CmdError::invalid)?;
    let root = input_root(options.root.as_deref())?;
    let paths = input_paths(&root, options.paths);
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
            |store| {
                let paths = resolve_paths(&paths)?;
                Ok(render::absorbed(&store.absorb(&id, &paths, &purpose)?))
            },
            |_, _, outcome, data| {
                let result = replay_absorb(data)?;
                Ok(render::absorbed(&result).with_outcome(outcome))
            },
        )
    })
    .await
}
