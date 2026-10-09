//! A file destination cannot also contain another batch destination.
use crate::common::{Griz, TestResult};
use serde_json::json;

#[test]
fn ancestor_destinations_are_refused_before_staging_or_journaling() -> TestResult {
    let griz = Griz::new()?;
    let mut ops = vec![
        json!({"op":"create","path":"parent","text":"file"}),
        json!({"op":"create","path":"parent/child.txt","text":"child"}),
    ];
    ops.extend((0..32).map(|index| {
        json!({
            "op":"create","path":format!("f{index:03}.txt"),"text":"unrelated"
        })
    }));
    let plan = griz.id(&["plan", "--ops", &serde_json::to_string(&ops)?], "plan")?;
    let output = griz.run(&[
        "apply",
        &plan,
        "--purpose",
        "test",
        "--idempotency-key",
        "apply",
    ])?;
    assert_eq!(output.code, Some(1));
    assert!(
        !griz.path("parent").exists(),
        "staging created an ancestor directory"
    );
    for index in 0..32 {
        assert!(!griz.path(&format!("f{index:03}.txt")).exists());
    }
    assert_eq!(griz.run(&["log"])?.json["operations"], json!([]));
    Ok(())
}

#[test]
fn case_alias_ancestors_are_refused_before_any_source_commit() -> TestResult {
    let griz = Griz::new()?;
    griz.write("case-probe", "probe")?;
    if !griz.path("CASE-PROBE").exists() {
        return Ok(());
    }
    let mut ops = vec![
        json!({"op":"create","path":"Parent","text":"file"}),
        json!({"op":"create","path":"parent/child.txt","text":"child"}),
    ];
    ops.extend((0..32).map(|index| {
        json!({
            "op":"create","path":format!("f{index:03}.txt"),"text":"unrelated"
        })
    }));
    let plan = griz.id(&["plan", "--ops", &serde_json::to_string(&ops)?], "plan")?;
    let output = griz.run(&[
        "apply",
        &plan,
        "--purpose",
        "test",
        "--idempotency-key",
        "apply",
    ])?;
    assert_eq!(output.code, Some(1));
    assert!(!griz.path("parent/child.txt").exists());
    for index in 0..32 {
        assert!(!griz.path(&format!("f{index:03}.txt")).exists());
    }
    let log = griz.run(&["log"])?;
    assert_eq!(log.json["operations"][0]["state"], "failed");
    crate::parallel_stage::check_stages(&griz)
}
