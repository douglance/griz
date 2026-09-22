//! Absorb receipts retain each call's results, even as the operation evolves.

use crate::common::{Griz, Run, TestResult};
use serde_json::{Value, json};

fn fixture() -> Result<(Griz, String), Box<dyn std::error::Error>> {
    let griz = Griz::new()?;
    griz.write("a.txt", "before a\n")?;
    griz.write("b.txt", "before b\n")?;
    let ops = json!([
        {"op": "replace", "path": "a.txt", "find": "before", "replace": "after"},
        {"op": "replace", "path": "b.txt", "find": "before", "replace": "after"}
    ])
    .to_string();
    let plan = griz.id(&["plan", "--ops", &ops], "plan")?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    Ok((griz, operation))
}

fn absorb(
    griz: &Griz,
    op: &str,
    key: &str,
    level: &str,
) -> Result<Run, Box<dyn std::error::Error>> {
    griz.run(&[
        "absorb",
        op,
        "--purpose",
        "test",
        "--idempotency-key",
        key,
        "--verbosity",
        level,
    ])
}

fn isolated_store(griz: &Griz) -> Result<griz_store::Store, Box<dyn std::error::Error>> {
    let command = griz.command(&[]);
    let home = command
        .get_envs()
        .find_map(|(key, value)| (key == "GRIZ_HOME").then_some(value).flatten())
        .ok_or("missing isolated home")?;
    Ok(griz_store::Store::open(std::path::Path::new(home))?)
}

fn without_replayed(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("replayed");
    }
    value
}

#[test]
fn absorb_retry_retains_skipped_files_and_failure_reason() -> TestResult {
    let (griz, op) = fixture()?;
    std::fs::remove_file(griz.path("b.txt"))?;
    griz.write("a.txt", "formatted a\n")?;
    let first = absorb(&griz, &op, "first", "info")?;
    assert_eq!(first.code, Some(1));
    assert_eq!(first.json["summary"]["skipped"], 1);
    assert_eq!(first.json["summary"]["absorbed"], 1);
    assert!(first.json["reason"].is_string());
    let retry = absorb(&griz, &op, "first", "info")?;
    assert_eq!(retry.code, first.code);
    assert_eq!(retry.json["replayed"], true);
    assert_eq!(without_replayed(retry.json), first.json);
    assert!(!griz.path("b.txt").exists());
    assert_eq!(griz.read("a.txt")?, "formatted a\n");
    Ok(())
}

#[test]
fn absorb_retry_preserves_every_verbosity_after_later_absorbs() -> TestResult {
    let (griz, op) = fixture()?;
    griz.write("a.txt", "first formatting\n")?;
    let levels = ["off", "error", "warn", "info", "debug", "trace"];
    let original = levels
        .iter()
        .map(|level| absorb(&griz, &op, "first", level))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(original[3].json["summary"]["absorbed"], 1);
    griz.write("a.txt", "second formatting\n")?;
    griz.write("b.txt", "second b\n")?;
    let later = absorb(&griz, &op, "second", "info")?;
    assert_eq!(later.json["summary"]["absorbed"], 2);
    for (level, first) in levels.iter().zip(original) {
        let retry = absorb(&griz, &op, "first", level)?;
        assert_eq!(retry.code, first.code);
        assert_eq!(retry.json["replayed"], true);
        assert_eq!(
            without_replayed(retry.json),
            without_replayed(first.json),
            "{level}"
        );
    }
    assert_eq!(griz.read("a.txt")?, "second formatting\n");
    assert_eq!(griz.read("b.txt")?, "second b\n");
    restores_original(&griz, &op)
}

fn restores_original(griz: &Griz, op: &str) -> TestResult {
    griz.id(&["undo", op], "undo")?;
    assert_eq!(griz.read("a.txt")?, "before a\n");
    assert_eq!(griz.read("b.txt")?, "before b\n");
    Ok(())
}

#[test]
fn absorb_no_change_retry_reports_zero_newly_absorbed_files() -> TestResult {
    let (griz, op) = fixture()?;
    griz.write("a.txt", "formatted a\n")?;
    absorb(&griz, &op, "first", "info")?;
    let first = absorb(&griz, &op, "unchanged", "info")?;
    assert_eq!(first.json["summary"]["absorbed"], 0);
    let retry = absorb(&griz, &op, "unchanged", "info")?;
    assert_eq!(retry.code, Some(0));
    assert_eq!(without_replayed(retry.json), first.json);
    Ok(())
}

#[test]
fn compact_absorb_retry_does_not_read_snapshot_blobs() -> TestResult {
    let (griz, op) = fixture()?;
    griz.write("a.txt", "formatted a\n")?;
    let first = absorb(&griz, &op, "first", "error")?;
    let store = isolated_store(&griz)?;
    std::fs::remove_dir_all(store.home().join("blobs"))?;
    let retry = absorb(&griz, &op, "first", "error")?;
    assert_eq!(retry.code, Some(0));
    assert_eq!(without_replayed(retry.json), first.json);
    let detail = absorb(&griz, &op, "first", "info")?;
    assert_eq!(detail.code, Some(1));
    assert_eq!(detail.json["code"], "NOT_FOUND");
    assert_eq!(griz.read("a.txt")?, "formatted a\n");
    Ok(())
}

#[test]
fn legacy_absorb_receipts_replay_compactly_without_inventing_details() -> TestResult {
    let (griz, op) = fixture()?;
    griz.write("a.txt", "formatted a\n")?;
    let first = absorb(&griz, &op, "legacy", "error")?;
    let store = isolated_store(&griz)?;
    store.finish_receipt("absorb:legacy", &first.json)?;
    for level in ["off", "error"] {
        let retry = absorb(&griz, &op, "legacy", level)?;
        assert_eq!(retry.code, Some(0));
        assert_eq!(without_replayed(retry.json), first.json);
    }
    for level in ["warn", "info", "debug", "trace"] {
        let retry = absorb(&griz, &op, "legacy", level)?;
        assert_eq!(retry.code, Some(1));
        assert_eq!(retry.json["code"], "IDEMPOTENCY_DETAIL_UNAVAILABLE");
    }
    assert_eq!(griz.read("a.txt")?, "formatted a\n");
    griz.id(&["undo", &op], "undo")?;
    assert_eq!(griz.read("a.txt")?, "before a\n");
    Ok(())
}
