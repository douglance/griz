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

#[test]
fn unchecked_range_refuses_the_entire_batch() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.txt", "one OLD\n")?;
    griz.write("b.txt", "keep\n")?;
    let observed = griz.read("a.txt")?;
    griz.write("a.txt", "one NEW\n")?;
    let ops = json!([
        {"op":"replace","path":"a.txt","range":{"start":0,"end":observed.len()},
         "replace":observed.replace("one", "two")},
        {"op":"replace","path":"b.txt","find":"keep","replace":"changed"}
    ])
    .to_string();
    let planned = griz.run(&[
        "plan",
        "--ops",
        &ops,
        "--purpose",
        "range guard",
        "--idempotency-key",
        "range-plan",
    ])?;
    assert_eq!(planned.code, Some(1), "{}", planned.json);
    assert_eq!(planned.json["outcome"], "error");
    let id = planned.json["id"].as_str().ok_or("no plan id")?;
    let record = griz.store()?.plan(id)?;
    assert_eq!(record.problems.len(), 1);
    assert!(
        matches!(&record.problems[0].kind, ProblemKind::Invalid { message }
        if message.contains("expect_hash") && message.contains("find"))
    );
    let applied = griz.run(&[
        "apply",
        id,
        "--purpose",
        "refuse unchecked range",
        "--idempotency-key",
        "range-apply",
    ])?;
    assert_ne!(applied.code, Some(0), "{}", applied.json);
    assert_eq!(griz.read("a.txt")?, "one NEW\n");
    assert_eq!(griz.read("b.txt")?, "keep\n");
    Ok(())
}
