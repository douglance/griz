//! Invalid restore boundaries never become broad history restores.

use crate::common::{Griz, TestResult};
use serde_json::json;

#[test]
fn unknown_restore_boundary_refuses_and_frees_the_key_for_correction() -> TestResult {
    let griz = Griz::new()?;
    griz.write("repo-a/a.txt", "one\n")?;
    griz.write("repo-b/b.txt", "two\n")?;
    let ops = json!([
        {"op": "replace", "path": "repo-a/a.txt", "find": "one", "replace": "ONE"},
        {"op": "replace", "path": "repo-b/b.txt", "find": "two", "replace": "TWO"},
    ])
    .to_string();
    let plan = griz.id(&["plan", "--ops", &ops], "plan")?;
    let applied = griz.id(&["apply", &plan], "apply")?;
    let invalid = griz.run(&[
        "undo",
        "--since",
        "op_00000000000000000000000000000000",
        "--purpose",
        "invalid boundary",
        "--idempotency-key",
        "restore",
    ])?;
    assert_eq!(invalid.code, Some(1), "{}", invalid.json);
    assert_eq!(invalid.json["code"], "NOT_FOUND");
    assert_eq!(griz.read("repo-a/a.txt")?, "ONE\n");
    assert_eq!(griz.read("repo-b/b.txt")?, "TWO\n");
    let log = griz.run(&["log"])?;
    assert_eq!(log.json["operations"].as_array().map(Vec::len), Some(1));
    griz.id(&["undo", "--since", &applied], "restore")?;
    assert_eq!(griz.read("repo-a/a.txt")?, "one\n");
    assert_eq!(griz.read("repo-b/b.txt")?, "two\n");
    Ok(())
}
