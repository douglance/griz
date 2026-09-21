//! Raising verbosity only adds keys; `trace` always still carries `outcome`.

use crate::common::{Griz, TestResult};
use serde_json::Value;

fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn assert_default_subset_of_trace(default: &Value, trace: &Value) {
    let trace_keys = keys(trace);
    for key in keys(default) {
        assert!(
            trace_keys.contains(&key),
            "trace is missing `{key}`: default {default} vs trace {trace}"
        );
    }
    assert!(
        trace_keys.iter().any(|key| key == "outcome"),
        "trace must still carry outcome: {trace}"
    );
}

#[test]
fn plan_trace_is_a_superset_of_the_default() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"}]"#;
    let default = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "d",
    ])?;
    let trace = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "t",
        "--verbosity",
        "trace",
    ])?;
    assert_default_subset_of_trace(&default.json, &trace.json);
    Ok(())
}

#[test]
fn apply_trace_is_a_superset_of_the_default() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"}]"#;
    let plan1 = griz.id(&["plan", "--ops", ops], "plan1")?;
    let default = griz.run(&["apply", &plan1, "--purpose", "t", "--idempotency-key", "d"])?;
    griz.write("a.rs", "a\n")?;
    let plan2 = griz.id(&["plan", "--ops", ops], "plan2")?;
    let trace = griz.run(&[
        "apply",
        &plan2,
        "--purpose",
        "t",
        "--idempotency-key",
        "t",
        "--verbosity",
        "trace",
    ])?;
    assert_default_subset_of_trace(&default.json, &trace.json);
    Ok(())
}

#[test]
fn undo_trace_is_a_superset_of_the_default() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"}]"#;
    let plan1 = griz.id(&["plan", "--ops", ops], "plan1")?;
    let op1 = griz.id(&["apply", &plan1], "apply1")?;
    let default = griz.run(&["undo", &op1, "--purpose", "t", "--idempotency-key", "d"])?;
    griz.write("a.rs", "a\n")?;
    let plan2 = griz.id(&["plan", "--ops", ops], "plan2")?;
    let op2 = griz.id(&["apply", &plan2], "apply2")?;
    let trace = griz.run(&[
        "undo",
        &op2,
        "--purpose",
        "t",
        "--idempotency-key",
        "t",
        "--verbosity",
        "trace",
    ])?;
    assert_default_subset_of_trace(&default.json, &trace.json);
    Ok(())
}

#[test]
fn select_trace_is_a_superset_of_the_default() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    griz.write("b.rs", "b\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"A"},{"op":"replace","path":"b.rs","find":{"text":"b"},"replace":"B"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let default = griz.run(&[
        "select",
        &plan,
        "--paths",
        "a.rs",
        "--purpose",
        "t",
        "--idempotency-key",
        "d",
    ])?;
    let trace = griz.run(&[
        "select",
        &plan,
        "--paths",
        "b.rs",
        "--purpose",
        "t",
        "--idempotency-key",
        "t",
        "--verbosity",
        "trace",
    ])?;
    assert_default_subset_of_trace(&default.json, &trace.json);
    Ok(())
}

#[test]
fn absorb_trace_is_a_superset_of_the_default() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn  f() {}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"f()"},"replace":"g()"}]"#;
    let plan1 = griz.id(&["plan", "--ops", ops], "plan1")?;
    let op1 = griz.id(&["apply", &plan1], "apply1")?;
    griz.write("a.rs", "fn g() {}\n")?;
    let default = griz.run(&["absorb", &op1, "--purpose", "t", "--idempotency-key", "d"])?;
    griz.write("b.rs", "fn  h() {}\n")?;
    let b_ops = r#"[{"op":"replace","path":"b.rs","find":{"text":"h()"},"replace":"i()"}]"#;
    let plan2 = griz.id(&["plan", "--ops", b_ops], "plan2")?;
    let operation2 = griz.id(&["apply", &plan2], "apply2")?;
    griz.write("b.rs", "fn i() {}\n")?;
    let trace = griz.run(&[
        "absorb",
        &operation2,
        "--purpose",
        "t",
        "--idempotency-key",
        "t",
        "--verbosity",
        "trace",
    ])?;
    assert_default_subset_of_trace(&default.json, &trace.json);
    Ok(())
}
