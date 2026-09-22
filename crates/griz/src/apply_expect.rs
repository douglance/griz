//! Checks an apply's declared file count before source writes.

use crate::{
    context::CmdError,
    render,
    verdict::{Outcome, Rendered, unmet},
};
use griz_store::{ApplyRequest, Operation, OperationKind, OperationState, Store};

/// Applies a plan only when its file count matches the caller's expectation.
///
/// # Errors
/// Returns errors reading the plan, recording the refusal, or applying it.
pub fn apply(
    store: &Store,
    request: &ApplyRequest,
    expected: Option<usize>,
) -> Result<Rendered, CmdError> {
    if let Some(expected) = expected {
        let plan = store.plan(&request.plan)?;
        let reason = unmet("files", Some(expected), plan.files.len())
            .filter(|_| plan.problems.is_empty() && plan.confidence >= request.min_confidence);
        if let Some(reason) = reason {
            return refuse_count(store, request, &reason);
        }
    }
    Ok(render::operation(&store.apply(request)?, expected))
}

fn refuse_count(store: &Store, request: &ApplyRequest, reason: &str) -> Result<Rendered, CmdError> {
    let mut operation = Operation::new(OperationKind::Apply, &request.purpose);
    operation.plan = Some(request.plan.clone());
    operation.state = OperationState::Failed;
    operation.reason = Some(format!("{reason}; no files written"));
    store.save_operation(&operation)?;
    Ok(render::operation_with(&operation, Outcome::Failed, None))
}
