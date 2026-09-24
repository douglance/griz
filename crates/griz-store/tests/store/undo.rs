//! Undoing operations.

use crate::common::{Fixture, TestResult, request, undo_request};
use griz_core::Op;
use griz_store::{
    ConflictRegion, OnStale, OperationKind, OperationState, UndoRequest, parse_blob_id,
};

#[test]
fn undo_restores_every_file_byte_for_byte() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "one", "ONE"),
        Op::Create {
            path: fx.path("new.rs"),
            text: "n\n".into(),
            overwrite: false,
            expect_hash: None,
        },
    ])?;
    let applied = fx.store.apply(&request(&plan))?;
    let undone = fx.store.undo(&undo_request(fx.root(), &applied.id))?;
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
    let undone = fx.store.undo(&undo_request(fx.root(), &applied.id))?;
    let redone = fx.store.undo(&undo_request(fx.root(), &undone.id))?;
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
    let undone = fx.store.undo(&undo_request(fx.root(), &applied.id))?;
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
    fx.store.undo(&UndoRequest {
        paths: vec![fx.path("b.rs")],
        ..undo_request(fx.root(), &applied.id)
    })?;
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
        fx.store.undo(&undo_request(fx.root(), &failed.id))?.state,
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
    let refused = fx.store.undo(&undo_request(fx.root(), &applied.id))?;
    assert_eq!(refused.state, OperationState::Failed);
    let absorbed = fx.store.absorb(&applied.id, &[], "formatted")?;
    assert_eq!(absorbed.absorbed, vec![fx.path("a.rs")]);
    assert_eq!(absorbed.operation.absorbed, vec![fx.path("a.rs")]);
    let undone = fx.store.undo(&undo_request(fx.root(), &applied.id))?;
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

#[test]
fn merge_policy_reverts_the_undo_around_an_unrelated_later_edit() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\ntwo\nthree\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "one", "ONE")])?;
    let applied = fx.store.apply(&request(&plan))?;
    fx.write("a.rs", "ONE\ntwo\nTHREE\n")?;
    let merged = fx.store.undo(&UndoRequest {
        on_stale: OnStale::Merge,
        ..undo_request(fx.root(), &applied.id)
    })?;
    assert_eq!(merged.state, OperationState::Applied, "{:?}", merged.reason);
    assert!(merged.files[0].merged);
    assert!(merged.merge_conflicts.is_empty());
    assert_eq!(fx.read("a.rs")?, "one\ntwo\nTHREE\n");
    Ok(())
}

#[test]
fn merge_policy_leaves_an_overlapping_later_edit_alone_and_lists_it() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\ntwo\nthree\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "one", "ONE")])?;
    let applied = fx.store.apply(&request(&plan))?;
    fx.write("a.rs", "ONE-ALSO-CHANGED\ntwo\nthree\n")?;
    let refused = fx.store.undo(&UndoRequest {
        on_stale: OnStale::Merge,
        ..undo_request(fx.root(), &applied.id)
    })?;
    assert_eq!(refused.state, OperationState::Failed);
    assert_eq!(refused.conflicts, vec![fx.path("a.rs")]);
    let conflict = refused
        .merge_conflicts
        .first()
        .ok_or("expected one merge conflict")?;
    assert_eq!(conflict.path, fx.path("a.rs"));
    assert_eq!(conflict.regions, vec![ConflictRegion { start: 1, end: 1 }]);
    let base_hash = parse_blob_id(&conflict.base).ok_or("base is not a blob id")?;
    let planned_hash = parse_blob_id(&conflict.planned).ok_or("planned is not a blob id")?;
    let current_hash = parse_blob_id(&conflict.current).ok_or("current is not a blob id")?;
    assert_eq!(current_hash, conflict.current_fingerprint);
    assert_eq!(fx.store.get_blob(base_hash)?, "ONE\ntwo\nthree\n");
    assert_eq!(fx.store.get_blob(planned_hash)?, "one\ntwo\nthree\n");
    assert_eq!(
        fx.store.get_blob(current_hash)?,
        "ONE-ALSO-CHANGED\ntwo\nthree\n"
    );
    assert_eq!(fx.read("a.rs")?, "ONE-ALSO-CHANGED\ntwo\nthree\n");
    Ok(())
}
