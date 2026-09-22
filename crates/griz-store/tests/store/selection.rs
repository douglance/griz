//! Selections use the original plan's inputs, including unchanged files.

use crate::common::{Fixture, TestResult, request};
use griz_core::content_hash;
use griz_store::{OperationState, Selection};
#[test]
fn selection_preserves_absence_after_a_cancelled_create() -> TestResult {
    let fx = Fixture::new()?;
    let plan = fx.plan(vec![
        griz_core::Op::Create {
            path: fx.path("a.txt"),
            text: "planned\n".into(),
            overwrite: false,
        },
        griz_core::Op::Delete {
            path: fx.path("a.txt"),
            expect_hash: None,
        },
    ])?;
    assert!(plan.files.is_empty());
    fx.write("a.txt", "concurrent\n")?;
    let selected = fx.store.select(&plan.id, &first_edit(), "restore create")?;
    assert!(selected.problems.is_empty(), "{:?}", selected.problems);
    assert_eq!(selected.files.len(), 1);
    assert_eq!(selected.files[0].before_hash, None);
    assert_eq!(
        fx.store.apply(&request(&selected))?.state,
        OperationState::Failed
    );
    assert_eq!(fx.read("a.txt")?, "concurrent\n");
    Ok(())
}

#[test]
fn selection_preserves_both_inputs_of_a_move_round_trip() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "original\n")?;
    let plan = fx.plan(vec![
        griz_core::Op::Move {
            path: fx.path("a.txt"),
            to: fx.path("b.txt"),
            expect_hash: None,
        },
        griz_core::Op::Move {
            path: fx.path("b.txt"),
            to: fx.path("a.txt"),
            expect_hash: None,
        },
    ])?;
    assert!(plan.files.is_empty());
    fx.write("b.txt", "concurrent\n")?;
    let selected = fx.store.select(
        &plan.id,
        &Selection {
            edits: vec!["e0.to".into()],
            ..Selection::default()
        },
        "keep first move",
    )?;
    assert!(selected.problems.is_empty(), "{:?}", selected.problems);
    assert_eq!(selected.files.len(), 2);
    assert_eq!(
        fx.store.apply(&request(&selected))?.state,
        OperationState::Failed
    );
    assert_eq!(fx.read("a.txt")?, "original\n");
    assert_eq!(fx.read("b.txt")?, "concurrent\n");
    Ok(())
}

#[test]
fn repeated_selection_preserves_unchanged_inputs() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("a.txt", "ONE", "one"),
    ])?;
    let all = fx
        .store
        .select(&plan.id, &Selection::default(), "keep both")?;
    assert!(all.files.is_empty());
    fx.write("a.txt", "one\nconcurrent\n")?;
    let selected = fx.store.select(&all.id, &first_edit(), "keep first")?;
    assert!(selected.problems.is_empty(), "{:?}", selected.problems);
    assert_eq!(selected.files[0].before_hash, Some(content_hash("one\n")));
    assert_eq!(
        fx.store.apply(&request(&selected))?.state,
        OperationState::Failed
    );
    assert_eq!(fx.read("a.txt")?, "one\nconcurrent\n");
    Ok(())
}

#[test]
fn legacy_selection_refuses_an_unrecorded_input() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    let ops = vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("a.txt", "ONE", "one"),
    ];
    let mut built = griz_core::build_plan(&ops, &griz_core::DiskSource);
    built.unchanged_inputs.clear();
    let legacy = fx.store.save_plan("legacy record", ops, built)?;
    assert!(
        serde_json::to_value(&legacy)?
            .get("unchanged_inputs")
            .is_none()
    );
    fx.write("a.txt", "one\nconcurrent\n")?;
    let selected = fx.store.select(&legacy.id, &first_edit(), "keep first")?;
    assert_eq!(selected.problems.len(), 1);
    let griz_core::ProblemKind::Invalid { message } = &selected.problems[0].kind else {
        panic!("expected missing snapshot: {:?}", selected.problems);
    };
    assert!(message.contains("not recorded in this plan"), "{message}");
    assert_eq!(
        fx.store.apply(&request(&selected))?.state,
        OperationState::Failed
    );
    assert_eq!(fx.read("a.txt")?, "one\nconcurrent\n");
    Ok(())
}

#[test]
fn legacy_selection_uses_available_changed_file_snapshots() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    let legacy = fx.plan(vec![fx.replace("a.txt", "one", "ONE")])?;
    assert!(
        serde_json::to_value(&legacy)?
            .get("unchanged_inputs")
            .is_none()
    );
    fx.write("a.txt", "one\nconcurrent\n")?;
    let selected = fx.store.select(&legacy.id, &first_edit(), "keep first")?;
    assert!(selected.problems.is_empty(), "{:?}", selected.problems);
    assert_eq!(selected.files[0].before_hash, Some(content_hash("one\n")));
    assert_eq!(
        fx.store.apply(&request(&selected))?.state,
        OperationState::Failed
    );
    assert_eq!(fx.read("a.txt")?, "one\nconcurrent\n");
    Ok(())
}

fn first_edit() -> Selection {
    Selection {
        edits: vec!["e0".into()],
        ..Selection::default()
    }
}

#[test]
fn selection_from_a_net_zero_plan_keeps_the_original_snapshot() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("a.txt", "ONE", "one"),
    ])?;
    assert!(plan.files.is_empty());
    assert_eq!(plan.edits.len(), 2);
    fx.write("a.txt", "one\nconcurrent\n")?;
    let selected = fx.store.select(
        &plan.id,
        &Selection {
            edits: vec!["e0".to_string()],
            ..Selection::default()
        },
        "keep the first edit",
    )?;
    assert!(selected.problems.is_empty(), "{:?}", selected.problems);
    assert_eq!(selected.files.len(), 1);
    assert_eq!(selected.files[0].before_hash, Some(content_hash("one\n")));
    let operation = fx.store.apply(&request(&selected))?;
    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(operation.conflicts, vec![fx.path("a.txt")]);
    assert_eq!(fx.read("a.txt")?, "one\nconcurrent\n");
    Ok(())
}
