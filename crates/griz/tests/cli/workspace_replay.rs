//! `WorkspaceEdit` retries must identify the request before reading live files.

use crate::common::{Griz, TestResult};
use serde_json::{Value, json};
fn text_edit(griz: &Griz, replacement: &str) -> Value {
    let uri = format!("file://{}", griz.path("a.rs").display());
    json!({"changes": {uri: [{
        "range": {
            "start": {"line": 0, "character": 6},
            "end": {"line": 0, "character": 9}
        },
        "newText": replacement
    }]}})
}

#[test]
fn workspace_edit_replays_from_a_file_after_target_disappears() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    let edit = text_edit(&griz, "NEW");
    let plan = griz.id(&["plan", "--workspace-edit", &edit.to_string()], "same")?;
    std::fs::remove_file(griz.path("a.rs"))?;
    griz.write("edit.json", &serde_json::to_string_pretty(&edit)?)?;
    let again = griz.run(&[
        "plan",
        "--workspace-edit",
        "@edit.json",
        "--position-encoding",
        "utf-16",
        "--verbosity",
        "info",
        "--purpose",
        "retry",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(again.code, Some(0), "{}", again.json);
    assert_eq!(again.json["id"], plan);
    assert_eq!(again.json["outcome"], "passed");
    assert_eq!(again.json["replayed"], true);
    assert_eq!(again.json["summary"]["files"], 1);
    assert!(!griz.path("a.rs").exists());
    Ok(())
}

#[test]
fn workspace_edit_changed_request_conflicts_before_reading_target() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    let original = text_edit(&griz, "NEW").to_string();
    let changed = text_edit(&griz, "OTHER").to_string();
    griz.id(&["plan", "--workspace-edit", &original], "same")?;
    std::fs::remove_file(griz.path("a.rs"))?;
    for options in [
        vec!["--workspace-edit", &changed],
        vec![
            "--workspace-edit",
            &original,
            "--position-encoding",
            "utf-8",
        ],
        vec!["--workspace-edit", &original, "--expect-edits", "2"],
        vec!["--workspace-edit", &original, "--expect-syntax", "clean"],
    ] {
        let run = griz.run(
            &[
                &["plan"][..],
                &options,
                &["--purpose", "changed", "--idempotency-key", "same"],
            ]
            .concat(),
        )?;
        assert_eq!(run.code, Some(1));
        assert_eq!(run.json["code"], "IDEMPOTENCY_CONFLICT", "{}", run.json);
    }
    Ok(())
}

#[test]
fn workspace_edit_changed_input_file_is_not_a_replay() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    griz.write("edit.json", &text_edit(&griz, "NEW").to_string())?;
    griz.id(&["plan", "--workspace-edit", "@edit.json"], "same")?;
    griz.write("edit.json", &text_edit(&griz, "OTHER").to_string())?;
    let run = griz.run(&[
        "plan",
        "--workspace-edit",
        "@edit.json",
        "--purpose",
        "changed",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(run.code, Some(1));
    assert_eq!(run.json["code"], "IDEMPOTENCY_CONFLICT", "{}", run.json);
    Ok(())
}

#[test]
fn workspace_edit_validation_failure_releases_its_claim() -> TestResult {
    let griz = Griz::new()?;
    let edit = text_edit(&griz, "NEW").to_string();
    let args = [
        "plan",
        "--workspace-edit",
        &edit,
        "--purpose",
        "test",
        "--idempotency-key",
        "same",
    ];
    let missing = griz.run(&args)?;
    assert_eq!(missing.code, Some(1));
    assert_eq!(missing.json["code"], "VALIDATION_ERROR");
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    let corrected = griz.run(&args)?;
    assert_eq!(corrected.code, Some(0), "{}", corrected.json);
    assert_eq!(corrected.json["outcome"], "passed");
    assert_ne!(corrected.json["replayed"], true);
    assert_eq!(griz.read("a.rs")?, "const OLD: u8 = 1;\n");
    Ok(())
}

#[test]
fn workspace_edit_failed_expectation_keeps_its_verdict_on_retry() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    let edit = text_edit(&griz, "NEW").to_string();
    let args = [
        "plan",
        "--workspace-edit",
        &edit,
        "--expect-edits",
        "2",
        "--purpose",
        "test",
        "--idempotency-key",
        "same",
    ];
    let first = griz.run(&args)?;
    assert_eq!(first.code, Some(1));
    assert_eq!(first.json["outcome"], "failed");
    griz.write("a.rs", "")?;
    let again = griz.run(&args)?;
    assert_eq!(again.code, Some(1));
    assert_eq!(
        again.json,
        json!({
            "id": first.json["id"], "outcome": "failed", "replayed": true
        })
    );
    Ok(())
}

fn check_retry(griz: &Griz, edit: &Value) -> TestResult {
    let edit = edit.to_string();
    let args = ["plan", "--workspace-edit", &edit];
    let plan = griz.id(&args, "original")?;
    griz.id(&["apply", &plan], "apply")?;
    let again = griz.run(
        &[
            &args[..],
            &["--purpose", "retry", "--idempotency-key", "original"],
        ]
        .concat(),
    )?;
    assert_eq!(again.code, Some(0), "{}", again.json);
    assert_eq!(
        again.json,
        json!({
            "id": plan, "outcome": "passed", "replayed": true
        })
    );
    Ok(())
}

#[test]
fn workspace_edit_replays_after_its_text_was_applied() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "const OLD: u8 = 1;\n")?;
    let uri = format!("file://{}", griz.path("a.rs").display());
    let edit = json!({"changes": {uri: [{
        "range": {
            "start": {"line": 0, "character": 6},
            "end": {"line": 0, "character": 9}
        },
        "newText": "NEW"
    }]}});
    check_retry(&griz, &edit)?;
    assert_eq!(griz.read("a.rs")?, "const NEW: u8 = 1;\n");
    Ok(())
}

#[test]
fn workspace_edit_replays_after_its_source_was_renamed() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn a() {}\n")?;
    let old = format!("file://{}", griz.path("a.rs").display());
    let new = format!("file://{}", griz.path("b.rs").display());
    let edit = json!({"documentChanges": [
        {"kind": "rename", "oldUri": old, "newUri": new}
    ]});
    check_retry(&griz, &edit)?;
    assert!(!griz.path("a.rs").exists());
    assert_eq!(griz.read("b.rs")?, "fn a() {}\n");
    Ok(())
}
