//! Returned write errors clean only the call's owned staged files.

use crate::common::{Griz, TestResult};
use serde_json::{Value, json};
use std::{error::Error, fs, path::PathBuf};

const SENTINEL: &str = ".other.griz-keep.tmp";

fn fixture() -> Result<(Griz, String), Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write(SENTINEL, "unrelated")?;
    for name in ["a.txt", "b.txt"] {
        griz.write(name, "before\n")?;
    }
    let ops = json!([
        {"op":"replace","path":"a.txt","find":"before","replace":"after"},
        {"op":"replace","path":"b.txt","find":"before","replace":"after"}
    ]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    Ok((griz, plan))
}

fn staged(griz: &Griz) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    Ok(fs::read_dir(griz.work.path())?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| {
            entry.file_name().to_string_lossy().contains(".griz-") && entry.file_name() != SENTINEL
        })
        .map(|entry| entry.path())
        .collect())
}

fn fail_apply(griz: &Griz, plan: &str, point: &str) -> TestResult {
    let output = griz
        .command(&[
            "apply",
            plan,
            "--purpose",
            "cleanup test",
            "--idempotency-key",
            "failed",
        ])
        .env("GRIZ_FAILPOINT", point)
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let body: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(body["code"], "STORE_ERROR");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains(point))
    );
    Ok(())
}

fn check_files(griz: &Griz, expected: &str) -> TestResult {
    assert_eq!(griz.read("a.txt")?, expected);
    assert_eq!(griz.read("b.txt")?, expected);
    assert_eq!(griz.read(SENTINEL)?, "unrelated");
    assert!(staged(griz)?.is_empty(), "owned staging files remain");
    Ok(())
}

#[test]
fn returned_staging_error_cleans_the_partial_file_and_does_not_recover_it() -> TestResult {
    let (griz, plan) = fixture()?;
    fail_apply(&griz, &plan, "after_stage_write")?;
    check_files(&griz, "before\n")?;
    let log = griz.run(&["log"])?;
    assert_eq!(log.json["operations"][0]["state"], "failed");
    check_files(&griz, "before\n")?;
    griz.id(&["apply", &plan], "retry")?;
    check_files(&griz, "after\n")
}

#[test]
fn returned_commit_error_cleans_stages_and_keeps_recovery_possible() -> TestResult {
    let (griz, plan) = fixture()?;
    fail_apply(&griz, &plan, "before_commit")?;
    check_files(&griz, "before\n")?;
    let log = griz.run(&["log"])?;
    assert_eq!(log.json["operations"][0]["state"], "applied");
    let id = log.json["operations"][0]["id"]
        .as_str()
        .ok_or("no operation")?;
    assert_eq!(griz.run(&["get", id])?.json["recovered"], true);
    check_files(&griz, "after\n")
}
