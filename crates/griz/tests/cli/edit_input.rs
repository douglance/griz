//! Native edit fields are checked before planning or reserving a retry key.
use crate::common::{Griz, TestResult};
use serde_json::{Value, json};

fn native_ops() -> Vec<Value> {
    vec![
        json!({"op":"replace","path":"probe.txt","find":"before","replace":"after"}),
        json!({"op":"insert","path":"probe.txt","anchor":"before","text":"after"}),
        json!({"op":"create","path":"new.txt","text":"after"}),
        json!({"op":"delete","path":"probe.txt"}),
        json!({"op":"move","path":"probe.txt","to":"moved.txt"}),
    ]
}

fn reject(griz: &Griz, ops: &Value, field: &str, key: &str) -> TestResult {
    let run = griz.run(&[
        "plan",
        "--ops",
        &ops.to_string(),
        "--purpose",
        "test",
        "--idempotency-key",
        key,
    ])?;
    assert_eq!(run.code, Some(1), "{}", run.json);
    assert_eq!(run.json["code"], "VALIDATION_ERROR", "{}", run.json);
    let message = run.json["message"].as_str().ok_or("no error message")?;
    assert!(message.contains("ops["), "{message}");
    assert!(
        message.contains(&format!("unknown field `{field}`")),
        "{message}"
    );
    assert!(run.json.get("id").is_none(), "{}", run.json);
    assert_eq!(griz.read("probe.txt")?, "before\n");
    Ok(())
}

#[test]
fn full_anchors_preserve_scope_and_whole_line_matching() -> TestResult {
    let griz = Griz::new()?;
    griz.write("probe.txt", "before\nprefix\nbefore_end\nbefore\n")?;
    let ops = json!([{"op":"replace","path":"probe.txt",
        "find":{"text":"before","after":"prefix","whole_lines":true},"replace":"after"}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(
        griz.read("probe.txt")?,
        "before\nprefix\nbefore_end\nafter\n"
    );
    Ok(())
}
#[test]
fn native_operations_reject_misspelled_fingerprint_fields() -> TestResult {
    let griz = Griz::new()?;
    griz.write("probe.txt", "before\n")?;
    for (index, mut op) in native_ops().into_iter().enumerate() {
        op["expect_hahs"] = json!("0".repeat(64));
        reject(&griz, &json!([op]), "expect_hahs", &format!("bad-{index}"))?;
    }
    Ok(())
}

#[test]
fn insert_rejects_an_unsupported_occurrence() -> TestResult {
    let griz = Griz::new()?;
    griz.write("probe.txt", "before\n")?;
    let op = json!({"op":"insert","path":"probe.txt","anchor":"before",
        "text":"after","occurrence":{"nth":2}});
    reject(&griz, &json!([op]), "occurrence", "bad-insert")
}

#[test]
fn native_locators_reject_unknown_fields_by_name() -> TestResult {
    let griz = Griz::new()?;
    griz.write("probe.txt", "before\n")?;
    let cases = [
        (
            json!({"op":"replace","path":"probe.txt","replace":"after",
            "find":{"text":"before","whole_line":true}}),
            "whole_line",
        ),
        (
            json!({"op":"insert","path":"probe.txt","text":"after",
            "anchor":{"text":"before","afterr":"prefix"}}),
            "afterr",
        ),
        (
            json!({"op":"replace","path":"probe.txt","replace":"after",
            "range":{"start":0,"end":6,"end_byte":6}}),
            "end_byte",
        ),
        (
            json!({"op":"replace","path":"probe.txt","replace":"after",
            "pattern":{"pattern":"before","langauge":"rust"}}),
            "langauge",
        ),
    ];
    for (op, field) in cases {
        reject(&griz, &json!([op]), field, field)?;
    }
    Ok(())
}

#[test]
fn invalid_fields_report_the_index_and_do_not_reserve_the_key() -> TestResult {
    let griz = Griz::new()?;
    griz.write("probe.txt", "before\n")?;
    let mut ops = json!([
        {"op":"create","path":"new.txt","text":"created\n"},
        {"op":"replace","path":"probe.txt","find":"before","replace":"after",
         "expect_hahs":"incorrect"}
    ]);
    reject(&griz, &ops, "expect_hahs", "retry")?;
    let error = griz.run(&[
        "plan",
        "--ops",
        &ops.to_string(),
        "--purpose",
        "test",
        "--idempotency-key",
        "retry",
    ])?;
    assert!(
        error.json["message"]
            .as_str()
            .is_some_and(|s| s.starts_with("ops[1]:"))
    );
    ops[1]
        .as_object_mut()
        .ok_or("not an operation")?
        .remove("expect_hahs");
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "retry")?;
    let applied = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("probe.txt")?, "after\n");
    assert_eq!(griz.read("new.txt")?, "created\n");
    griz.id(&["undo", &applied], "undo")?;
    assert_eq!(griz.read("probe.txt")?, "before\n");
    assert!(!griz.path("new.txt").exists());
    Ok(())
}

#[test]
fn overwrite_guard_refuses_stale_reads_and_allows_fresh_undo() -> TestResult {
    let griz = Griz::new()?;
    griz.write("probe.txt", "observed\n")?;
    let read = griz.run(&["read", "probe.txt"])?;
    let mut ops = json!([{"op":"create","path":"probe.txt","text":"replacement\n",
        "overwrite":true,"expect_hash":read.json["hash"]}]);
    griz.write("probe.txt", "newer\n")?;
    let stale = griz.run(&[
        "plan",
        "--ops",
        &ops.to_string(),
        "--purpose",
        "test",
        "--idempotency-key",
        "stale",
        "--verbosity",
        "trace",
    ])?;
    assert_eq!(stale.code, Some(1));
    assert_eq!(stale.json["outcome"], "error", "{}", stale.json);
    assert_eq!(stale.json["problems"][0]["kind"], "stale", "{}", stale.json);
    assert_eq!(griz.read("probe.txt")?, "newer\n");
    let read = griz.run(&["read", "probe.txt"])?;
    ops[0]["expect_hash"] = read.json["hash"].clone();
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "fresh")?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("probe.txt")?, "replacement\n");
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(griz.read("probe.txt")?, "newer\n");
    Ok(())
}
