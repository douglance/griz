//! `create` with `overwrite`: replacing a whole file without measuring it.

use crate::common::{source, source_with_absent};
use griz_core::{Op, ProblemKind, build_plan};
use std::path::PathBuf;

#[test]
fn overwrite_replaces_a_whole_file_without_a_range() {
    let source = source(&[("a.rs", "fn old() {}\n")]);
    let plan = build_plan(
        &[Op::Create {
            path: PathBuf::from("a.rs"),
            text: "fn new() {}\n".into(),
            overwrite: true,
            expect_hash: None,
        }],
        &source,
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.files[0].after.as_deref(), Some("fn new() {}\n"));
    assert_eq!(plan.files[0].before.as_deref(), Some("fn old() {}\n"));
}

#[test]
fn create_without_overwrite_still_refuses_an_existing_file() {
    let source = source(&[("a.rs", "fn old() {}\n")]);
    let plan = build_plan(
        &[Op::Create {
            path: PathBuf::from("a.rs"),
            text: "fn new() {}\n".into(),
            overwrite: false,
            expect_hash: None,
        }],
        &source,
    );
    assert!(matches!(plan.problems[0].kind, ProblemKind::Exists));
}

#[test]
fn overwrite_creates_a_file_that_does_not_exist() {
    let source = source_with_absent(&[], &["new.rs"]);
    let plan = build_plan(
        &[Op::Create {
            path: PathBuf::from("new.rs"),
            text: "fn new() {}\n".into(),
            overwrite: true,
            expect_hash: None,
        }],
        &source,
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.files[0].before, None);
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn guarded_overwrite(expected: &str) -> Result<Op, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "op": "create", "path": "a.txt", "text": "replacement",
        "overwrite": true, "expect_hash": griz_core::content_hash(expected)
    }))
}

#[test]
fn overwrite_guard_accepts_the_observed_file() -> TestResult {
    let plan = build_plan(
        &[guarded_overwrite("observed")?],
        &source(&[("a.txt", "observed")]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.files[0].before.as_deref(), Some("observed"));
    assert_eq!(plan.files[0].after.as_deref(), Some("replacement"));
    Ok(())
}

#[test]
fn overwrite_guard_rejects_a_stale_caller_read() -> TestResult {
    let plan = build_plan(
        &[guarded_overwrite("observed")?],
        &source(&[("a.txt", "newer")]),
    );
    assert!(plan.files.is_empty());
    assert!(plan.edits.is_empty());
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: griz_core::content_hash("observed"),
            actual: Some(griz_core::content_hash("newer")),
        }
    );
    Ok(())
}

#[test]
fn overwrite_guard_refuses_a_deleted_file_even_if_it_was_empty() -> TestResult {
    let plan = build_plan(
        &[guarded_overwrite("")?],
        &source_with_absent(&[], &["a.txt"]),
    );
    assert!(plan.files.is_empty());
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: griz_core::content_hash(""),
            actual: None,
        }
    );
    Ok(())
}

#[test]
fn overwrite_guard_checks_the_original_not_an_earlier_edit() -> TestResult {
    let ops = [
        crate::common::replace("a.txt", "observed", "intermediate"),
        guarded_overwrite("observed")?,
        guarded_overwrite("replacement")?,
    ];
    let plan = build_plan(&ops, &source(&[("a.txt", "observed")]));
    assert_eq!(plan.edits.len(), 2);
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(plan.problems[0].op, 2);
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: griz_core::content_hash("replacement"),
            actual: Some(griz_core::content_hash("observed")),
        }
    );
    assert_eq!(plan.files[0].after.as_deref(), Some("replacement"));
    Ok(())
}

#[test]
fn overwrite_guard_does_not_treat_an_earlier_create_as_original() -> TestResult {
    let first: Op = serde_json::from_value(serde_json::json!({
        "op": "create", "path": "a.txt", "text": "observed"
    }))?;
    let plan = build_plan(
        &[first, guarded_overwrite("observed")?],
        &source_with_absent(&[], &["a.txt"]),
    );
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: griz_core::content_hash("observed"),
            actual: None,
        }
    );
    assert_eq!(plan.files[0].after.as_deref(), Some("observed"));
    Ok(())
}
