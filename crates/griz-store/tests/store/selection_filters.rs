//! Selection filters operate on whole operations and intersect.

use crate::common::{Fixture, TestResult};
use griz_core::{Confidence, Occurrence, Op};
use griz_store::{PlanRecord, Selection};
use std::error::Error;

fn original(fx: &Fixture) -> Result<PlanRecord, Box<dyn Error>> {
    fx.write("a.txt", "hit hit\n")?;
    fx.write("b.txt", "fuzzy   \n")?;
    fx.write("c.txt", "keep\n")?;
    fx.write("d.txt", "move\n")?;
    let mut all = fx.replace("a.txt", "hit", "HIT");
    let Op::Replace { occurrence, .. } = &mut all else {
        panic!("expected replace")
    };
    *occurrence = Occurrence::All;
    let record = fx.plan(vec![
        all,
        fx.replace("b.txt", "fuzzy\n", "FUZZY\n"),
        fx.replace("c.txt", "missing", "MISSING"),
        Op::Move {
            path: fx.path("d.txt"),
            to: fx.path("e.txt"),
            expect_hash: None,
        },
    ])?;
    assert_eq!(record.problems.len(), 1);
    assert_eq!(record.confidence, Confidence::Maybe);
    Ok(record)
}

#[test]
fn selection_keeps_whole_operations_in_original_order() -> TestResult {
    let fx = Fixture::new()?;
    let original = original(&fx)?;
    let selected = fx.store.select(
        &original.id,
        &Selection {
            edits: vec!["e3.to".into(), "e0.2".into(), "e0.2".into()],
            ..Selection::default()
        },
        "keep two operations",
    )?;
    assert_eq!(
        selected.ops,
        vec![original.ops[0].clone(), original.ops[3].clone()]
    );
    assert!(selected.problems.is_empty());
    let changes = fx.store.plan_changes(&selected)?;
    let a = changes
        .iter()
        .find(|change| change.path == fx.path("a.txt"))
        .ok_or("no a.txt")?;
    assert_eq!(a.after.as_deref(), Some("HIT HIT\n"));
    assert_eq!(selected.edits.len(), 4);
    Ok(())
}

#[test]
fn selection_by_move_destination_keeps_both_sides() -> TestResult {
    let fx = Fixture::new()?;
    let original = original(&fx)?;
    let selected = fx.store.select(
        &original.id,
        &Selection {
            paths: vec![fx.path("e.txt")],
            ..Selection::default()
        },
        "select destination",
    )?;
    assert_eq!(selected.ops, vec![original.ops[3].clone()]);
    assert_eq!(selected.files.len(), 2);
    assert!(selected.problems.is_empty());
    Ok(())
}

#[test]
fn selection_path_edit_and_confidence_filters_intersect() -> TestResult {
    let fx = Fixture::new()?;
    let original = original(&fx)?;
    for selection in [
        Selection {
            paths: vec![fx.path("a.txt")],
            edits: vec!["e1".into()],
            min_confidence: None,
            ..Selection::default()
        },
        Selection {
            edits: vec!["e1".into()],
            min_confidence: Some(Confidence::Machine),
            ..Selection::default()
        },
        Selection {
            edits: vec!["unknown".into()],
            ..Selection::default()
        },
    ] {
        let selected = fx.store.select(&original.id, &selection, "intersection")?;
        assert!(selected.ops.is_empty(), "{:?}", selected.ops);
        assert!(selected.files.is_empty());
    }
    Ok(())
}

#[test]
fn selection_does_not_hide_failed_operations_without_edits() -> TestResult {
    let fx = Fixture::new()?;
    let original = original(&fx)?;
    let all = fx
        .store
        .select(&original.id, &Selection::default(), "all")?;
    assert_eq!(all.ops, original.ops);
    assert_eq!(all.problems.len(), 1);
    let machine = fx.store.select(
        &original.id,
        &Selection {
            min_confidence: Some(Confidence::Machine),
            ..Selection::default()
        },
        "machine",
    )?;
    assert_eq!(
        machine.ops,
        vec![
            original.ops[0].clone(),
            original.ops[2].clone(),
            original.ops[3].clone()
        ]
    );
    assert_eq!(machine.problems.len(), 1);
    assert_eq!(machine.problems[0].op, 1);
    Ok(())
}

#[test]
fn resolved_selection_intersects_paths_and_confidence() -> TestResult {
    let fx = Fixture::new()?;
    let original = original(&fx)?;
    let selected = fx.store.select(
        &original.id,
        &Selection {
            paths: vec![fx.path("a.txt"), fx.path("b.txt"), fx.path("e.txt")],
            min_confidence: Some(Confidence::Machine),
            resolved_only: true,
            ..Selection::default()
        },
        "resolved exact operations",
    )?;
    assert_eq!(
        selected.ops,
        vec![original.ops[0].clone(), original.ops[3].clone()]
    );
    assert!(selected.problems.is_empty());
    let applied = fx.store.apply(&crate::common::request(&selected))?;
    assert_eq!(applied.state, griz_store::OperationState::Applied);
    assert_eq!(fx.read("a.txt")?, "HIT HIT\n");
    assert_eq!(fx.read("b.txt")?, "fuzzy   \n");
    assert_eq!(fx.read("c.txt")?, "keep\n");
    assert!(!fx.path("d.txt").exists());
    assert_eq!(fx.read("e.txt")?, "move\n");
    Ok(())
}

#[test]
fn resolved_selection_rebuilds_and_refuses_missing_prerequisites() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "start   \n")?;
    let original = fx.plan(vec![
        fx.replace("a.txt", "start\n", "middle\n"),
        fx.replace("a.txt", "middle", "done"),
    ])?;
    assert!(original.problems.is_empty());
    assert_eq!(original.edits[0].confidence, Confidence::Maybe);
    assert_eq!(original.edits[1].confidence, Confidence::Machine);
    let selected = fx.store.select(
        &original.id,
        &Selection {
            min_confidence: Some(Confidence::Machine),
            resolved_only: true,
            ..Selection::default()
        },
        "drop prerequisite",
    )?;
    assert_eq!(selected.problems.len(), 1);
    assert_eq!(
        fx.store.apply(&crate::common::request(&selected))?.state,
        griz_store::OperationState::Failed
    );
    assert_eq!(fx.read("a.txt")?, "start   \n");
    Ok(())
}

#[test]
fn resolved_selection_default_preserves_legacy_identity() -> TestResult {
    let legacy = serde_json::json!({"paths":[],"edits":[],"min_confidence":null});
    let selection: Selection = serde_json::from_value(legacy.clone())?;
    assert!(!selection.resolved_only);
    assert_eq!(serde_json::to_value(&selection)?, legacy);
    Ok(())
}
