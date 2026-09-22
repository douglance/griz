//! Selection reads snapshots only for operations it keeps.

use crate::common::{Fixture, TestResult};
use griz_core::content_hash;
use griz_store::{Selection, StoreError};
use std::fs;

fn remove_snapshot(fx: &Fixture, text: &str) -> TestResult {
    let hash = content_hash(text);
    fs::remove_file(
        fx.store
            .home()
            .join("blobs")
            .join(&hash[..2])
            .join(&hash[2..]),
    )?;
    Ok(())
}

fn choose(fx: &Fixture, name: &str) -> Selection {
    Selection {
        paths: vec![fx.path(name)],
        ..Selection::default()
    }
}

#[test]
fn unselected_changed_snapshots_are_not_required() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    fx.write("b.txt", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("b.txt", "two", "TWO"),
    ])?;
    remove_snapshot(&fx, "two\n")?;
    fx.write("a.txt", "concurrent\n")?;
    let selected = fx.store.select(&plan.id, &choose(&fx, "a.txt"), "keep a")?;
    assert!(selected.problems.is_empty());
    let changes = fx.store.plan_changes(&selected)?;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].before.as_deref(), Some("one\n"));
    assert_eq!(changes[0].after.as_deref(), Some("ONE\n"));
    assert_eq!(fx.read("a.txt")?, "concurrent\n");
    Ok(())
}

#[test]
fn unselected_net_zero_snapshots_are_not_required() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    fx.write("b.txt", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("b.txt", "two", "TWO"),
        fx.replace("b.txt", "TWO", "two"),
    ])?;
    assert!(plan.unchanged_inputs.contains_key(&fx.path("b.txt")));
    remove_snapshot(&fx, "two\n")?;
    let selected = fx.store.select(&plan.id, &choose(&fx, "a.txt"), "keep a")?;
    assert!(selected.problems.is_empty());
    assert_eq!(selected.files.len(), 1);
    assert_eq!(selected.files[0].path, fx.path("a.txt"));
    assert!(selected.unchanged_inputs.is_empty());
    Ok(())
}

#[test]
fn required_missing_snapshots_still_return_store_errors() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    fx.write("b.txt", "two\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "one", "ONE"),
        fx.replace("b.txt", "two", "TWO"),
    ])?;
    remove_snapshot(&fx, "one\n")?;
    for selection in [choose(&fx, "a.txt"), Selection::default()] {
        assert!(matches!(
            fx.store
                .select(&plan.id, &selection, "missing selected input"),
            Err(StoreError::NotFound(_))
        ));
    }
    assert_eq!(fx.read("a.txt")?, "one\n");
    Ok(())
}

#[test]
fn an_empty_selection_does_not_load_any_snapshot() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "one\n")?;
    let plan = fx.plan(vec![fx.replace("a.txt", "one", "ONE")])?;
    remove_snapshot(&fx, "one\n")?;
    let selected = fx
        .store
        .select(&plan.id, &choose(&fx, "absent.txt"), "keep none")?;
    assert!(selected.ops.is_empty());
    assert!(selected.files.is_empty());
    assert!(selected.problems.is_empty());
    assert_eq!(fx.read("a.txt")?, "one\n");
    Ok(())
}
