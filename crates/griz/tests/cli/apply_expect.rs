//! Apply expectations must be checked before changing source files.

use crate::common::{Griz, TestResult};

const OPS: &str = r#"[
    {"op":"replace","path":"edit.txt","find":"before","replace":"after"},
    {"op":"create","path":"new.txt","text":"created"},
    {"op":"delete","path":"delete.txt"},
    {"op":"move","path":"old.txt","to":"moved.txt"}
]"#;

fn prepared() -> Result<(Griz, String), Box<dyn std::error::Error>> {
    let griz = Griz::new()?;
    griz.write("edit.txt", "before")?;
    griz.write("delete.txt", "keep until apply")?;
    griz.write("old.txt", "move me")?;
    let plan = griz.id(&["plan", "--ops", OPS, "--expect-files", "5"], "plan")?;
    Ok((griz, plan))
}

fn assert_untouched(griz: &Griz) -> TestResult {
    assert_eq!(griz.read("edit.txt")?, "before");
    assert_eq!(griz.read("delete.txt")?, "keep until apply");
    assert_eq!(griz.read("old.txt")?, "move me");
    assert!(!griz.path("new.txt").exists());
    assert!(!griz.path("moved.txt").exists());
    Ok(())
}

#[test]
fn mismatched_apply_count_writes_nothing_and_records_a_failed_operation() -> TestResult {
    let (griz, plan) = prepared()?;
    let run = griz.run(&[
        "apply",
        &plan,
        "--expect-files",
        "4",
        "--purpose",
        "test",
        "--idempotency-key",
        "apply",
        "--verbosity",
        "trace",
    ])?;
    assert_eq!(run.code, Some(1));
    assert_eq!(run.json["outcome"], "failed");
    assert_untouched(&griz)?;
    assert_eq!(run.json["state"], "failed");
    assert_eq!(run.json["plan"], plan);
    assert_eq!(run.json["files"], serde_json::json!([]));
    let id = run.json["id"].as_str().ok_or("no operation id")?;
    assert!(id.starts_with("op_"));
    let stored = griz.run(&["get", id])?;
    assert_eq!(stored.json["state"], "failed");
    assert_eq!(stored.json["files"], serde_json::json!([]));
    assert!(
        stored.json["reason"]
            .as_str()
            .is_some_and(|s| s.contains("observed 5"))
    );
    Ok(())
}

#[test]
fn failed_apply_count_replays_without_writing_and_new_key_can_apply() -> TestResult {
    let (griz, plan) = prepared()?;
    let args = [
        "apply",
        &plan,
        "--expect-files",
        "0",
        "--purpose",
        "test",
        "--idempotency-key",
        "apply",
        "--verbosity",
        "warn",
    ];
    let first = griz.run(&args)?;
    let replay = griz.run(&args)?;
    assert_eq!(replay.code, Some(1));
    assert_eq!(replay.json["outcome"], "failed");
    assert_eq!(replay.json["id"], first.json["id"]);
    assert_eq!(replay.json["reason"], first.json["reason"]);
    assert_eq!(replay.json["replayed"], true);
    assert_untouched(&griz)?;
    let conflict = griz.run(&[
        "apply",
        &plan,
        "--expect-files",
        "5",
        "--purpose",
        "test",
        "--idempotency-key",
        "apply",
    ])?;
    assert_eq!(conflict.json["code"], "IDEMPOTENCY_CONFLICT");
    griz.id(&["apply", &plan, "--expect-files", "5"], "corrected")?;
    assert_applied(&griz)?;
    Ok(())
}

fn assert_applied(griz: &Griz) -> TestResult {
    assert_eq!(griz.read("edit.txt")?, "after");
    assert_eq!(griz.read("new.txt")?, "created");
    assert_eq!(griz.read("moved.txt")?, "move me");
    assert!(!griz.path("delete.txt").exists());
    assert!(!griz.path("old.txt").exists());
    Ok(())
}

#[test]
fn empty_plan_accepts_zero_expected_files() -> TestResult {
    let griz = Griz::new()?;
    griz.write("edit.txt", "unchanged")?;
    let ops = r#"[{"op":"replace","path":"edit.txt","find":"unchanged","replace":"unchanged"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    griz.id(&["apply", &plan, "--expect-files", "0"], "apply")?;
    assert_eq!(griz.read("edit.txt")?, "unchanged");
    Ok(())
}
