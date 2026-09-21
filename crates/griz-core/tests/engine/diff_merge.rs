//! Diff rendering and three-way merge.

use crate::common::{replace, source};
use griz_core::{MergeOutcome, build_plan, render_diff, three_way};

#[test]
fn diff_counts_lines_and_uses_git_headers() {
    let plan = build_plan(
        &[replace("a.rs", "b", "B\nC")],
        &source(&[("a.rs", "a\nb\nc\n")]),
    );
    let diffs = render_diff(&plan.files, std::path::Path::new("/"));
    assert_eq!((diffs[0].added, diffs[0].removed), (2, 1));
    assert!(
        diffs[0].text.starts_with("--- a/a.rs\n+++ b/a.rs\n"),
        "{}",
        diffs[0].text
    );
    assert!(diffs[0].text.contains("-b\n+B\n+C\n"), "{}", diffs[0].text);
}

#[test]
fn non_overlapping_changes_merge_cleanly() {
    let merged = three_way("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n");
    assert_eq!(
        merged,
        MergeOutcome::Clean {
            text: "A\nb\nC\n".to_string()
        }
    );
}

#[test]
fn overlapping_changes_conflict() {
    assert_eq!(three_way("a\n", "b\n", "c\n"), MergeOutcome::Conflict);
}
