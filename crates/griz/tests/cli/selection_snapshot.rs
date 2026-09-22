//! Selecting a net-zero plan must retain its original stale-file guard.

use crate::common::{Griz, TestResult};
use serde_json::json;

#[test]
fn selection_from_net_zero_plan_refuses_later_disk_contents() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.txt", "one\n")?;
    let ops = json!([
        {"op": "replace", "path": "a.txt", "find": {"text": "one"}, "replace": "ONE"},
        {"op": "replace", "path": "a.txt", "find": {"text": "ONE"}, "replace": "one"}
    ])
    .to_string();
    let plan = griz.id(&["plan", "--ops", &ops, "--expect-files", "0"], "net-zero")?;
    griz.id(&["apply", &plan, "--expect-files", "0"], "apply-noop")?;
    assert_eq!(griz.read("a.txt")?, "one\n");
    griz.write("a.txt", "one\nconcurrent\n")?;
    let selected = griz.id(&["select", &plan, "--edits", "e0"], "select-first")?;
    let run = griz.run(&[
        "apply",
        &selected,
        "--expect-files",
        "1",
        "--purpose",
        "try selection",
        "--idempotency-key",
        "apply-selection",
    ])?;
    assert_eq!(run.code, Some(1), "{}", run.json);
    assert_ne!(run.json["outcome"], "passed");
    assert_eq!(griz.read("a.txt")?, "one\nconcurrent\n");
    Ok(())
}
