//! Parse facts attached to a plan: whether the before and after text of a
//! changed file parses cleanly, in an enabled language.

use crate::common::{replace, source};
use griz_core::{FileSyntax, PlanSyntax, SyntaxPosition, build_plan};
use std::{collections::BTreeMap, path::PathBuf};

fn syntax_of<'a>(files: &'a BTreeMap<PathBuf, FileSyntax>, path: &str) -> &'a FileSyntax {
    files
        .get(&PathBuf::from(path))
        .unwrap_or_else(|| panic!("no syntax fact for {path}"))
}

#[test]
fn removing_a_closing_brace_introduces_an_error_at_the_right_position() {
    let before = "fn f() {\n    1;\n}\n";
    let plan = build_plan(&[replace("a.rs", "}\n", "")], &source(&[("a.rs", before)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.syntax, PlanSyntax::IntroducedErrors);
    let fact = syntax_of(&plan.file_syntax, "a.rs");
    assert_eq!(fact.language, "rust");
    assert_eq!(fact.before_errors, 0);
    assert_eq!(fact.after_errors, 1);
    assert_eq!(
        fact.first_new_error,
        Some(SyntaxPosition { line: 1, column: 1 })
    );
}

#[test]
fn editing_an_already_broken_file_reports_preexisting_errors() {
    let before = "fn f() {\n    1;\n";
    let plan = build_plan(&[replace("a.rs", "1;", "2;")], &source(&[("a.rs", before)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.syntax, PlanSyntax::PreexistingErrors);
    let fact = syntax_of(&plan.file_syntax, "a.rs");
    assert_eq!(fact.before_errors, 1);
    assert_eq!(fact.after_errors, 1);
    assert_eq!(fact.first_new_error, None);
}

#[test]
fn a_non_enabled_language_is_unknown() {
    let before = "old text\n";
    let plan = build_plan(
        &[replace("notes.txt", "old", "new")],
        &source(&[("notes.txt", before)]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.syntax, PlanSyntax::Unknown);
    assert!(plan.file_syntax.is_empty(), "{:?}", plan.file_syntax);
}

#[test]
fn a_clean_edit_reports_clean() {
    let before = "fn f() {\n    1;\n}\n";
    let plan = build_plan(&[replace("a.rs", "1;", "2;")], &source(&[("a.rs", before)]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(plan.syntax, PlanSyntax::Clean);
    let fact = syntax_of(&plan.file_syntax, "a.rs");
    assert_eq!((fact.before_errors, fact.after_errors), (0, 0));
}
