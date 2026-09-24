//! Compares range translation with sequential replay across mixed splices.

use crate::{
    common::{anchor, replace, source},
    range_edits::{range_edit, with_hash},
};
use griz_core::{ByteRange, Op, ProblemKind, build_plan};
use std::path::PathBuf;

type Step = (usize, usize, &'static str);
const ORIGINAL: &str = "0123456789";

fn anchored_splice(text: &str, range: ByteRange, with: &str) -> Op {
    if range.start != range.end {
        return replace("a.txt", &text[range.start..range.end], with);
    }
    let after = range.start == text.len();
    let at = range.start - usize::from(after);
    Op::Insert {
        path: PathBuf::from("a.txt"),
        anchor: anchor(&text[at..=at]),
        after,
        text: with.to_string(),
        expect_hash: None,
    }
}

fn replay(mut range: ByteRange, splices: &[Step]) -> Option<ByteRange> {
    for &(start, end, with) in splices {
        if range.end <= start {
            continue;
        }
        if range.start < end {
            return None;
        }
        range.start = range.start - (end - start) + with.len();
        range.end = range.end - (end - start) + with.len();
    }
    Some(range)
}

fn check_range(prefix: &[Op], text: &str, steps: &[Step], range: ByteRange) {
    let mut ops = prefix.to_vec();
    ops.push(with_hash(range_edit(range, None, "!"), ORIGINAL));
    let plan = build_plan(&ops, &source(&[("a.txt", ORIGINAL)]));
    let mut expected = text.to_string();
    if let Some(mapped) = replay(range, steps) {
        expected.replace_range(mapped.start..mapped.end, "!");
        assert!(
            plan.problems.is_empty(),
            "{steps:?} {range:?}: {:?}",
            plan.problems
        );
    } else {
        assert_eq!(plan.problems.len(), 1, "{steps:?} {range:?}");
        assert_eq!(plan.problems[0].op, prefix.len());
        assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
    }
    assert_eq!(
        plan.files[0].after.as_deref(),
        Some(expected.as_str()),
        "{steps:?} {range:?}"
    );
}

fn check_sequence(steps: &[Step]) {
    let mut text = ORIGINAL.to_string();
    let mut ops = Vec::new();
    for &(start, end, with) in steps {
        let range = ByteRange { start, end };
        ops.push(anchored_splice(&text, range, with));
        text.replace_range(start..end, with);
    }
    for start in 0..=ORIGINAL.len() {
        for end in start..=ORIGINAL.len() {
            check_range(&ops, &text, steps, ByteRange { start, end });
        }
    }
}

#[test]
fn mixed_splices_match_sequential_replay_for_every_original_range() {
    let cases = [
        [(1, 3, "ABC"), (8, 8, "Q")],
        [(8, 10, "XYZ"), (1, 3, "")],
        [(3, 5, ""), (1, 5, "AB")],
        [(3, 3, "ABC"), (3, 4, "")],
        [(3, 3, "ABC"), (4, 5, "Q")],
        [(3, 3, "ABC"), (5, 8, "Q")],
        [(3, 5, "ABC"), (6, 6, "Q")],
        [(3, 5, ""), (3, 3, "Q")],
        [(0, 0, "ABC"), (10, 13, "")],
        [(10, 10, "ABC"), (0, 1, "Q")],
        [(1, 3, ""), (0, 0, "ABC")],
        [(1, 3, "ABC"), (1, 1, "Q")],
    ];
    for steps in cases {
        check_sequence(&steps);
    }
}

#[test]
fn repeated_insertions_at_one_boundary_keep_point_affinity() {
    check_sequence(&[(3, 3, "ABC"), (3, 3, "Q"), (4, 4, "Z")]);
    check_sequence(&[(3, 5, ""), (3, 3, "ABC"), (6, 6, "Q")]);
}

#[test]
fn an_extreme_range_after_earlier_splices_is_rejected_without_panicking() {
    let ops = [
        replace("a.txt", "12", "ABC"),
        replace("a.txt", "89", "XYZ"),
        with_hash(
            range_edit(
                ByteRange {
                    start: 5,
                    end: usize::MAX,
                },
                None,
                "!",
            ),
            ORIGINAL,
        ),
    ];
    let plan = build_plan(&ops, &source(&[("a.txt", ORIGINAL)]));
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(plan.problems[0].op, 2);
    assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
    assert_eq!(plan.files[0].after.as_deref(), Some("0ABC34567XYZ"));
}
