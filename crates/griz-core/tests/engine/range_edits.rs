//! Range writes preserve UTF-8, ordering, and overlap validation.

use crate::common::{anchor, source};
use griz_core::{ByteRange, Occurrence, Op, ProblemKind, build_plan};
use std::path::PathBuf;

fn range_edit(range: ByteRange, expected: Option<&str>, with: &str) -> Op {
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
            &[range_edit(range, None, "x")],
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
        range_edit(ByteRange { start: 1, end: 3 }, None, "xx"),
    ];
    let plan = build_plan(&ops, &source(&[("a.txt", "aaa")]));
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(plan.problems[0].op, 1);
    assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
    assert_eq!(plan.files[0].after.as_deref(), Some("bba"));
}
