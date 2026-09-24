//! Explicit recovery of operations that resolved in a partially failed plan.
use crate::common::{Griz, TestResult};
use serde_json::json;
use std::error::Error;

const BEFORE: &str = "first\nrepeat\nrepeat\nlast\n";
const AFTER: &str = "FIRST\nrepeat\nrepeat\nLAST\n";

fn partial(griz: &Griz) -> Result<String, Box<dyn Error>> {
    griz.write("a.txt", BEFORE)?;
    let ops = json!([
        {"op":"replace","path":"a.txt","find":"first","replace":"FIRST"},
        {"op":"replace","path":"a.txt","find":"missing","replace":"MISSING"},
        {"op":"replace","path":"a.txt","find":"repeat","replace":"REPEAT"},
        {"op":"replace","path":"a.txt","find":"last","replace":"LAST"}
    ]);
    let run = griz.run(&[
        "plan",
        "--ops",
        &ops.to_string(),
        "--purpose",
        "partial",
        "--idempotency-key",
        "partial",
    ])?;
    assert_eq!(run.json["outcome"], "error", "{}", run.json);
    Ok(run.json["id"].as_str().ok_or("no plan id")?.to_string())
}

#[test]
fn resolved_selection_applies_only_good_operations_and_undoes_exactly() -> TestResult {
    let griz = Griz::new()?;
    let original = partial(&griz)?;
    let selected = griz.id(&["select", &original, "--resolved-only"], "select")?;
    let record = griz.run(&["get", &selected])?.json;
    assert_eq!(record["selected_from"], original);
    assert_eq!(record["ops"].as_array().ok_or("no ops")?.len(), 2);
    assert_eq!(record["edits"].as_array().ok_or("no edits")?.len(), 2);
    assert_eq!(record["problems"], json!([]));
    assert_eq!(griz.read("a.txt")?, BEFORE);
    let operation = griz.id(&["apply", &selected], "apply")?;
    assert_eq!(griz.read("a.txt")?, AFTER);
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(griz.read("a.txt")?, BEFORE);
    Ok(())
}

#[test]
fn resolved_selection_remains_bound_to_the_original_file() -> TestResult {
    let griz = Griz::new()?;
    let original = partial(&griz)?;
    griz.write("a.txt", "concurrent\n")?;
    let selected = griz.id(&["select", &original, "--resolved-only"], "select")?;
    let result = griz.run(&[
        "apply",
        &selected,
        "--purpose",
        "refuse stale",
        "--idempotency-key",
        "stale",
    ])?;
    assert_ne!(result.code, Some(0));
    assert_ne!(result.json["outcome"], "passed");
    assert_eq!(griz.read("a.txt")?, "concurrent\n");
    Ok(())
}

#[test]
fn resolved_selection_is_opt_in_and_part_of_retry_identity() -> TestResult {
    let griz = Griz::new()?;
    let original = partial(&griz)?;
    let args = [
        "select",
        &original,
        "--purpose",
        "same selection",
        "--idempotency-key",
        "selection",
    ];
    let first = griz.run(&args)?;
    assert_eq!(first.json["outcome"], "error");
    let mut explicit_false = args.to_vec();
    explicit_false.push("--resolved-only=false");
    let replay = griz.run(&explicit_false)?;
    assert_eq!(replay.json["id"], first.json["id"]);
    assert_eq!(replay.json["outcome"], "error");
    let mut different = args.to_vec();
    different.push("--resolved-only");
    let conflict = griz.run(&different)?;
    assert_ne!(conflict.code, Some(0));
    assert!(conflict.json.to_string().contains("IDEMPOTENCY_CONFLICT"));
    assert_eq!(griz.read("a.txt")?, BEFORE);
    Ok(())
}

#[test]
fn resolved_selection_intersects_explicit_edit_filter() -> TestResult {
    let griz = Griz::new()?;
    let original = partial(&griz)?;
    let selected = griz.id(
        &["select", &original, "--resolved-only", "--edits", "e3"],
        "last-only",
    )?;
    griz.id(&["apply", &selected], "apply-last")?;
    assert_eq!(griz.read("a.txt")?, "first\nrepeat\nrepeat\nLAST\n");
    Ok(())
}
