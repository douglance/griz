//! Every operation checks its own guard against the original file.

use crate::common::{replace, source, source_with_absent};
use griz_core::{Op, ProblemKind, build_plan, content_hash};
use std::path::PathBuf;

fn guarded(find: &str, replacement: &str, expected: &str) -> Op {
    let mut op = replace("a.txt", find, replacement);
    let Op::Replace { expect_hash, .. } = &mut op else {
        panic!("replace fixture must return a replace operation");
    };
    *expect_hash = Some(content_hash(expected));
    op
}

#[test]
fn each_guard_uses_the_original_fingerprint_after_prior_edits() {
    let before = "one two\n";
    let ops = [
        guarded("one", "ONE", before),
        guarded("two", "wrong", "ONE two\n"),
        guarded("two", "TWO", before),
    ];
    let plan = build_plan(&ops, &source(&[("a.txt", before)]));
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(plan.problems[0].op, 1);
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: content_hash("ONE two\n"),
            actual: Some(content_hash(before)),
        }
    );
    assert_eq!(plan.files[0].after.as_deref(), Some("ONE TWO\n"));
    assert_eq!(plan.files[0].before_hash, Some(content_hash(before)));
}

#[test]
fn bad_guard_does_not_poison_a_later_valid_guard() {
    let before = "one two\n";
    let ops = [
        guarded("one", "wrong", "stale"),
        guarded("two", "TWO", before),
    ];
    let plan = build_plan(&ops, &source(&[("a.txt", before)]));
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(plan.problems[0].op, 0);
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.files[0].after.as_deref(), Some("one TWO\n"));
}

#[test]
fn a_new_files_original_fingerprint_stays_absent() {
    let ops = [
        Op::Create {
            path: PathBuf::from("a.txt"),
            text: "one".to_string(),
            overwrite: false,
        },
        guarded("one", "wrong", "one"),
    ];
    let plan = build_plan(&ops, &source_with_absent(&[], &["a.txt"]));
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: content_hash("one"),
            actual: None,
        }
    );
    assert_eq!(plan.files[0].before_hash, None);
    assert_eq!(plan.files[0].after.as_deref(), Some("one"));
}
