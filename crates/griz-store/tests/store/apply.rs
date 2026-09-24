//! Applying plans.

use crate::common::{Fixture, TestResult, request};
use griz_core::{Confidence, Op};
use griz_store::{OnStale, OperationState, Selection};

#[test]
fn apply_writes_every_file_and_journals_hashes() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\n")?;
    fx.write("b.rs", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "one", "ONE"),
        fx.replace("b.rs", "two", "TWO"),
    ])?;
    let op = fx.store.apply(&request(&plan))?;
    assert_eq!(op.state, OperationState::Applied, "{:?}", op.reason);
    assert_eq!(
        (fx.read("a.rs")?, fx.read("b.rs")?),
        ("ONE\n".into(), "TWO\n".into())
    );
    assert_eq!(op.files.len(), 2);
    assert_eq!(fx.store.operation(&op.id)?, op);
    Ok(())
}

#[test]
fn a_plan_with_any_problem_writes_nothing() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\n")?;
    fx.write("b.rs", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "one", "ONE"),
        fx.replace("b.rs", "missing", "x"),
    ])?;
    let op = fx.store.apply(&request(&plan))?;
    assert_eq!(op.state, OperationState::Failed);
    assert_eq!(fx.read("a.rs")?, "one\n");
    Ok(())
}

#[test]
fn a_file_changed_after_planning_blocks_the_whole_apply() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "one\n")?;
    fx.write("b.rs", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "one", "ONE"),
        fx.replace("b.rs", "two", "TWO"),
    ])?;
    fx.write("b.rs", "two\nsomeone else\n")?;
    let op = fx.store.apply(&request(&plan))?;
    assert_eq!(op.state, OperationState::Failed);
    assert_eq!(op.conflicts, vec![fx.path("b.rs")]);
    assert_eq!(fx.read("a.rs")?, "one\n");
    Ok(())
}

#[test]
fn merge_policy_lands_the_plan_on_a_file_that_moved_on() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\nb\nc\n")?;
    let plan = fx.plan(vec![fx.replace("a.rs", "a\n", "A\n")])?;
    fx.write("a.rs", "a\nb\nC\n")?;
    let mut merge = request(&plan);
    merge.on_stale = OnStale::Merge;
    let op = fx.store.apply(&merge)?;
    assert_eq!(op.state, OperationState::Applied, "{:?}", op.reason);
    assert!(op.files[0].merged);
    assert_eq!(fx.read("a.rs")?, "A\nb\nC\n");
    Ok(())
}

#[test]
fn tolerant_edits_are_blocked_until_selected_or_accepted() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "exact\n")?;
    fx.write("b.rs", "fuzzy   \n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "exact", "EXACT"),
        fx.replace("b.rs", "fuzzy\n", "FUZZY\n"),
    ])?;
    assert_eq!(plan.confidence, Confidence::Maybe);
    assert_eq!(
        fx.store.apply(&request(&plan))?.state,
        OperationState::Failed
    );
    assert_eq!(fx.read("a.rs")?, "exact\n");

    let only_exact = Selection {
        min_confidence: Some(Confidence::Machine),
        ..Selection::default()
    };
    let selected = fx.store.select(&plan.id, &only_exact, "keep exact edits")?;
    assert_eq!(selected.selected_from.as_deref(), Some(plan.id.as_str()));
    assert_eq!(
        fx.store.apply(&request(&selected))?.state,
        OperationState::Applied
    );
    assert_eq!(
        (fx.read("a.rs")?, fx.read("b.rs")?),
        ("EXACT\n".into(), "fuzzy   \n".into())
    );
    Ok(())
}

#[test]
fn selection_by_path_keeps_only_those_files() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "a\n")?;
    fx.write("b.rs", "b\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.rs", "a", "A"),
        fx.replace("b.rs", "b", "B"),
    ])?;
    let by_path = Selection {
        paths: vec![fx.path("b.rs")],
        ..Selection::default()
    };
    let selected = fx.store.select(&plan.id, &by_path, "only b")?;
    assert_eq!(selected.files.len(), 1);
    fx.store.apply(&request(&selected))?;
    assert_eq!(
        (fx.read("a.rs")?, fx.read("b.rs")?),
        ("a\n".into(), "B\n".into())
    );
    Ok(())
}

#[test]
fn create_and_delete_apply_as_file_operations() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("gone.rs", "g\n")?;
    let plan = fx.plan(vec![
        Op::Create {
            path: fx.path("dir/new.rs"),
            text: "n\n".into(),
            overwrite: false,
            expect_hash: None,
        },
        Op::Delete {
            path: fx.path("gone.rs"),
            expect_hash: None,
        },
    ])?;
    assert_eq!(
        fx.store.apply(&request(&plan))?.state,
        OperationState::Applied
    );
    assert_eq!(fx.read("dir/new.rs")?, "n\n");
    assert!(!crate::common::exists(&fx.path("gone.rs")));
    Ok(())
}
