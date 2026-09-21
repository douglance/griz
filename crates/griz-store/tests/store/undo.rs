//! Undoing operations.

use crate::common::{Fixture, TestResult, request};
use griz_core::Op;
use griz_store::{OperationKind, OperationState};

#[test]
fn undo_restores_every_file_byte_for_byte() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "one", "ONE"),
        Op::Create {
            path: fx.path("new.rs"),
            text: "n\n".into(),
        },
    ])?;
    let applied = fx.store.apply(&request(&plan))?;
    let undone = fx.store.undo(&applied.id, &[], "revert")?;
    assert_eq!(undone.state, OperationState::Applied, "{:?}", undone.reason);
    assert_eq!(undone.kind, OperationKind::Undo);
    assert_eq!(undone.undoes.as_deref(), Some(applied.id.as_str()));
    assert_eq!(fx.read("a.rs")?, "one\n");
    assert!(!fx.path("new.rs").exists());
    Ok(())
}

#[test]
fn an_undo_can_itself_be_undone() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "one", "ONE")])?;
    let applied = fx.store.apply(&request(&plan))?;
    let undone = fx.store.undo(&applied.id, &[], "revert")?;
    let redone = fx.store.undo(&undone.id, &[], "revert the revert")?;
    assert_eq!(redone.state, OperationState::Applied, "{:?}", redone.reason);
    assert_eq!(fx.read("a.rs")?, "ONE\n");
    Ok(())
}

#[test]
fn undo_leaves_files_changed_since_alone_and_lists_them() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\n")?;
    fx.write("b.rs", "b\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "a", "A"),
        fx.replace("b.rs", "b", "B"),
    ])?;
    let applied = fx.store.apply(&request(&plan))?;
    fx.write("b.rs", "B\nhuman edit\n")?;
    let undone = fx.store.undo(&applied.id, &[], "revert")?;
    assert_eq!(undone.conflicts, vec![fx.path("b.rs")]);
    assert_eq!(fx.read("a.rs")?, "a\n");
    assert_eq!(fx.read("b.rs")?, "B\nhuman edit\n");
    Ok(())
}

#[test]
fn undo_can_restore_only_some_files() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\n")?;
    fx.write("b.rs", "b\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "a", "A"),
        fx.replace("b.rs", "b", "B"),
    ])?;
    let applied = fx.store.apply(&request(&plan))?;
    fx.store.undo(&applied.id, &[fx.path("b.rs")], "revert b")?;
    assert_eq!(
        (fx.read("a.rs")?, fx.read("b.rs")?),
        ("A\n".into(), "b\n".into())
    );
    Ok(())
}

#[test]
fn a_failed_operation_cannot_be_undone() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "missing", "x")])?;
    let failed = fx.store.apply(&request(&plan))?;
    assert_eq!(
        fx.store.undo(&failed.id, &[], "revert")?.state,
        OperationState::Failed
    );
    Ok(())
}
