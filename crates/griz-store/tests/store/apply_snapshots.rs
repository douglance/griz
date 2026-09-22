//! Snapshot loading must finish before any apply writes a source file.

use crate::common::{Fixture, TestResult, request};
use griz_core::content_hash;
use griz_store::{OnStale, OperationState, StoreError};
use std::fs;

fn missing_snapshot(text: &str) -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    fx.write("z.txt", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("z.txt", "two", "TWO"),
    ])?;
    let hash = content_hash(text);
    let snapshot = fx
        .store
        .home()
        .join("blobs")
        .join(&hash[..2])
        .join(&hash[2..]);
    fs::remove_file(&snapshot)?;
    for policy in [OnStale::Refuse, OnStale::Merge] {
        let mut apply = request(&plan);
        apply.on_stale = policy;
        assert!(matches!(
            fx.store.apply(&apply),
            Err(StoreError::NotFound(_))
        ));
        assert_eq!(fx.read("a.txt")?, "one\n");
        assert_eq!(fx.read("z.txt")?, "two\n");
        assert!(!snapshot.exists());
    }
    Ok(())
}

#[test]
fn missing_later_base_snapshot_blocks_every_write() -> TestResult {
    missing_snapshot("two\n")
}

#[test]
fn missing_later_planned_snapshot_blocks_every_write() -> TestResult {
    missing_snapshot("TWO\n")
}

fn merge_fixture() -> Result<(Fixture, griz_store::PlanRecord), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    for name in ["a.txt", "z.txt"] {
        fx.write(name, "a\nb\nc\n")?;
    }
    let plan = fx.plan(vec![
        fx.replace("a.txt", "a\n", "A\n"),
        fx.replace("z.txt", "a\n", "A\n"),
    ])?;
    fx.write("a.txt", "a\nb\nC\n")?;
    fx.write("z.txt", "Z\nb\nc\n")?;
    Ok((fx, plan))
}

#[test]
fn a_later_merge_conflict_blocks_an_earlier_clean_merge() -> TestResult {
    let (fx, plan) = merge_fixture()?;
    let mut apply = request(&plan);
    apply.on_stale = OnStale::Merge;
    let operation = fx.store.apply(&apply)?;
    assert_eq!(operation.state, OperationState::Failed);
    assert!(operation.files.is_empty());
    assert_eq!(operation.conflicts, vec![fx.path("z.txt")]);
    assert_eq!(fx.read("a.txt")?, "a\nb\nC\n");
    assert_eq!(fx.read("z.txt")?, "Z\nb\nc\n");
    assert_eq!(fx.store.operation(&operation.id)?, operation);
    check_conflict(&fx, &operation);
    Ok(())
}

fn check_conflict(fx: &Fixture, operation: &griz_store::Operation) {
    assert_eq!(operation.merge_conflicts.len(), 1);
    let conflict = &operation.merge_conflicts[0];
    assert_eq!(conflict.path, fx.path("z.txt"));
    assert_eq!(conflict.base, format!("blob_{}", content_hash("a\nb\nc\n")));
    assert_eq!(
        conflict.planned,
        format!("blob_{}", content_hash("A\nb\nc\n"))
    );
    assert_eq!(
        conflict.current,
        format!("blob_{}", content_hash("Z\nb\nc\n"))
    );
    assert!(!conflict.regions.is_empty());
}
