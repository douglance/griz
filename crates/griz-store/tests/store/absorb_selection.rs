//! Indexed absorb selection preserves caller validation and journal order.

use crate::common::{Fixture, TestResult, request, undo_request};
use griz_core::content_hash;
use griz_store::{Operation, OperationState, StoreError};
use std::error::Error;

fn fixture(fx: &Fixture) -> Result<Operation, Box<dyn Error>> {
    let mut ops = Vec::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fx.write(name, "before\n")?;
        ops.push(fx.replace(name, "before", "after"));
    }
    let plan = fx.plan(ops)?;
    let operation = fx.store.apply(&request(&plan))?;
    assert_eq!(operation.state, OperationState::Applied);
    for name in ["a.txt", "b.txt", "c.txt"] {
        fx.write(name, "formatted\n")?;
    }
    Ok(operation)
}

#[test]
fn duplicate_reversed_paths_keep_file_order_and_unselected_fingerprints() -> TestResult {
    let fx = Fixture::new()?;
    let original = fixture(&fx)?;
    let paths = [fx.path("c.txt"), fx.path("a.txt"), fx.path("c.txt")];
    let result = fx.store.absorb(&original.id, &paths, "selected")?;
    assert_eq!(result.absorbed, vec![fx.path("a.txt"), fx.path("c.txt")]);
    assert!(result.skipped.is_empty());
    assert_eq!(result.operation.files[1], original.files[1]);
    let expected = Some(content_hash("formatted\n"));
    assert_eq!(result.operation.files[0].after_hash, expected);
    assert_eq!(result.operation.files[2].after_hash, expected);
    let undone = fx.store.undo(&undo_request(&original.id))?;
    assert_eq!(undone.conflicts, vec![fx.path("b.txt")]);
    assert_eq!(fx.read("a.txt")?, "before\n");
    assert_eq!(fx.read("b.txt")?, "formatted\n");
    assert_eq!(fx.read("c.txt")?, "before\n");
    Ok(())
}

#[test]
fn absorb_history_keeps_first_absorbed_order_without_duplicates() -> TestResult {
    let fx = Fixture::new()?;
    let original = fixture(&fx)?;
    fx.store
        .absorb(&original.id, &[fx.path("c.txt")], "first")?;
    let result = fx.store.absorb(&original.id, &[], "second")?;
    assert_eq!(result.absorbed, vec![fx.path("a.txt"), fx.path("b.txt")]);
    assert_eq!(
        result.operation.absorbed,
        vec![fx.path("c.txt"), fx.path("a.txt"), fx.path("b.txt")]
    );
    fx.write("a.txt", "formatted again\n")?;
    let again = fx.store.absorb(&original.id, &[], "third")?;
    assert_eq!(again.absorbed, vec![fx.path("a.txt")]);
    assert_eq!(again.operation.absorbed, result.operation.absorbed);
    assert_eq!(again.operation.absorb_purpose.as_deref(), Some("third"));
    Ok(())
}

#[test]
fn invalid_selection_reports_first_unknown_path_without_record_changes() -> TestResult {
    let fx = Fixture::new()?;
    let original = fixture(&fx)?;
    let first_unknown = fx.path("z-missing.txt");
    let paths = [
        fx.path("a.txt"),
        first_unknown.clone(),
        fx.path("d-missing.txt"),
    ];
    let result = fx.store.absorb(&original.id, &paths, "invalid");
    let Err(StoreError::Invalid(message)) = result else {
        panic!("unknown selection was not refused");
    };
    assert_eq!(
        message,
        format!(
            "absorb: `{}` is not a file this operation wrote",
            first_unknown.display()
        )
    );
    assert_eq!(fx.store.operation(&original.id)?, original);
    assert_eq!(fx.read("a.txt")?, "formatted\n");
    Ok(())
}
