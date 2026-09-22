//! Compact retries preserve their original verdict without repeating work.

use crate::common::{Griz, TestResult};
use serde_json::json;

fn check_compact_replay(find: &str, expected: &str, count: &str) -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn old() {}\n")?;
    let ops = json!([{
        "op": "replace", "path": "a.rs",
        "find": {"text": find}, "replace": "new"
    }])
    .to_string();
    let args = [
        "plan",
        "--ops",
        &ops,
        "--expect-edits",
        count,
        "--purpose",
        "test replay",
        "--idempotency-key",
        "same",
    ];
    let first = griz.run(&args)?;
    assert_eq!(first.json["outcome"], expected, "{}", first.json);
    let code = i32::from(expected != "passed");
    assert_eq!(first.code, Some(code));
    griz.write("a.rs", "workspace changed\n")?;
    let receipt = json!({
        "id": first.json["id"], "outcome": expected, "replayed": true
    });
    check_compact_levels(&griz, &args, &receipt, code)?;
    let detail = griz.run(&[&args[..], &["--verbosity", "info"]].concat())?;
    assert_eq!(detail.code, Some(code));
    assert_eq!(detail.json["id"], first.json["id"]);
    assert_eq!(detail.json["outcome"], expected);
    assert_eq!(detail.json["replayed"], true);
    assert!(detail.json["summary"].is_object(), "{}", detail.json);
    assert_eq!(griz.read("a.rs")?, "workspace changed\n");
    Ok(())
}

fn check_compact_levels(
    griz: &Griz,
    args: &[&str],
    expected: &serde_json::Value,
    code: i32,
) -> TestResult {
    for level in ["off", "error"] {
        let again = griz.run(&[args, &["--verbosity", level]].concat())?;
        assert_eq!(again.code, Some(code));
        assert_eq!(&again.json, expected);
    }
    Ok(())
}

#[test]
fn compact_replay_preserves_passed_receipt() -> TestResult {
    check_compact_replay("old", "passed", "1")
}

#[test]
fn compact_replay_preserves_failed_receipt() -> TestResult {
    check_compact_replay("old", "failed", "2")
}

#[test]
fn compact_replay_preserves_error_receipt() -> TestResult {
    check_compact_replay("missing", "error", "1")
}
