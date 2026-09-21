//! The find, plan, apply, undo loop.

use crate::common::{Griz, TestResult};
use serde_json::json;

#[test]
fn find_results_become_exact_range_edits_and_undo_restores_bytes() -> TestResult {
    let griz = Griz::new()?;
    griz.write("src/a.rs", "let oldName = 1;\nuse_it(oldName);\n")?;
    griz.write("src/b.rs", "oldName();\n")?;
    let found = griz.run(&["find", "--literal", "oldName", "--expect-matches", "3"])?;
    assert_eq!(found.json["outcome"], "passed", "{}", found.json);
    let ops: Vec<_> = found.json["matches"]
        .as_array()
        .ok_or("no matches")?
        .iter()
        .map(|m| json!({ "op": "replace", "path": m["path"], "range": m["range"], "find": { "text": m["text"] }, "replace": "new_name", "expect_hash": m["file_hash"] }))
        .collect();
    let ops = serde_json::to_string(&ops)?;
    let plan = griz.id(&["plan", "--ops", &ops, "--expect-edits", "3"], "plan")?;
    let op = griz.id(&["apply", &plan, "--expect-files", "2"], "apply")?;
    assert_eq!(
        griz.read("src/a.rs")?,
        "let new_name = 1;\nuse_it(new_name);\n"
    );
    assert_eq!(griz.read("src/b.rs")?, "new_name();\n");
    griz.id(&["undo", &op], "undo")?;
    assert_eq!(
        griz.read("src/a.rs")?,
        "let oldName = 1;\nuse_it(oldName);\n"
    );
    assert_eq!(griz.read("src/b.rs")?, "oldName();\n");
    Ok(())
}

#[test]
fn patch_files_plan_like_operations() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    old();\n}\n")?;
    griz.write("change.patch", "*** Begin Patch\n*** Update File: a.rs\n@@\n fn f() {\n-    old();\n+    new();\n }\n*** End Patch\n")?;
    let plan = griz.id(&["plan", "--patch", "@change.patch"], "plan")?;
    let diff = griz.run(&["diff", &plan, "--grep", "^[-+] "])?;
    let lines: Vec<_> = diff.json["lines"]
        .as_array()
        .ok_or("no lines")?
        .iter()
        .map(|l| l["text"].clone())
        .collect();
    assert_eq!(lines, [json!("-    old();"), json!("+    new();")]);
    assert_eq!(
        diff.json["files"][0]["path"]
            .as_str()
            .map(|p| p.ends_with("a.rs")),
        Some(true)
    );
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("a.rs")?, "fn f() {\n    new();\n}\n");
    Ok(())
}

#[test]
fn a_structural_pattern_matches_across_formatting() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    foo(1, bar, baz);\n}\n")?;
    let found = griz.run(&["find", "--pattern", "foo($A, $$$REST)"])?;
    assert_eq!(found.json["total"], 1, "{}", found.json);
    assert_eq!(found.json["matches"][0]["vars"]["A"], "1");
    assert_eq!(found.json["matches"][0]["vars"]["REST"], "bar, baz");
    Ok(())
}

#[test]
fn absorb_folds_a_formatter_run_so_undo_restores_the_pre_apply_text() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn  f() {}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"f()"},"replace":"g()"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let op = griz.id(&["apply", &plan], "apply")?;
    griz.write("a.rs", "fn g() {}\n")?;
    let absorbed = griz.run(&[
        "absorb",
        &op,
        "--purpose",
        "t",
        "--idempotency-key",
        "absorb",
    ])?;
    assert_eq!(absorbed.json["outcome"], "passed", "{}", absorbed.json);
    griz.id(&["undo", &op], "undo")?;
    assert_eq!(griz.read("a.rs")?, "fn  f() {}\n");
    Ok(())
}

#[test]
fn absorb_errors_on_a_path_the_operation_never_wrote() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("untouched.rs", "u\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let op = griz.id(&["apply", &plan], "apply")?;
    let run = griz.run(&[
        "absorb",
        &op,
        "--paths",
        "untouched.rs",
        "--purpose",
        "t",
        "--idempotency-key",
        "absorb",
    ])?;
    assert_eq!(run.json["code"], "VALIDATION_ERROR", "{}", run.json);
    Ok(())
}

#[test]
fn select_applies_only_the_chosen_files() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("b.rs", "b\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"},{"op":"replace","path":"b.rs","find":{"text":"b"},"replace":"B"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let only_b = griz.id(&["select", &plan, "--paths", "b.rs"], "select")?;
    griz.id(&["apply", &only_b], "apply")?;
    assert_eq!(
        (griz.read("a.rs")?, griz.read("b.rs")?),
        ("a\n".into(), "B\n".into())
    );
    Ok(())
}

#[test]
fn a_structural_pattern_replaces_only_the_matched_capture() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {\n    foo(old_name, 2);\n}\n")?;
    let ops = serde_json::to_string(&json!([{
        "op": "replace",
        "path": "a.rs",
        "pattern": { "pattern": "foo($A, $B)" },
        "target": "$A",
        "replace": "new_name",
    }]))?;
    let plan = griz.id(
        &["plan", "--ops", &ops, "--expect-edits", "1"],
        "pattern-plan",
    )?;
    griz.id(&["apply", &plan], "pattern-apply")?;
    assert_eq!(griz.read("a.rs")?, "fn f() {\n    foo(new_name, 2);\n}\n");
    Ok(())
}
