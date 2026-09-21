//! Planning several operations against one overlay.

use crate::common::{anchor, replace, source, source_with_absent};
use griz_core::{
    ByteRange, ChangeKind, Occurrence, Op, ProblemKind, Rung, build_plan, content_hash,
};
use std::path::PathBuf;

fn after_of<'a>(plan: &'a griz_core::Plan, path: &str) -> Option<&'a str> {
    plan.files
        .iter()
        .find(|file| file.path.as_path() == std::path::Path::new(path))
        .and_then(|file| file.after.as_deref())
}

#[test]
fn later_operations_see_earlier_ones_in_the_same_file() {
    let ops = [
        replace("a.rs", "one", "two"),
        replace("a.rs", "two", "three"),
    ];
    let plan = build_plan(&ops, &source(&[("a.rs", "one\n")]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(after_of(&plan, "a.rs"), Some("three\n"));
    assert_eq!(plan.files.len(), 1);
}

#[test]
fn find_ranges_stay_valid_after_earlier_edits_change_lengths() {
    let text = "aa bb cc\n";
    let ranges = [
        ByteRange { start: 0, end: 2 },
        ByteRange { start: 6, end: 8 },
    ];
    let ops: Vec<_> = ranges
        .iter()
        .map(|range| Op::Replace {
            path: PathBuf::from("a.rs"),
            find: Some(anchor(&text[range.start..range.end])),
            range: Some(*range),
            pattern: None,
            replace: "longer".to_string(),
            occurrence: Occurrence::Unique,
            target: None,
            expect_hash: Some(content_hash(text)),
        })
        .collect();
    let plan = build_plan(&ops, &source(&[("a.rs", text)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert!(plan.edits.iter().all(|edit| edit.rung == Rung::Range));
    assert_eq!(after_of(&plan, "a.rs"), Some("longer bb longer\n"));
}

#[test]
fn a_range_that_no_longer_holds_its_text_is_refused() {
    let op = Op::Replace {
        path: PathBuf::from("a.rs"),
        find: Some(anchor("zz")),
        range: Some(ByteRange { start: 0, end: 2 }),
        pattern: None,
        replace: "x".to_string(),
        occurrence: Occurrence::Unique,
        target: None,
        expect_hash: None,
    };
    let plan = build_plan(&[op], &source(&[("a.rs", "aa\n")]));
    assert_eq!(plan.problems[0].kind, ProblemKind::BadRange);
}

#[test]
fn a_stale_fingerprint_is_refused_with_both_hashes() {
    let op = Op::Delete {
        path: PathBuf::from("a.rs"),
        expect_hash: Some(content_hash("old\n")),
    };
    let plan = build_plan(&[op], &source(&[("a.rs", "new\n")]));
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Stale {
            expected: content_hash("old\n"),
            actual: Some(content_hash("new\n")),
        }
    );
    assert!(plan.files.is_empty());
}

#[test]
fn create_delete_and_move_track_file_existence() {
    let ops = [
        Op::Create {
            path: PathBuf::from("new.rs"),
            text: "n\n".to_string(),
            overwrite: false,
        },
        Op::Delete {
            path: PathBuf::from("gone.rs"),
            expect_hash: None,
        },
        Op::Move {
            path: PathBuf::from("old.rs"),
            to: PathBuf::from("moved.rs"),
            expect_hash: None,
        },
    ];
    let files = [("gone.rs", "g\n"), ("old.rs", "o\n")];
    let plan = build_plan(&ops, &source_with_absent(&files, &["new.rs", "moved.rs"]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let kind = |path: &str| {
        plan.files
            .iter()
            .find(|f| f.path.as_path() == std::path::Path::new(path))
            .map(|f| f.kind)
    };
    assert_eq!(kind("new.rs"), Some(ChangeKind::Create));
    assert_eq!(kind("gone.rs"), Some(ChangeKind::Delete));
    assert_eq!(kind("old.rs"), Some(ChangeKind::Delete));
    assert_eq!(kind("moved.rs"), Some(ChangeKind::Create));
    assert_eq!(after_of(&plan, "moved.rs"), Some("o\n"));
    let ids: Vec<_> = plan.edits.iter().map(|edit| edit.id.as_str()).collect();
    assert_eq!(ids, ["e0", "e1", "e2.from", "e2.to"]);
}

#[test]
fn existing_targets_and_missing_sources_are_problems() {
    let ops = [
        Op::Create {
            path: PathBuf::from("a.rs"),
            text: String::new(),
            overwrite: false,
        },
        Op::Delete {
            path: PathBuf::from("none.rs"),
            expect_hash: None,
        },
    ];
    let plan = build_plan(&ops, &source_with_absent(&[("a.rs", "a\n")], &["none.rs"]));
    let kinds: Vec<_> = plan.problems.iter().map(|p| p.kind.clone()).collect();
    assert_eq!(kinds, [ProblemKind::Exists, ProblemKind::NotFound]);
}

#[test]
fn every_problem_is_reported_not_just_the_first() {
    let ops = [
        replace("a.rs", "missing one", "x"),
        replace("a.rs", "present", "kept"),
        replace("a.rs", "missing two", "y"),
    ];
    let plan = build_plan(&ops, &source(&[("a.rs", "present\n")]));
    let failed: Vec<_> = plan.problems.iter().map(|p| p.op).collect();
    assert_eq!(failed, [0, 2]);
    assert_eq!(plan.edits.len(), 1);
}

#[test]
fn occurrence_all_and_nth_select_matches() {
    let all = Op::Replace {
        path: PathBuf::from("a.rs"),
        find: Some(anchor("x")),
        range: None,
        pattern: None,
        replace: "y".to_string(),
        occurrence: Occurrence::All,
        target: None,
        expect_hash: None,
    };
    let plan = build_plan(&[all], &source(&[("a.rs", "x x x\n")]));
    assert_eq!(after_of(&plan, "a.rs"), Some("y y y\n"));
    let ids: Vec<_> = plan.edits.iter().map(|edit| edit.id.as_str()).collect();
    assert_eq!(ids, ["e0.1", "e0.2", "e0.3"]);

    let nth = Op::Replace {
        path: PathBuf::from("a.rs"),
        find: Some(anchor("x")),
        range: None,
        pattern: None,
        replace: "y".to_string(),
        occurrence: Occurrence::Nth(2),
        target: None,
        expect_hash: None,
    };
    let plan = build_plan(&[nth], &source(&[("a.rs", "x x x\n")]));
    assert_eq!(after_of(&plan, "a.rs"), Some("x y x\n"));
}

#[test]
fn an_empty_anchor_is_invalid() {
    let plan = build_plan(&[replace("a.rs", "", "x")], &source(&[("a.rs", "a\n")]));
    assert!(matches!(plan.problems[0].kind, ProblemKind::Invalid { .. }));
}

#[test]
fn a_plain_string_is_shorthand_for_an_anchor() -> Result<(), serde_json::Error> {
    let op: Op = serde_json::from_value(serde_json::json!({
        "op": "replace", "path": "a.rs", "find": "one", "replace": "two"
    }))?;
    let plan = build_plan(&[op], &source(&[("a.rs", "one\n")]));
    assert_eq!(after_of(&plan, "a.rs"), Some("two\n"));
    Ok(())
}
