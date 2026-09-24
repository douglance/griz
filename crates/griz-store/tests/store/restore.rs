//! Restoring a span of operations to a point in time.

use crate::common::{Fixture, TestResult, request, undo_request};
use griz_store::{OnStale, Operation, OperationState, RestoreScope};

#[test]
fn restore_since_spans_three_operations_byte_exact() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\ntwo\nthree\n")?;
    let plan1 = fx.plan(vec![fx.replace("a.rs", "one", "ONE")])?;
    let op1 = fx.store.apply(&request(&plan1))?;
    // A formatter run absorbed into op1, so its recorded after text changes
    // without a second operation.
    fx.write("a.rs", "ONE  \ntwo\nthree\n")?;
    fx.store.absorb(&op1.id, &[], "formatted")?;
    let plan2 = fx.plan(vec![fx.replace("a.rs", "two", "TWO")])?;
    let op2 = fx.store.apply(&request(&plan2))?;
    let plan3 = fx.plan(vec![fx.replace("a.rs", "three", "THREE")])?;
    let op3 = fx.store.apply(&request(&plan3))?;
    assert_eq!(fx.read("a.rs")?, "ONE  \nTWO\nTHREE\n");

    let restored = fx.store.restore_since(
        &op1.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[],
        },
        OnStale::Refuse,
        "roll back",
    )?;
    assert_eq!(
        restored.state,
        OperationState::Applied,
        "{:?}",
        restored.reason
    );
    assert_eq!(fx.read("a.rs")?, "one\ntwo\nthree\n");
    let recorded = restored
        .restores
        .as_ref()
        .ok_or("expected a restores record")?;
    assert_eq!(recorded.since, op1.id);
    assert_eq!(
        recorded.operations,
        vec![op1.id.clone(), op2.id.clone(), op3.id.clone()]
    );
    Ok(())
}

#[test]
fn a_broken_chain_refuses_the_whole_restore_unless_narrowed() -> TestResult {
    let fx = Fixture::new()?;
    let op1 = broken_chain(&fx)?;

    let refused = fx.store.restore_since(
        &op1.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[],
        },
        OnStale::Refuse,
        "roll back",
    )?;
    assert_eq!(refused.state, OperationState::Failed);
    assert_eq!(refused.conflicts, vec![fx.path("a.rs")]);
    assert_eq!(fx.read("a.rs")?, "ONE\nTWO\nthree\nFOUR\n");
    assert_eq!(fx.read("b.rs")?, "2\n");

    let narrowed = fx.store.restore_since(
        &op1.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[fx.path("b.rs")],
        },
        OnStale::Refuse,
        "roll back b",
    )?;
    assert_eq!(
        narrowed.state,
        OperationState::Applied,
        "{:?}",
        narrowed.reason
    );
    assert_eq!(fx.read("b.rs")?, "1\n");
    assert_eq!(fx.read("a.rs")?, "ONE\nTWO\nthree\nFOUR\n");
    Ok(())
}

#[test]
fn a_broken_chain_names_the_file_and_offers_no_merge() -> TestResult {
    let fx = Fixture::new()?;
    let op1 = broken_chain(&fx)?;
    // A merge cannot bridge a change made between two writes, so the refusal
    // names the file and does not offer on_stale merge as the way out.
    let merged = fx.store.restore_since(
        &op1.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[],
        },
        OnStale::Merge,
        "roll back merged",
    )?;
    assert_eq!(merged.state, OperationState::Failed);
    let reason = merged.reason.unwrap_or_default();
    assert!(
        reason.contains("a.rs") && reason.contains("outside griz") && !reason.contains("on_stale"),
        "{reason}"
    );
    assert_eq!(fx.read("a.rs")?, "ONE\nTWO\nthree\nFOUR\n");
    Ok(())
}

/// Two operations on `a.rs` with an untracked edit between them, so its
/// chain of writes is broken; `b.rs` is written once. Returns the first.
fn broken_chain(fx: &Fixture) -> Result<Operation, Box<dyn std::error::Error>> {
    fx.write("a.rs", "one\ntwo\nthree\nfour\n")?;
    fx.write("b.rs", "1\n")?;
    let plan1 = fx.plan(vec![
        fx.replace("a.rs", "one", "ONE"),
        fx.replace("b.rs", "1", "2"),
    ])?;
    let op1 = fx.store.apply(&request(&plan1))?;
    let plan2 = fx.plan(vec![fx.replace("a.rs", "two", "TWO")])?;
    // An untracked, non-adjacent edit lands between planning and applying
    // op2, so its merge records a before text op1 never wrote: the chain
    // breaks even though the merge itself is clean.
    fx.write("a.rs", "ONE\ntwo\nthree\nFOUR\n")?;
    let mut merge = request(&plan2);
    merge.on_stale = OnStale::Merge;
    let op2 = fx.store.apply(&merge)?;
    assert_eq!(op2.state, OperationState::Applied, "{:?}", op2.reason);
    assert_eq!(fx.read("a.rs")?, "ONE\nTWO\nthree\nFOUR\n");
    Ok(op1)
}

#[test]
fn undoing_a_restore_returns_to_the_newest_state() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\ntwo\nthree\n")?;
    let plan1 = fx.plan(vec![fx.replace("a.rs", "one", "ONE")])?;
    let op1 = fx.store.apply(&request(&plan1))?;
    let plan2 = fx.plan(vec![fx.replace("a.rs", "two", "TWO")])?;
    fx.store.apply(&request(&plan2))?;
    let plan3 = fx.plan(vec![fx.replace("a.rs", "three", "THREE")])?;
    fx.store.apply(&request(&plan3))?;
    let newest = fx.read("a.rs")?;
    assert_eq!(newest, "ONE\nTWO\nTHREE\n");

    let restored = fx.store.restore_since(
        &op1.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[],
        },
        OnStale::Refuse,
        "roll back",
    )?;
    assert_eq!(fx.read("a.rs")?, "one\ntwo\nthree\n");
    let redone = fx.store.undo(&undo_request(fx.root(), &restored.id))?;
    assert_eq!(redone.state, OperationState::Applied, "{:?}", redone.reason);
    assert_eq!(fx.read("a.rs")?, newest);
    Ok(())
}
