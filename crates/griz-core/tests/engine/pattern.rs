//! Replacing anchored by structural pattern instead of text.

use crate::common::source;
use griz_core::{Occurrence, Op, PatternLocator, ProblemKind, Rung, build_plan};
use std::path::PathBuf;

fn locator(pattern: &str) -> PatternLocator {
    PatternLocator {
        pattern: pattern.to_string(),
        language: None,
    }
}

fn pattern_op(path: &str, pattern: &str, replace: &str, target: Option<&str>) -> Op {
    Op::Replace {
        path: PathBuf::from(path),
        find: None,
        range: None,
        pattern: Some(locator(pattern)),
        replace: replace.to_string(),
        occurrence: Occurrence::Unique,
        target: target.map(str::to_string),
        expect_hash: None,
    }
}

fn after_of<'a>(plan: &'a griz_core::Plan, path: &str) -> Option<&'a str> {
    plan.files
        .iter()
        .find(|file| file.path.as_path() == std::path::Path::new(path))
        .and_then(|file| file.after.as_deref())
}

#[test]
fn a_pattern_match_edits_the_whole_call_and_is_never_a_tolerant_rung() {
    let text = "fn f() {\n    foo(1, 2);\n    bar(3);\n}\n";
    let plan = build_plan(
        &[pattern_op("a.rs", "foo($A, $B)", "baz()", None)],
        &source(&[("a.rs", text)]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        after_of(&plan, "a.rs"),
        Some("fn f() {\n    baz();\n    bar(3);\n}\n")
    );
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].rung, Rung::Pattern);
    assert!(
        plan.edits
            .iter()
            .all(|edit| edit.rung != Rung::TrailingWhitespace
                && edit.rung != Rung::Trimmed
                && edit.rung != Rung::Indentation)
    );
}

#[test]
fn an_ambiguous_pattern_lists_every_candidate_line() {
    let text = "foo(1);\nfoo(2);\n";
    let plan = build_plan(
        &[pattern_op("a.rs", "foo($A)", "bar()", None)],
        &source(&[("a.rs", text)]),
    );
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Ambiguous { lines: vec![1, 2] },
        "{:?}",
        plan.problems
    );
}

#[test]
fn target_edits_only_the_captured_metavariable() {
    let text = "fn f() {\n    foo(old_name, 2);\n}\n";
    let plan = build_plan(
        &[pattern_op("a.rs", "foo($A, $B)", "new_name", Some("$A"))],
        &source(&[("a.rs", text)]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        after_of(&plan, "a.rs"),
        Some("fn f() {\n    foo(new_name, 2);\n}\n")
    );
}

#[test]
fn var_ranges_in_a_find_result_point_at_the_captured_text() -> Result<(), Box<dyn std::error::Error>>
{
    use griz_core::{FindQuery, find};
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("a.rs"),
        "fn f() {\n    foo(old_name, 2);\n}\n",
    )?;
    let page = find(&FindQuery {
        paths: vec![dir.path().to_path_buf()],
        pattern: Some("foo($A, $B)".to_string()),
        limit: 10,
        ..FindQuery::default()
    })?;
    let m = &page.matches[0];
    let range = m.var_ranges.get("A").ok_or("no range for $A")?;
    let text = std::fs::read_to_string(dir.path().join("a.rs"))?;
    assert_eq!(&text[range.start..range.end], "old_name");
    Ok(())
}

#[test]
fn giving_both_pattern_and_find_is_invalid() {
    let op = Op::Replace {
        path: PathBuf::from("a.rs"),
        find: Some(griz_core::Anchor {
            text: "foo".to_string(),
            after: None,
            whole_lines: false,
        }),
        range: None,
        pattern: Some(locator("foo($A)")),
        replace: "bar".to_string(),
        occurrence: Occurrence::Unique,
        target: None,
        expect_hash: None,
    };
    let plan = build_plan(&[op], &source(&[("a.rs", "foo(1)\n")]));
    assert!(matches!(plan.problems[0].kind, ProblemKind::Invalid { .. }));
}
