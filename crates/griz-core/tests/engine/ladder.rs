//! The matching ladder and the confidence it reports.

use crate::common::{replace, source};
use griz_core::{Confidence, ProblemKind, Rung, build_plan};
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

fn after(plan: &griz_core::Plan) -> Result<&str, Box<dyn Error>> {
    plan.files
        .first()
        .and_then(|file| file.after.as_deref())
        .ok_or_else(|| "plan changed no file".into())
}

#[test]
fn exact_match_is_machine_confidence() -> TestResult {
    let plan = build_plan(
        &[replace("a.rs", "let x = 1;", "let x = 2;")],
        &source(&[("a.rs", "fn f() {\n    let x = 1;\n}\n")]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.edits[0].rung, Rung::Exact);
    assert_eq!(plan.edits[0].confidence, Confidence::Machine);
    assert_eq!(plan.edits[0].line, 2);
    assert_eq!(after(&plan)?, "fn f() {\n    let x = 2;\n}\n");
    Ok(())
}

#[test]
fn trailing_whitespace_match_is_only_maybe() -> TestResult {
    let file = "a();   \nb();\n";
    let plan = build_plan(
        &[replace("a.rs", "a();\nb();", "c();")],
        &source(&[("a.rs", file)]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.edits[0].rung, Rung::TrailingWhitespace);
    assert_eq!(plan.edits[0].confidence, Confidence::Maybe);
    assert_eq!(plan.confidence(), Confidence::Maybe);
    assert_eq!(after(&plan)?, "c();\n");
    Ok(())
}

#[test]
fn trimmed_match_steps_below_trailing_whitespace() -> TestResult {
    let file = "  x = 1\n  y = 2\n";
    let plan = build_plan(
        &[replace("a.py", "x = 1\n    y = 2", "z = 3")],
        &source(&[("a.py", file)]),
    );
    assert_eq!(plan.edits[0].rung, Rung::Trimmed);
    assert_eq!(after(&plan)?, "  z = 3\n");
    Ok(())
}

#[test]
fn indentation_match_reindents_the_replacement() -> TestResult {
    let file = "fn f() {\n        if a {\n            b();\n        }\n}\n";
    let find = "if a {\n    b();\n}";
    let with = "if a {\n    c();\n}";
    let plan = build_plan(&[replace("a.rs", find, with)], &source(&[("a.rs", file)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.edits[0].rung, Rung::Indentation);
    assert_eq!(plan.edits[0].confidence, Confidence::Maybe);
    assert_eq!(
        after(&plan)?,
        "fn f() {\n        if a {\n            c();\n        }\n}\n"
    );
    Ok(())
}

#[test]
fn ambiguous_anchor_lists_every_candidate_instead_of_taking_the_first() {
    let file = "x();\ny();\nx();\n";
    let plan = build_plan(
        &[replace("a.rs", "x();", "z();")],
        &source(&[("a.rs", file)]),
    );
    assert!(plan.files.is_empty());
    assert_eq!(
        plan.problems[0].kind,
        ProblemKind::Ambiguous { lines: vec![1, 3] }
    );
}

#[test]
fn missing_anchor_reports_the_nearest_real_text() -> TestResult {
    let file = "fn alpha() {}\nfn beta(x: u32) {}\nfn gamma() {}\n";
    let plan = build_plan(
        &[replace("a.rs", "fn beta(x: u64) {}", "")],
        &source(&[("a.rs", file)]),
    );
    let ProblemKind::Missing {
        nearest: Some(window),
    } = &plan.problems[0].kind
    else {
        return Err(format!("expected a nearest window, got {:?}", plan.problems).into());
    };
    assert_eq!(window.line, 2);
    assert_eq!(window.text, "fn beta(x: u32) {}");
    Ok(())
}

#[test]
fn whole_line_anchor_ignores_partial_line_matches() {
    let mut op = replace("a.rs", "b", "c");
    if let griz_core::Op::Replace {
        find: Some(anchor), ..
    } = &mut op
    {
        anchor.whole_lines = true;
    }
    let plan = build_plan(&[op], &source(&[("a.rs", "abc\nb\n")]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.edits[0].line, 2);
}

#[test]
fn after_hint_narrows_the_search() -> TestResult {
    let mut op = replace("a.rs", "x();", "z();");
    if let griz_core::Op::Replace {
        find: Some(anchor), ..
    } = &mut op
    {
        anchor.after = Some("fn second".to_string());
    }
    let plan = build_plan(
        &[op],
        &source(&[("a.rs", "fn first() { x(); }\nfn second() { x(); }\n")]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(after(&plan)?, "fn first() { x(); }\nfn second() { z(); }\n");
    Ok(())
}
