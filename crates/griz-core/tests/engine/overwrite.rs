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
        }],
        &source,
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.files[0].before, None);
}
