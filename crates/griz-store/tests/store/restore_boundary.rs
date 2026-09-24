//! A restore span must start at an operation that exists.

use crate::common::{Fixture, TestResult, request};
use griz_store::{OnStale, RestoreScope, StoreError};
#[test]
fn a_failed_operation_can_bound_later_applied_writes() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "zero\n")?;
    let before = fx.plan(vec![fx.replace("a.txt", "zero", "one")])?;
    fx.store.apply(&request(&before))?;
    let invalid = fx.plan(vec![fx.replace("a.txt", "missing", "unused")])?;
    let boundary = fx.store.apply(&request(&invalid))?;
    assert_eq!(boundary.state, griz_store::OperationState::Failed);
    let later = fx.plan(vec![fx.replace("a.txt", "one", "two")])?;
    let applied = fx.store.apply(&request(&later))?;
    let restored = fx.store.restore_since(
        &boundary.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[],
        },
        OnStale::Refuse,
        "later writes",
    )?;
    assert_eq!(restored.state, griz_store::OperationState::Applied);
    assert_eq!(fx.read("a.txt")?, "one\n");
    let span = restored.restores.ok_or("no restore span")?;
    assert_eq!(span.operations, vec![applied.id]);
    Ok(())
}

#[test]
fn restore_since_unknown_id_writes_nothing_and_creates_no_operation() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "before\n")?;
    let plan = fx.plan(vec![fx.replace("a.txt", "before", "after")])?;
    let applied = fx.store.apply(&request(&plan))?;
    for missing in [
        "",
        "op_",
        "op_00000000000000000000000000000000",
        "op_ffffffffffffffffffffffffffffffff",
    ] {
        let result = fx.store.restore_since(
            missing,
            RestoreScope {
                root: &fx.root(),
                paths: &[],
            },
            OnStale::Refuse,
            "invalid boundary",
        );
        let Err(StoreError::NotFound(message)) = result else {
            panic!("expected missing operation, got {result:?}");
        };
        assert!(message.contains(missing));
        assert_eq!(fx.read("a.txt")?, "after\n");
        assert_eq!(fx.store.operations(10, None)?, vec![applied.clone()]);
    }
    Ok(())
}
