//! Range writes preserve UTF-8, ordering, and overlap validation.

use crate::common::{anchor, source};
use griz_core::{ByteRange, Occurrence, Op, ProblemKind, build_plan};
use std::path::PathBuf;

pub(super) fn range_edit(range: ByteRange, expected: Option<&str>, with: &str) -> Op {
    Op::Replace {
        path: PathBuf::from("a.txt"),
        find: expected.map(anchor),
        range: Some(range),
        pattern: None,
        replace: with.to_string(),
        occurrence: Occurrence::Unique,
        target: None,
        expect_hash: None,
    }
}

#[test]
fn growing_shrinking_and_deleting_unicode_ranges_work_in_any_order() {
    let choices = [
        range_edit(ByteRange { start: 0, end: 2 }, Some("α"), "A"),
        range_edit(ByteRange { start: 3, end: 5 }, Some("β"), "longer"),
        range_edit(ByteRange { start: 6, end: 8 }, Some("γ"), ""),
    ];
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [2, 0, 1]] {
        let ops: Vec<_> = order.iter().map(|&i| choices[i].clone()).collect();
        let plan = build_plan(&ops, &source(&[("a.txt", "α β γ\n")]));
        assert!(plan.problems.is_empty(), "{order:?}: {:?}", plan.problems);
        assert_eq!(plan.files[0].before.as_deref(), Some("α β γ\n"));
        assert_eq!(plan.files[0].after.as_deref(), Some("A longer \n"));
    }
}

#[test]
fn invalid_utf8_reversed_and_out_of_bounds_ranges_write_nothing() {
    let ranges = [
        ByteRange { start: 1, end: 2 },
        ByteRange { start: 2, end: 1 },
        ByteRange { start: 0, end: 99 },
    ];
    for range in ranges {
        let plan = build_plan(
            &[with_hash(range_edit(range, None, "x"), "α\n")],
            &source(&[("a.txt", "α\n")]),
        );
        assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
        assert!(plan.files.is_empty());
        assert!(plan.edits.is_empty());
    }
}

#[test]
fn overlapping_original_ranges_do_not_rewrite_prior_edits() {
    let ops = [
        range_edit(ByteRange { start: 0, end: 2 }, Some("aa"), "bb"),
        with_hash(
            range_edit(ByteRange { start: 1, end: 3 }, None, "xx"),
            "aaa",
        ),
    ];
    let plan = build_plan(&ops, &source(&[("a.txt", "aaa")]));
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(plan.problems[0].op, 1);
    assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
    assert_eq!(plan.files[0].after.as_deref(), Some("bba"));
}

pub(super) fn with_hash(mut op: Op, text: &str) -> Op {
    let Op::Replace { expect_hash, .. } = &mut op else {
        panic!("expected a replacement");
    };
    *expect_hash = Some(griz_core::content_hash(text));
    op
}

#[test]
fn unchecked_range_cannot_erase_newer_text() {
    let observed = "one OLD\n";
    let op = range_edit(
        ByteRange {
            start: 0,
            end: observed.len(),
        },
        None,
        "two OLD\n",
    );
    let plan = build_plan(&[op], &source(&[("a.txt", "one NEW\n")]));
    let Some(problem) = plan.problems.first() else {
        panic!("unchecked range was accepted");
    };
    assert!(matches!(&problem.kind, ProblemKind::Invalid { message }
        if message.contains("expect_hash") && message.contains("find")));
    assert!(plan.files.is_empty());
    assert!(plan.edits.is_empty());
}

#[test]
fn range_hash_preconditions_check_the_observed_text() {
    for (observed, accepted) in [("one OLD\n", false), ("one NEW\n", true)] {
        let range = ByteRange {
            start: 0,
            end: observed.len(),
        };
        let op = with_hash(range_edit(range, None, "two NEW\n"), observed);
        let plan = build_plan(&[op], &source(&[("a.txt", "one NEW\n")]));
        if accepted {
            assert!(plan.problems.is_empty());
            assert_eq!(plan.files[0].after.as_deref(), Some("two NEW\n"));
        } else {
            assert!(matches!(plan.problems[0].kind, ProblemKind::Stale { .. }));
            assert!(plan.files.is_empty());
        }
    }
}

#[test]
fn range_old_text_preconditions_check_the_observed_text() {
    for (observed, accepted) in [("one OLD\n", false), ("one NEW\n", true)] {
        let range = ByteRange {
            start: 0,
            end: observed.len(),
        };
        let op = range_edit(range, Some(observed), "two NEW\n");
        let plan = build_plan(&[op], &source(&[("a.txt", "one NEW\n")]));
        if accepted {
            assert!(plan.problems.is_empty());
            assert_eq!(plan.files[0].after.as_deref(), Some("two NEW\n"));
        } else {
            assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
            assert!(plan.files.is_empty());
        }
    }
}

#[test]
fn range_text_guard_preserves_newer_text_outside_the_range() {
    let op = range_edit(ByteRange { start: 0, end: 3 }, Some("one"), "two");
    let plan = build_plan(&[op], &source(&[("a.txt", "one NEW\n")]));
    assert!(plan.problems.is_empty());
    assert_eq!(plan.files[0].after.as_deref(), Some("two NEW\n"));
}
