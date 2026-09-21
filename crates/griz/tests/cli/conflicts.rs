//! Merge conflicts and their blob ids, at the CLI boundary.

use crate::common::{Griz, TestResult};

#[test]
fn an_undo_merge_conflict_carries_blob_ids_get_can_read() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "one\ntwo\nthree\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"one"},"replace":"ONE"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let op = griz.id(&["apply", &plan], "apply")?;
    griz.write("a.rs", "ONE-ALSO-CHANGED\ntwo\nthree\n")?;
    let undo = griz.run(&[
        "undo",
        &op,
        "--on-stale",
        "merge",
        "--verbosity",
        "debug",
        "--purpose",
        "t",
        "--idempotency-key",
        "u",
    ])?;
    // Nothing was written, so this is an error rather than a partial failure.
    assert_eq!(undo.json["outcome"], "error", "{}", undo.json);
    let conflict = &undo.json["detail"]["merge_conflicts"][0];
    assert_eq!(
        conflict["regions"],
        serde_json::json!([{ "start": 1, "end": 1 }])
    );
    for field in ["base", "planned", "current"] {
        let id = conflict[field].as_str().ok_or(field)?;
        assert!(id.starts_with("blob_"), "{field} = {id}");
    }
    let current_id = conflict["current"].as_str().ok_or("no current id")?;
    let got = griz.run(&["get", current_id])?;
    assert_eq!(got.json["text"], "ONE-ALSO-CHANGED\ntwo\nthree\n");
    let current_fingerprint = conflict["current_fingerprint"]
        .as_str()
        .ok_or("no fingerprint")?;
    assert!(current_id.ends_with(current_fingerprint));
    Ok(())
}
