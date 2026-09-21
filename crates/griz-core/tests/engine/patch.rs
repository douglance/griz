//! Codex patch text becomes ordinary operations.

use crate::common::{source, source_with_absent};
use griz_core::{ChangeKind, Op, build_plan, parse_patch};
use std::{
    error::Error,
    path::{Path, PathBuf},
};

type TestResult = Result<(), Box<dyn Error>>;

fn resolve(path: &str) -> PathBuf {
    PathBuf::from(path)
}

fn only_after(plan: &griz_core::Plan) -> Option<&str> {
    plan.files.first().and_then(|file| file.after.as_deref())
}

#[test]
fn several_patch_documents_in_one_text_are_one_plan() -> TestResult {
    // Programs concatenate patch documents; each one is its own
    // `*** Begin Patch` … `*** End Patch` pair.
    let text = concat!(
        "*** Begin Patch\n*** Add File: a.rs\n+fn a() {}\n*** End Patch\n",
        "*** Begin Patch\n*** Add File: b.rs\n+fn b() {}\n*** End Patch\n"
    );
    let ops = parse_patch(text, &resolve)?;
    assert_eq!(ops.len(), 2);
    Ok(())
}

#[test]
fn text_after_a_patch_document_is_an_error() {
    let text = "*** Begin Patch\n*** Add File: a.rs\n+fn a() {}\n*** End Patch\nstray\n";
    let Err(error) = parse_patch(text, &resolve) else {
        panic!("stray text after a document must be an error");
    };
    assert!(format!("{error}").contains("End Patch"), "{error}");
}

#[test]
fn update_hunk_replaces_context_and_removed_lines() -> TestResult {
    let patch = "*** Begin Patch\n*** Update File: a.rs\n@@\n fn f() {\n-    old();\n+    new();\n }\n*** End Patch\n";
    let ops = parse_patch(patch, &resolve)?;
    let plan = build_plan(&ops, &source(&[("a.rs", "fn f() {\n    old();\n}\n")]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(only_after(&plan), Some("fn f() {\n    new();\n}\n"));
    Ok(())
}

#[test]
fn two_blocks_for_one_file_merge_instead_of_failing() -> TestResult {
    let patch = "*** Begin Patch\n*** Update File: a.rs\n-a\n+A\n*** Update File: a.rs\n-c\n+C\n*** End Patch\n";
    let ops = parse_patch(patch, &resolve)?;
    let plan = build_plan(&ops, &source(&[("a.rs", "a\nb\nc\n")]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(only_after(&plan), Some("A\nb\nC\n"));
    assert_eq!(plan.files.len(), 1);
    Ok(())
}

#[test]
fn add_delete_and_move_blocks_become_file_operations() -> TestResult {
    let patch = "*** Begin Patch\n*** Add File: n.rs\n+new\n*** Delete File: d.rs\n*** Update File: m.rs\n*** Move to: moved.rs\n-x\n+y\n*** End Patch\n";
    let ops = parse_patch(patch, &resolve)?;
    let kinds: Vec<_> = ops
        .iter()
        .map(|op| match op {
            Op::Create { .. } => "create",
            Op::Delete { .. } => "delete",
            Op::Replace { .. } => "replace",
            Op::Move { .. } => "move",
            Op::Insert { .. } => "insert",
        })
        .collect();
    assert_eq!(kinds, ["create", "delete", "replace", "move"]);
    let files = [("d.rs", "d\n"), ("m.rs", "x\n")];
    let plan = build_plan(&ops, &source_with_absent(&files, &["n.rs", "moved.rs"]));
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let file = |path: &str| {
        plan.files
            .iter()
            .find(|f| f.path.as_path() == Path::new(path))
    };
    assert_eq!(file("moved.rs").map(|f| f.kind), Some(ChangeKind::Create));
    assert_eq!(
        file("moved.rs").and_then(|f| f.after.as_deref()),
        Some("y\n")
    );
    assert_eq!(file("n.rs").and_then(|f| f.after.as_deref()), Some("new\n"));
    Ok(())
}

#[test]
fn additions_without_context_append_to_the_file() -> TestResult {
    let patch = "*** Begin Patch\n*** Update File: a.rs\n+tail\n*** End Patch\n";
    let ops = parse_patch(patch, &resolve)?;
    let plan = build_plan(&ops, &source(&[("a.rs", "head")]));
    assert_eq!(only_after(&plan), Some("head\ntail\n"));
    Ok(())
}

#[test]
fn additions_after_a_header_land_below_that_line() -> TestResult {
    let patch =
        "*** Begin Patch\n*** Update File: a.rs\n@@ fn second() {\n+    added();\n*** End Patch\n";
    let ops = parse_patch(patch, &resolve)?;
    let plan = build_plan(
        &ops,
        &source(&[("a.rs", "fn first() {\n}\nfn second() {\n}\n")]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        only_after(&plan),
        Some("fn first() {\n}\nfn second() {\n    added();\n}\n")
    );
    Ok(())
}

#[test]
fn a_header_disambiguates_repeated_context() -> TestResult {
    let patch = "*** Begin Patch\n*** Update File: a.rs\n@@ fn second() {\n-    x();\n+    y();\n*** End Patch\n";
    let ops = parse_patch(patch, &resolve)?;
    let plan = build_plan(
        &ops,
        &source(&[(
            "a.rs",
            "fn first() {\n    x();\n}\nfn second() {\n    x();\n}\n",
        )]),
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    assert_eq!(
        only_after(&plan),
        Some("fn first() {\n    x();\n}\nfn second() {\n    y();\n}\n")
    );
    Ok(())
}

#[test]
fn malformed_patches_name_the_offending_line() {
    let no_begin = parse_patch("*** Update File: a\n", &resolve);
    assert_eq!(no_begin.map_err(|e| e.line), Err(1));
    let bad_hunk = parse_patch(
        "*** Begin Patch\n*** Update File: a\n?oops\n*** End Patch\n",
        &resolve,
    );
    assert_eq!(bad_hunk.map_err(|e| e.line), Err(3));
    let no_end = parse_patch("*** Begin Patch\n*** Delete File: a\n", &resolve);
    assert!(no_end.is_err());
}
