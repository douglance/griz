//! Bulk anchored edits retain original positions and range mapping.

use crate::common::{anchor, replace, source};
use griz_core::{ByteRange, Occurrence, Op, Rung, build_plan};
use std::{hint::black_box, time::Instant};

fn all(from: &str, to: &str) -> Op {
    let mut op = replace("bulk.txt", from, to);
    if let Op::Replace { occurrence, .. } = &mut op {
        *occurrence = Occurrence::All;
    }
    op
}

#[test]
fn bulk_anchors_keep_original_lines_and_later_ranges() {
    let text = "é old\nold\n尾 keep\nold";
    let ops = [
        all("old", "new\nline"),
        Op::Replace {
            path: "bulk.txt".into(),
            find: Some(anchor("keep")),
            range: Some(ByteRange { start: 15, end: 19 }),
            pattern: None,
            replace: "kept".into(),
            occurrence: Occurrence::Unique,
            target: None,
            expect_hash: None,
        },
    ];
    let plan = build_plan(&ops, &source(&[("bulk.txt", text)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        plan.files[0].after.as_deref(),
        Some("é new\nline\nnew\nline\n尾 kept\nnew\nline")
    );
    let lines: Vec<_> = plan.edits.iter().map(|edit| edit.line).collect();
    assert_eq!(lines, [1, 2, 4, 5]);
    let ids: Vec<_> = plan.edits.iter().map(|edit| edit.id.as_str()).collect();
    assert_eq!(ids, ["e0.1", "e0.2", "e0.3", "e1"]);
}

#[test]
fn bulk_anchors_preserve_surrounding_whitespace_and_deletion() {
    let text = "  old\n    old\nend\n";
    let plan = build_plan(&[all("old", "new")], &source(&[("bulk.txt", text)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        plan.files[0].after.as_deref(),
        Some("  new\n    new\nend\n")
    );
    let plan = build_plan(&[all("old", "")], &source(&[("bulk.txt", text)]));
    assert_eq!(plan.files[0].after.as_deref(), Some("  \n    \nend\n"));
    assert!(plan.edits.iter().all(|edit| edit.rung == Rung::Exact));
}

#[test]
fn bulk_tolerant_anchors_reindent_each_match() {
    let mut op = all("old", "new\n  child");
    if let Op::Replace {
        find: Some(find), ..
    } = &mut op
    {
        find.whole_lines = true;
    }
    let text = "  old\n    old\nend\n";
    let plan = build_plan(&[op], &source(&[("bulk.txt", text)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        plan.files[0].after.as_deref(),
        Some("  new\n    child\n    new\n      child\nend\n")
    );
    assert!(plan.edits.iter().all(|edit| edit.rung == Rung::Indentation));
}

#[test]
fn overlapping_tolerant_matches_are_refused_without_partial_edits() {
    let plan = build_plan(&[all("a\na", "")], &source(&[("bulk.txt", "a \na \na ")]));
    assert!(plan.files.is_empty());
    assert!(plan.edits.is_empty());
    assert_eq!(plan.problems.len(), 1);
    assert_eq!(
        plan.problems[0].kind,
        griz_core::ProblemKind::Ambiguous { lines: vec![1, 2] }
    );
}

#[test]
#[ignore = "manual release-mode bulk anchor timing"]
fn bulk_anchor_measurement() {
    for count in [1_000, 10_000, 30_000] {
        let before = "oldName payload\n".repeat(count);
        let after = "new_name payload\n".repeat(count);
        let source = source(&[("bulk.txt", &before)]);
        let ops = [all("oldName", "new_name")];
        let mut times = Vec::new();
        for _ in 0..3 {
            let start = Instant::now();
            let plan = black_box(build_plan(&ops, &source));
            times.push(start.elapsed().as_micros());
            assert!(plan.problems.is_empty(), "{:?}", plan.problems);
            assert_eq!(plan.edits.len(), count);
            assert_eq!(plan.files[0].after.as_deref(), Some(after.as_str()));
            assert_eq!(plan.edits[count - 1].line, count);
        }
        times.sort_unstable();
        println!("bulk_anchor count={count} median_us={}", times[1]);
    }
}
