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

#[test]
fn absorbing_a_formatter_run_keeps_undo_working() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "fn  f() {}\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "f()", "g()")])?;
    let applied = fx.store.apply(&request(&plan))?;
    fx.write("a.rs", "fn g() {}\n")?;
    let refused = fx.store.undo(&applied.id, &[], "revert before absorbing")?;
    assert_eq!(refused.state, OperationState::Failed);
    let absorbed = fx.store.absorb(&applied.id, &[], "formatted")?;
    assert_eq!(absorbed.absorbed, vec![fx.path("a.rs")]);
    assert_eq!(absorbed.operation.absorbed, vec![fx.path("a.rs")]);
    let undone = fx.store.undo(&applied.id, &[], "revert")?;
    assert_eq!(undone.state, OperationState::Applied, "{:?}", undone.reason);
    assert_eq!(fx.read("a.rs")?, "fn  f() {}\n");
    Ok(())
}

#[test]
fn absorbing_skips_deleted_files_and_changes_nothing_on_disk() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("gone.rs", "g\n")?;
    let plan = fx.plan(vec![Op::Delete {
        path: fx.path("gone.rs"),
        expect_hash: None,
    }])?;
    let applied = fx.store.apply(&request(&plan))?;
    fx.write("gone.rs", "back\n")?;
    let absorbed = fx.store.absorb(&applied.id, &[], "formatted")?;
    assert_eq!(absorbed.skipped, vec![fx.path("gone.rs")]);
    assert_eq!(fx.read("gone.rs")?, "back\n");
    Ok(())
}

#[test]
fn absorb_on_an_unapplied_operation_errors() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "missing", "x")])?;
    let failed = fx.store.apply(&request(&plan))?;
    assert_eq!(failed.state, OperationState::Failed);
    assert!(fx.store.absorb(&failed.id, &[], "formatted").is_err());
    Ok(())
}

#[test]
fn absorb_errors_on_a_path_the_operation_never_wrote() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\n")?;
    fx.write("untouched.rs", "u\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "a", "A")])?;
    let applied = fx.store.apply(&request(&plan))?;
    let error = fx
        .store
        .absorb(&applied.id, &[fx.path("untouched.rs")], "formatted")
        .err()
        .ok_or("expected an error for a path the operation never wrote")?;
    assert!(error.to_string().contains("untouched.rs"), "{error}");
    Ok(())
}
