//! Verdicts, verbosity, exit codes, and idempotency.

use crate::common::{Griz, TestResult};

const MISSING: &str =
    r#"[{"op":"replace","path":"a.rs","find":{"text":"fn beta(x: u64) {}"},"replace":""}]"#;

#[test]
fn a_plan_that_cannot_land_is_an_error_with_the_nearest_text_on_request() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn alpha() {}\nfn beta(x: u32) {}\n")?;
    let base = [
        "plan",
        "--ops",
        MISSING,
        "--purpose",
        "t",
        "--idempotency-key",
        "k",
    ];
    let run = griz.run(&base)?;
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.json.as_object().map(serde_json::Map::len),
        Some(2),
        "{}",
        run.json
    );
    assert_eq!(run.json["outcome"], "error");
    let id = run.json["id"].as_str().ok_or("no id")?;
    let record = griz.run(&["get", id])?;
    assert_eq!(record.json["problems"][0]["nearest"]["line"], 2);
    assert_eq!(
        record.json["problems"][0]["nearest"]["text"],
        "fn beta(x: u32) {}"
    );
    let warn = griz.run(&[&base[..], &["--verbosity", "warn"]].concat())?;
    assert!(
        warn.json["reason"]
            .as_str()
            .is_some_and(|r| r.contains("nearest text is at line 2")),
        "{}",
        warn.json
    );
    Ok(())
}

#[test]
fn an_unmet_match_count_fails_find_and_exits_nonzero() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "x x\n")?;
    let run = griz.run(&["find", "--literal", "x", "--expect-matches", "3"])?;
    assert_eq!(
        (run.code, run.json["outcome"].as_str()),
        (Some(1), Some("failed"))
    );
    Ok(())
}

#[test]
fn trace_answers_with_the_full_record_and_the_verdict() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"b"}]"#;
    let run = griz.run(&[
        "plan",
        "--ops",
        ops,
        "--purpose",
        "t",
        "--idempotency-key",
        "k",
        "--verbosity",
        "trace",
    ])?;
    assert_eq!(run.json["outcome"], "passed");
    assert_eq!(run.json["edits"][0]["confidence"], "machine");
    assert_eq!(run.json["purpose"], "t");
    Ok(())
}

#[test]
fn a_repeated_key_replays_and_a_changed_input_conflicts() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"b"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let first = griz.run(&[
        "apply",
        &plan,
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    std::fs::write(griz.path("a.rs"), "a\n")?;
    let again = griz.run(&[
        "apply",
        &plan,
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(again.json["id"], first.json["id"]);
    assert_eq!(again.json["replayed"], true);
    assert_eq!(griz.read("a.rs")?, "a\n", "a replay must not write again");
    let other = griz.run(&[
        "apply",
        &plan,
        "--min-confidence",
        "maybe",
        "--purpose",
        "t",
        "--idempotency-key",
        "same",
    ])?;
    assert_eq!(other.json["code"], "IDEMPOTENCY_CONFLICT");
    Ok(())
}

#[test]
fn a_replayed_absorb_renders_the_same_shape_as_the_first_answer() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn  f() {}\n")?;
    let ops = r#"[{"op":"replace","path":"a.rs","find":{"text":"f()"},"replace":"g()"}]"#;
    let plan = griz.id(&["plan", "--ops", ops], "plan")?;
    let op = griz.id(&["apply", &plan], "apply")?;
    griz.write("a.rs", "fn g() {}\n")?;
    let base = [
        "absorb",
        op.as_str(),
        "--purpose",
        "t",
        "--idempotency-key",
        "absorb",
        "--verbosity",
        "info",
    ];
    let first = griz.run(&base)?;
    assert_eq!(first.json["outcome"], "passed", "{}", first.json);
    assert!(first.json["summary"]["absorbed"].is_u64(), "{}", first.json);
    assert!(first.json["summary"]["skipped"].is_u64(), "{}", first.json);
    let again = griz.run(&base)?;
    assert_eq!(again.json["replayed"], true);
    assert_eq!(again.json["id"], first.json["id"]);
    let (mut first_keys, mut again_keys) = (
        first
            .json
            .as_object()
            .ok_or("no object")?
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        again
            .json
            .as_object()
            .ok_or("no object")?
            .keys()
            .filter(|k| *k != "replayed")
            .cloned()
            .collect::<Vec<_>>(),
    );
    first_keys.sort();
    again_keys.sort();
    assert_eq!(first_keys, again_keys, "{} vs {}", first.json, again.json);
    assert!(again.json["summary"]["absorbed"].is_u64(), "{}", again.json);
    assert!(again.json["summary"]["skipped"].is_u64(), "{}", again.json);
    Ok(())
}

#[test]
fn a_rejected_request_frees_its_key_for_the_corrected_retry() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "a\n")?;
    let bad = griz.run(&[
        "plan",
        "--ops",
        r#"[{"op":"nonsense"}]"#,
        "--purpose",
        "t",
        "--idempotency-key",
        "k",
    ])?;
    assert_eq!(bad.json["code"], "VALIDATION_ERROR");
    let good = r#"[{"op":"replace","path":"a.rs","find":{"text":"a"},"replace":"b"}]"#;
    griz.id(&["plan", "--ops", good], "k")?;
    Ok(())
}
