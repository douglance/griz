//! Cursor boundaries and filtered pages follow insertion order.

use crate::common::{Fixture, TestResult};
use griz_store::{FileWrite, Operation, OperationKind, OperationState};
use rusqlite::{Connection, params};

pub fn seed(fx: &Fixture, count: usize, stride: usize) -> TestResult {
    let mut conn = Connection::open(fx.store.home().join("griz.sqlite3"))?;
    let tx = conn.transaction()?;
    let mut insert = tx.prepare(
        "INSERT INTO operations (id, created_at, state, body) VALUES (?1, ?2, 'applied', ?3)",
    )?;
    for index in 1..=count {
        let mut op = Operation::new(OperationKind::Apply, &index.to_string());
        op.id = format!("op_{:032x}", count - index);
        op.state = OperationState::Applied;
        let directory = if index % stride == 0 { "keep" } else { "other" };
        op.files = vec![FileWrite {
            path: fx.path(directory).join("a.txt"),
            before_hash: None,
            after_hash: None,
            merged: false,
        }];
        insert.execute(params![op.id, op.created_at, serde_json::to_string(&op)?])?;
    }
    drop(insert);
    tx.commit()?;
    Ok(())
}

fn purposes(ops: &[Operation]) -> Vec<&str> {
    ops.iter().map(|op| op.purpose.as_str()).collect()
}

#[test]
fn pages_exclude_the_cursor_and_missing_cursors_are_empty() -> TestResult {
    let fx = Fixture::new()?;
    seed(&fx, 5, 2)?;
    let first = fx.store.operations(2, None)?;
    assert_eq!(purposes(&first), vec!["5", "4"]);
    let second = fx.store.operations(2, Some(&first[1].id))?;
    assert_eq!(purposes(&second), vec!["3", "2"]);
    let last = fx.store.operations(2, Some(&second[1].id))?;
    assert_eq!(purposes(&last), vec!["1"]);
    assert!(fx.store.operations(2, Some(&last[0].id))?.is_empty());
    assert!(fx.store.operations(2, Some("op_missing"))?.is_empty());
    assert!(fx.store.operations(0, None)?.is_empty());
    Ok(())
}

#[test]
fn filtered_pages_cross_scan_batches_without_repeating_records() -> TestResult {
    let fx = Fixture::new()?;
    seed(&fx, 150, 70)?;
    let under = [fx.path("keep")];
    let first = fx.store.operations_under(1, None, &under)?;
    assert_eq!(purposes(&first.operations), vec!["140"]);
    let second = fx
        .store
        .operations_under(1, first.next.as_deref(), &under)?;
    assert_eq!(purposes(&second.operations), vec!["70"]);
    let last = fx
        .store
        .operations_under(1, second.next.as_deref(), &under)?;
    assert!(last.operations.is_empty());
    assert!(last.next.is_none());
    let missing = fx.store.operations_under(1, Some("op_missing"), &under)?;
    assert!(missing.operations.is_empty());
    assert!(missing.next.is_none());
    Ok(())
}
