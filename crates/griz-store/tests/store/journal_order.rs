//! History follows journal insertion, even when IDs were allocated earlier.

use crate::common::{Fixture, TestResult};
use griz_store::{FileWrite, OnStale, Operation, OperationKind, OperationState, RestoreScope};

fn recorded(fx: &Fixture, op: &mut Operation, before: &str, after: &str) -> TestResult {
    op.state = OperationState::Applied;
    op.files = vec![FileWrite {
        path: fx.path("a.txt"),
        before_hash: Some(fx.store.put_blob(before)?),
        after_hash: Some(fx.store.put_blob(after)?),
        merged: false,
    }];
    fx.store.save_operation(op)?;
    Ok(())
}

fn reversed_history(fx: &Fixture) -> Result<(Operation, Operation), Box<dyn std::error::Error>> {
    let mut delayed = Operation::new(OperationKind::Apply, "started first");
    let mut earlier = Operation::new(OperationKind::Apply, "written first");
    assert!(delayed.id < earlier.id);
    recorded(fx, &mut earlier, "zero\n", "one\n")?;
    recorded(fx, &mut delayed, "one\n", "two\n")?;
    fx.write("a.txt", "two\n")?;
    Ok((delayed, earlier))
}

#[test]
fn pagination_and_updates_preserve_journal_order() -> TestResult {
    let fx = Fixture::new()?;
    let (delayed, earlier) = reversed_history(&fx)?;
    let page = fx.store.operations_under(1, None, &[])?;
    assert_eq!(page.operations, vec![delayed.clone()]);
    assert_eq!(page.next, Some(delayed.id.clone()));
    assert_eq!(
        fx.store.operations(1, page.next.as_deref())?,
        vec![earlier.clone()]
    );
    fx.store.save_operation(&earlier)?;
    assert_eq!(
        fx.store.operations(2, None)?,
        vec![delayed.clone(), earlier.clone()]
    );
    Ok(())
}

#[test]
fn history_and_restore_follow_journal_order_instead_of_id_order() -> TestResult {
    let fx = Fixture::new()?;
    let (delayed, earlier) = reversed_history(&fx)?;
    assert_eq!(
        fx.store.operations_since(&delayed.id)?,
        vec![delayed.clone()]
    );
    assert_eq!(
        fx.store.operations_since(&earlier.id)?,
        vec![earlier.clone(), delayed.clone()]
    );
    let restored = fx.store.restore_since(
        &earlier.id,
        RestoreScope {
            root: &fx.root(),
            paths: &[],
        },
        OnStale::Refuse,
        "restore",
    )?;
    assert_eq!(
        restored.state,
        OperationState::Applied,
        "{:?}",
        restored.reason
    );
    assert_eq!(fx.read("a.txt")?, "zero\n");
    assert_eq!(
        restored.restores.ok_or("missing span")?.operations,
        vec![earlier.id, delayed.id]
    );
    Ok(())
}

#[test]
fn unfinished_operations_follow_journal_order() -> TestResult {
    let fx = Fixture::new()?;
    let delayed = Operation::new(OperationKind::Apply, "started first");
    let earlier = Operation::new(OperationKind::Apply, "journaled first");
    assert!(delayed.id < earlier.id);
    fx.store.save_operation(&earlier)?;
    fx.store.save_operation(&delayed)?;
    assert_eq!(fx.store.unfinished_operations()?, vec![earlier, delayed]);
    Ok(())
}
