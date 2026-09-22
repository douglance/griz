//! Large write batches preserve ordering, cleanup, and recovery.

use crate::common::{Griz, TestResult};
use serde_json::json;
use std::{error::Error, fs};

const FILES: usize = 64;

fn fixture() -> Result<(Griz, String), Box<dyn Error>> {
    let griz = Griz::new()?;
    let mut ops = Vec::new();
    for index in 0..FILES {
        let name = format!("f{index:03}.txt");
        griz.write(&name, &format!("before {index}\n"))?;
        ops.push(json!({
            "op":"replace","path":name,"find":"before","replace":"after"
        }));
    }
    let plan = griz.id(&["plan", "--ops", &json!(ops).to_string()], "plan")?;
    Ok((griz, plan))
}

fn check_files(griz: &Griz, prefix: &str) -> TestResult {
    for index in 0..FILES {
        assert_eq!(
            griz.read(&format!("f{index:03}.txt"))?,
            format!("{prefix} {index}\n")
        );
    }
    Ok(())
}

fn check_stages(griz: &Griz) -> TestResult {
    for entry in fs::read_dir(griz.work.path())? {
        assert!(!entry?.file_name().to_string_lossy().contains(".griz-"));
    }
    Ok(())
}

fn check_mixed(griz: &Griz, text: &str, missing: usize) -> TestResult {
    for index in 0..FILES {
        let name = format!("f{index:03}.txt");
        if index % 3 == missing {
            assert!(!griz.path(&name).exists());
        } else {
            assert_eq!(griz.read(&name)?, text);
        }
    }
    Ok(())
}

#[test]
fn a_large_mixed_batch_preserves_creates_replacements_and_deletions() -> TestResult {
    let griz = Griz::new()?;
    let mut ops = Vec::new();
    for index in 0..FILES {
        let name = format!("f{index:03}.txt");
        if index % 3 != 0 {
            griz.write(&name, "before\n")?;
        }
        let op = match index % 3 {
            0 => json!({"op":"create","path":name,"text":"after\n"}),
            1 => json!({"op":"replace","path":name,"find":"before","replace":"after"}),
            _ => json!({"op":"delete","path":name}),
        };
        ops.push(op);
    }
    let plan = griz.id(&["plan", "--ops", &json!(ops).to_string()], "plan")?;
    let applied = griz.id(&["apply", &plan, "--expect-files", "64"], "apply")?;
    check_mixed(&griz, "after\n", 2)?;
    griz.id(&["undo", &applied], "undo")?;
    check_mixed(&griz, "before\n", 0)?;
    check_stages(&griz)
}

#[test]
fn large_batches_keep_every_file_and_journal_order() -> TestResult {
    let (griz, plan) = fixture()?;
    let operation = griz.id(&["apply", &plan, "--expect-files", "64"], "apply")?;
    check_files(&griz, "after")?;
    let record = griz.run(&["get", &operation])?;
    let files = record.json["files"].as_array().ok_or("missing files")?;
    assert_eq!(files.len(), FILES);
    for (index, file) in files.iter().enumerate() {
        let path = file["path"].as_str().ok_or("missing path")?;
        assert_eq!(
            std::path::Path::new(path).file_name(),
            Some(format!("f{index:03}.txt").as_ref())
        );
    }
    griz.id(&["undo", &operation], "undo")?;
    check_files(&griz, "before")?;
    check_stages(&griz)
}

#[test]
fn a_returned_commit_error_cleans_every_large_batch_stage() -> TestResult {
    let (griz, plan) = fixture()?;
    let output = griz
        .command(&[
            "apply",
            &plan,
            "--purpose",
            "test",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "before_commit")
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    check_files(&griz, "before")?;
    check_stages(&griz)?;
    let log = griz.run(&["log"])?;
    assert_eq!(log.json["operations"][0]["state"], "applied");
    check_files(&griz, "after")?;
    check_stages(&griz)
}

#[test]
fn interrupted_large_batch_recovers_the_remaining_files_in_order() -> TestResult {
    let (griz, plan) = fixture()?;
    let output = griz
        .command(&[
            "apply",
            &plan,
            "--purpose",
            "test",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "after_first_rename")
        .output()?;
    assert!(!output.status.success());
    assert_eq!(griz.read("f000.txt")?, "after 0\n");
    for index in 1..FILES {
        assert_eq!(
            griz.read(&format!("f{index:03}.txt"))?,
            format!("before {index}\n")
        );
    }
    let log = griz.run(&["log"])?;
    let id = log.json["operations"][0]["id"]
        .as_str()
        .ok_or("missing operation")?;
    assert_eq!(griz.run(&["get", id])?.json["recovered"], true);
    check_files(&griz, "after")
}

#[test]
fn a_staging_failure_waits_for_cleanup_and_writes_no_batch_file() -> TestResult {
    let (griz, plan) = fixture()?;
    let output = griz
        .command(&[
            "apply",
            &plan,
            "--purpose",
            "test",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "mid_stage_error")
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(body["code"], "STORE_ERROR");
    check_files(&griz, "before")?;
    check_stages(&griz)?;
    assert_eq!(griz.run(&["log"])?.json["operations"][0]["state"], "failed");
    griz.id(&["apply", &plan], "retry")?;
    check_files(&griz, "after")?;
    check_stages(&griz)
}
