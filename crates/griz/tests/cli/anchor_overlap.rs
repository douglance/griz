//! Overlapping tolerant anchors must fail as a recorded plan, not a panic.

use crate::common::{Griz, TestResult};
use griz_core::ProblemKind;
use serde_json::json;

#[test]
fn overlapping_anchors_report_all_candidates_and_refuse_apply() -> TestResult {
    let griz = Griz::new()?;
    let original = "a \na \na ";
    griz.write("a.txt", original)?;
    let ops = json!([{
        "op": "replace", "path": "a.txt", "find": "a\na",
        "replace": "", "occurrence": "all"
    }])
    .to_string();
    let planned = griz.run(&[
        "plan",
        "--ops",
        &ops,
        "--purpose",
        "overlap",
        "--idempotency-key",
        "overlap-plan",
    ])?;
    assert_eq!(planned.code, Some(1), "{}", planned.json);
    assert_eq!(planned.json["outcome"], "error");
    let id = planned.json["id"].as_str().ok_or("no plan id")?;
    let record = griz.store()?.plan(id)?;
    assert!(record.files.is_empty());
    assert_eq!(record.problems.len(), 1);
    assert_eq!(
        record.problems[0].kind,
        ProblemKind::Ambiguous { lines: vec![1, 2] }
    );
    let applied = griz.run(&[
        "apply",
        id,
        "--purpose",
        "refuse overlap",
        "--idempotency-key",
        "overlap-apply",
    ])?;
    assert_ne!(applied.code, Some(0), "{}", applied.json);
    assert_eq!(griz.read("a.txt")?, original);
    Ok(())
}
