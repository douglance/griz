//! Every verbosity level preserves the fields and values below it.

use crate::common::{Griz, TestResult};
use serde_json::{Value, json};
use std::error::Error;

const OPS: &str = r#"[{"op":"replace","path":"a.rs","find":"old","replace":"new"}]"#;
const MISSING: &str = r#"[{"op":"replace","path":"a.rs","find":"missing","replace":"new"}]"#;

fn workspace() -> Result<Griz, Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn old() {}\n")?;
    Ok(griz)
}

fn assert_superset(previous: &Value, current: &Value) -> TestResult {
    for (key, value) in previous.as_object().ok_or("response is not an object")? {
        assert_eq!(
            current.get(key),
            Some(value),
            "changed or missing {key}: {current}"
        );
    }
    Ok(())
}

fn check_levels(
    griz: &Griz,
    args: &[&str],
    key: &str,
    outcome: &str,
) -> Result<Value, Box<dyn Error>> {
    let base = [
        args,
        &["--purpose", "test verbosity", "--idempotency-key", key],
    ]
    .concat();
    let mut previous = json!({});
    for level in ["off", "error", "warn", "info", "debug", "trace"] {
        let run = griz.run(&[&base[..], &["--verbosity", level]].concat())?;
        assert_eq!(run.code, Some(i32::from(outcome != "passed")));
        assert_eq!(run.json["outcome"], outcome);
        let mut current = run.json;
        current
            .as_object_mut()
            .ok_or("response is not an object")?
            .remove("replayed");
        assert_superset(&previous, &current)?;
        previous = current;
    }
    assert!(previous["summary"].is_object());
    assert!(previous["detail"].is_object());
    Ok(previous)
}

#[test]
fn plan_verbosity_preserves_passed_failed_and_error_facts() -> TestResult {
    let griz = workspace()?;
    for (key, ops, count, expected) in [
        ("passed", OPS, "1", "passed"),
        ("failed", OPS, "2", "failed"),
        ("error", MISSING, "1", "error"),
    ] {
        let trace = check_levels(
            &griz,
            &["plan", "--ops", ops, "--expect-files", count],
            key,
            expected,
        )?;
        assert_eq!(trace["purpose"], "test verbosity");
        assert!(trace["problems"].is_array());
    }
    assert_eq!(griz.read("a.rs")?, "fn old() {}\n");
    Ok(())
}

#[test]
fn apply_verbosity_preserves_facts_and_full_record() -> TestResult {
    let griz = workspace()?;
    let plan = griz.id(&["plan", "--ops", OPS], "plan")?;
    let trace = check_levels(&griz, &["apply", &plan], "apply", "passed")?;
    assert_eq!(trace["kind"], "apply");
    assert_eq!(trace["files"].as_array().map(Vec::len), Some(1));
    assert_eq!(griz.read("a.rs")?, "fn new() {}\n");
    Ok(())
}

#[test]
fn failed_apply_trace_keeps_the_count_failure_reason() -> TestResult {
    let griz = workspace()?;
    let plan = griz.id(&["plan", "--ops", OPS], "plan")?;
    let trace = check_levels(
        &griz,
        &["apply", &plan, "--expect-files", "2"],
        "apply",
        "failed",
    )?;
    assert_eq!(
        trace["reason"],
        "expected 2 files, observed 1; no files written"
    );
    assert_eq!(griz.read("a.rs")?, "fn old() {}\n");
    Ok(())
}

#[test]
fn undo_verbosity_preserves_facts_and_full_record() -> TestResult {
    let griz = workspace()?;
    let plan = griz.id(&["plan", "--ops", OPS], "plan")?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    let trace = check_levels(&griz, &["undo", &operation], "undo", "passed")?;
    assert_eq!(trace["kind"], "undo");
    assert_eq!(trace["undoes"], operation);
    assert_eq!(griz.read("a.rs")?, "fn old() {}\n");
    Ok(())
}

#[test]
fn select_verbosity_preserves_facts_and_full_record() -> TestResult {
    let griz = workspace()?;
    let plan = griz.id(&["plan", "--ops", OPS], "plan")?;
    let trace = check_levels(
        &griz,
        &["select", &plan, "--paths", "a.rs"],
        "select",
        "passed",
    )?;
    assert_eq!(trace["selected_from"], plan);
    assert_eq!(trace["ops"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn absorb_verbosity_preserves_success_and_skipped_file_facts() -> TestResult {
    for (text, expected) in [(Some("fn new() {}\n"), "passed"), (None, "failed")] {
        let griz = workspace()?;
        let plan = griz.id(&["plan", "--ops", OPS], "plan")?;
        let operation = griz.id(&["apply", &plan], "apply")?;
        match text {
            Some(text) => griz.write("a.rs", text)?,
            None => std::fs::remove_file(griz.path("a.rs"))?,
        }
        let trace = check_levels(&griz, &["absorb", &operation], "absorb", expected)?;
        assert_eq!(trace["kind"], "apply");
        assert_eq!(trace["summary"]["skipped"], usize::from(text.is_none()));
    }
    Ok(())
}

#[test]
fn fresh_plan_verbosity_keeps_requested_payloads() -> TestResult {
    let griz = workspace()?;
    for (level, summary, detail, record) in [
        ("off", false, false, false),
        ("error", false, false, false),
        ("warn", false, false, false),
        ("info", true, false, false),
        ("debug", true, true, false),
        ("trace", true, true, true),
    ] {
        let run = griz.run(&[
            "plan",
            "--ops",
            OPS,
            "--purpose",
            "fresh plan",
            "--idempotency-key",
            level,
            "--verbosity",
            level,
        ])?;
        assert_eq!(run.code, Some(0));
        assert_eq!(run.json["outcome"], "passed");
        assert!(run.json.get("replayed").is_none());
        assert_eq!(run.json.get("summary").is_some(), summary);
        assert_eq!(run.json.get("detail").is_some(), detail);
        assert_eq!(run.json.get("ops").is_some(), record);
        assert_plan_payloads(&run.json);
    }
    assert_eq!(griz.read("a.rs")?, "fn old() {}\n");
    Ok(())
}

fn assert_plan_payloads(value: &Value) {
    if let Some(summary) = value.get("summary") {
        assert_eq!(summary["files"], 1);
        assert_eq!(summary["edits"], 1);
        assert_eq!(summary["syntax"], "clean");
    }
    if let Some(detail) = value.get("detail") {
        assert_eq!(detail["edits"].as_array().map(Vec::len), Some(1));
        assert!(detail["files"][0]["syntax"].is_object());
    }
    if value.get("ops").is_some() {
        assert_plan_record(value);
    }
}

fn assert_plan_record(value: &Value) {
    assert_eq!(value["ops"][0]["replace"], "new");
    assert_eq!(value["purpose"], "fresh plan");
    assert_eq!(value["syntax"], "clean");
}
