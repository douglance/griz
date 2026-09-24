//! Turns stored plans and operations into verdicts.

use crate::verdict::{Outcome, Rendered, Verbosity, unmet};
use griz_core::{ChangeKind, Confidence, FileSyntax, PlanSyntax, Problem, ProblemKind};
use griz_store::{Absorbed, Operation, OperationState, PlanRecord};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Expected counts a caller declared for a plan.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanExpect {
    /// Edits the plan must contain.
    pub edits: Option<usize>,
    /// Files the plan must change.
    pub files: Option<usize>,
}

/// Renders a plan, judging it against `expect`.
#[must_use]
pub fn plan(record: &PlanRecord, expect: PlanExpect, level: Verbosity) -> Rendered {
    let (outcome, reason) = if let Some(problem) = record.problems.first() {
        (
            Outcome::Error,
            Some(describe(problem, record.problems.len())),
        )
    } else {
        let miss = unmet("edits", expect.edits, record.edits.len())
            .or_else(|| unmet("files", expect.files, record.files.len()));
        match miss {
            Some(reason) => (Outcome::Failed, Some(reason)),
            None => (Outcome::Passed, None),
        }
    };
    plan_with(record, outcome, reason, level)
}

/// Renders a plan with a known outcome, as when replaying a receipt.
#[must_use]
pub fn plan_with(
    record: &PlanRecord,
    outcome: Outcome,
    reason: Option<String>,
    level: Verbosity,
) -> Rendered {
    let count = |kind| record.files.iter().filter(|file| file.kind == kind).count();
    let confident = |level| {
        record
            .edits
            .iter()
            .filter(|edit| edit.confidence == level)
            .count()
    };
    Rendered {
        id: record.id.clone(),
        outcome,
        reason,
        summary: if level >= Verbosity::Info {
            json!({
                "files": record.files.len(),
                "edits": record.edits.len(),
                "created": count(ChangeKind::Create),
                "deleted": count(ChangeKind::Delete),
                "problems": record.problems.len(),
                "confidence": {
                    "machine": confident(Confidence::Machine),
                    "maybe": confident(Confidence::Maybe),
                },
            })
        } else {
            Value::Null
        },
        detail: if level >= Verbosity::Debug {
            json!({
                "edits": record.edits,
                "problems": record.problems,
                "files": record.files.iter().map(|f| json!({ "path": f.path, "kind": f.kind })).collect::<Vec<_>>(),
            })
        } else {
            Value::Null
        },
        record: if level == Verbosity::Trace {
            to_value(record)
        } else {
            Value::Null
        },
        replay_data: None,
    }
}

/// Attaches syntax facts computed for a plan build or replay: the aggregate
/// verdict in `summary` (info and up) and each file's facts in `detail` and
/// the traced `record` (debug and up, and trace).
#[must_use]
pub fn with_syntax(
    mut rendered: Rendered,
    syntax: PlanSyntax,
    file_syntax: &BTreeMap<PathBuf, FileSyntax>,
) -> Rendered {
    if let Some(summary) = rendered.summary.as_object_mut() {
        summary.insert("syntax".into(), json!(syntax));
    }
    if let Some(record) = rendered.record.as_object_mut() {
        record.insert("syntax".into(), json!(syntax));
    }
    attach_file_syntax(&mut rendered.detail, file_syntax);
    attach_file_syntax(&mut rendered.record, file_syntax);
    rendered
}

/// Fails a rendered plan that introduced syntax errors, when the caller
/// declared it must stay clean. The fact is griz's; the expectation is the
/// caller's.
#[must_use]
pub fn expect_clean_syntax(mut rendered: Rendered, syntax: PlanSyntax, expected: bool) -> Rendered {
    if !expected || rendered.outcome != Outcome::Passed || syntax != PlanSyntax::IntroducedErrors {
        return rendered;
    }
    rendered.outcome = Outcome::Failed;
    rendered.reason = Some("expected syntax clean, observed introduced_errors".to_string());
    rendered
}

fn attach_file_syntax(value: &mut Value, file_syntax: &BTreeMap<PathBuf, FileSyntax>) {
    let Some(files) = value.get_mut("files").and_then(Value::as_array_mut) else {
        return;
    };
    for file in files {
        let Some(path) = file.get("path").and_then(Value::as_str) else {
            continue;
        };
        if let Some(fact) = file_syntax.get(Path::new(path)) {
            file["syntax"] = json!(fact);
        }
    }
}

/// Renders an operation, judging it against an expected file count.
#[must_use]
pub fn operation(op: &Operation, expect_files: Option<usize>) -> Rendered {
    let (outcome, reason) = match op.state {
        OperationState::Failed => (Outcome::Error, op.reason.clone()),
        _ if !op.conflicts.is_empty() => (
            Outcome::Failed,
            Some(format!(
                "{} file(s) changed by someone else were left alone",
                op.conflicts.len()
            )),
        ),
        _ => match unmet("files", expect_files, op.files.len()) {
            Some(reason) => (Outcome::Failed, Some(reason)),
            None => (Outcome::Passed, None),
        },
    };
    operation_with(op, outcome, reason)
}

/// Renders an operation with a known outcome.
#[must_use]
pub fn operation_with(op: &Operation, outcome: Outcome, reason: Option<String>) -> Rendered {
    Rendered {
        id: op.id.clone(),
        outcome,
        reason: reason.or_else(|| op.reason.clone()),
        summary: json!({
            "kind": op.kind,
            "state": op.state,
            "files": op.files.len(),
            "merged": op.files.iter().filter(|file| file.merged).count(),
            "conflicts": op.conflicts.len(),
            "merge_conflicts": op.merge_conflicts.len(),
            "recovered": op.recovered,
        }),
        detail: json!({
            "files": op.files,
            "conflicts": op.conflicts,
            "merge_conflicts": op.merge_conflicts,
            "restores": op.restores,
        }),
        record: to_value(op),
        replay_data: None,
    }
}

fn to_value(record: &impl serde::Serialize) -> Value {
    serde_json::to_value(record).unwrap_or(Value::Null)
}

/// One line explaining the first problem, and how many there are.
#[must_use]
pub fn describe(problem: &Problem, count: usize) -> String {
    let what = match &problem.kind {
        ProblemKind::Missing {
            nearest: Some(window),
        } => {
            format!(
                "anchor matched nothing; nearest text is at line {}",
                window.line
            )
        }
        ProblemKind::Missing { nearest: None } => "anchor matched nothing".to_string(),
        ProblemKind::Ambiguous { lines } => {
            format!("anchor matched {} places, lines {lines:?}", lines.len())
        }
        ProblemKind::Stale { .. } => "file changed since its fingerprint was taken".to_string(),
        ProblemKind::Exists => "target already exists".to_string(),
        ProblemKind::NotFound => "file does not exist".to_string(),
        ProblemKind::BadRange => "range no longer holds the expected text".to_string(),
        ProblemKind::Invalid { message } => message.clone(),
    };
    let more = if count > 1 {
        format!(" (+{} more)", count - 1)
    } else {
        String::new()
    };
    format!(
        "op {} on {}: {what}{more}",
        problem.op,
        problem.path.display()
    )
}

/// Renders an absorb: passed when every chosen file could be absorbed.
#[must_use]
pub fn absorbed(result: &Absorbed) -> Rendered {
    let (outcome, reason) = if result.skipped.is_empty() {
        (Outcome::Passed, None)
    } else {
        (
            Outcome::Failed,
            Some(format!(
                "{} file(s) were deleted or are gone and were not absorbed",
                result.skipped.len()
            )),
        )
    };
    let mut rendered = operation_with(&result.operation, outcome, reason);
    rendered.summary["absorbed"] = json!(result.absorbed.len());
    rendered.summary["skipped"] = json!(result.skipped.len());
    rendered.replay_data = Some(to_value(result));
    rendered
}
