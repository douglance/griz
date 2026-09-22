//! Directory aliases share planned destinations without changing retry identity.
#![cfg(unix)]

use crate::common::{Griz, TestResult};
use serde_json::json;
use std::{error::Error, fs, os::unix::fs::symlink};

fn fixture() -> Result<Griz, Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write("actual/target.txt", "one two\n")?;
    symlink(griz.path("actual"), griz.path("link"))?;
    Ok(griz)
}

fn plan(griz: &Griz) -> Result<String, Box<dyn Error>> {
    let ops = json!([
        {"op":"replace","path":"actual/target.txt","find":"one","replace":"ONE"},
        {"op":"replace","path":"link/target.txt","find":"two","replace":"TWO"}
    ]);
    griz.id(
        &["plan", "--ops", &ops.to_string(), "--expect-files", "1"],
        "plan",
    )
}

#[test]
fn edits_through_directory_aliases_compose_in_one_file() -> TestResult {
    let griz = fixture()?;
    let plan = plan(&griz)?;
    let operation = griz.id(&["apply", &plan, "--expect-files", "1"], "apply")?;
    assert_eq!(griz.read("actual/target.txt")?, "ONE TWO\n");
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(griz.read("actual/target.txt")?, "one two\n");
    assert!(griz.path("link").is_symlink());
    Ok(())
}

#[test]
fn directory_retargeting_does_not_redirect_a_saved_plan() -> TestResult {
    let griz = fixture()?;
    let ops = json!([{"op":"replace","path":"link/target.txt","find":"one","replace":"ONE"}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    griz.write("other/target.txt", "one two\n")?;
    fs::remove_file(griz.path("link"))?;
    symlink(griz.path("other"), griz.path("link"))?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("actual/target.txt")?, "ONE two\n");
    assert_eq!(griz.read("other/target.txt")?, "one two\n");
    Ok(())
}

#[test]
fn aliases_in_reads_filters_and_roots_name_the_same_file() -> TestResult {
    let griz = fixture()?;
    let canonical = griz.path("actual/target.txt").canonicalize()?;
    let read = griz.run(&["read", "link/target.txt"])?;
    assert_eq!(read.json["path"], json!(canonical));
    let found = griz.run(&["find", "--paths", "link/target.txt", "--literal", "one"])?;
    assert_eq!(found.json["matches"][0]["path"], json!(canonical));
    let plan = plan(&griz)?;
    let selected = griz.id(&["select", &plan, "--paths", "link/target.txt"], "select")?;
    let operation = griz.id(&["apply", &selected], "apply")?;
    for args in [
        vec!["log", "--paths", "link/target.txt"],
        vec!["log", "--root", "link"],
    ] {
        let log = griz.run(&args)?;
        assert_eq!(log.json["operations"][0]["id"], operation);
    }
    griz.write("actual/target.txt", "formatted\n")?;
    griz.id(
        &["absorb", &operation, "--paths", "link/target.txt"],
        "absorb",
    )?;
    griz.id(&["undo", &operation, "--paths", "link/target.txt"], "undo")?;
    assert_eq!(griz.read("actual/target.txt")?, "one two\n");
    Ok(())
}

#[test]
fn creating_missing_directories_resolves_the_existing_alias_prefix() -> TestResult {
    let griz = fixture()?;
    let ops = json!([
        {"op":"create","path":"link/new/deep/file.txt","text":"before\n"},
        {"op":"replace","path":"actual/new/deep/file.txt","find":"before","replace":"after"}
    ]);
    let plan = griz.id(
        &["plan", "--ops", &ops.to_string(), "--expect-files", "1"],
        "plan",
    )?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("actual/new/deep/file.txt")?, "after\n");
    Ok(())
}

#[test]
fn alias_receipts_replay_after_the_link_is_removed() -> TestResult {
    let griz = fixture()?;
    let plan_id = plan(&griz)?;
    let selected = griz.id(
        &["select", &plan_id, "--paths", "link/target.txt"],
        "select",
    )?;
    let operation = griz.id(&["apply", &selected], "apply")?;
    griz.id(
        &["absorb", &operation, "--paths", "link/target.txt"],
        "absorb",
    )?;
    let undone = griz.id(&["undo", &operation, "--paths", "link/target.txt"], "undo")?;
    fs::remove_file(griz.path("link"))?;
    assert_eq!(plan(&griz)?, plan_id);
    for (command, target, expected) in [
        ("select", plan_id.as_str(), selected.as_str()),
        ("absorb", operation.as_str(), operation.as_str()),
        ("undo", operation.as_str(), undone.as_str()),
    ] {
        assert_eq!(
            griz.id(&[command, target, "--paths", "link/target.txt"], command)?,
            expected
        );
    }
    Ok(())
}

fn legacy_plan(griz: &Griz) -> Result<String, Box<dyn Error>> {
    let ops = serde_json::from_value::<Vec<griz_core::Op>>(json!([{
        "op":"replace","path":griz.work.path().canonicalize()?.join("link/target.txt"),
        "find":{"text":"one"},"replace":"ONE"
    }]))?;
    let plan = griz_core::build_plan(&ops, &griz_core::DiskSource);
    Ok(griz
        .store()?
        .save_plan("legacy alias fixture", ops, plan)?
        .id)
}

#[test]
fn saved_alias_filters_still_select_absorb_and_undo() -> TestResult {
    let griz = fixture()?;
    let plan = legacy_plan(&griz)?;
    let selected = griz.id(&["select", &plan, "--paths", "link/target.txt"], "select")?;
    let applied = griz.id(&["apply", &selected, "--expect-files", "1"], "apply")?;
    assert_eq!(griz.read("actual/target.txt")?, "ONE two\n");
    let log = griz.run(&["log", "--paths", "link/target.txt"])?;
    assert_eq!(log.json["operations"][0]["id"], applied);
    griz.write("actual/target.txt", "formatted\n")?;
    griz.id(
        &["absorb", &applied, "--paths", "link/target.txt"],
        "absorb",
    )?;
    griz.id(&["undo", &applied, "--paths", "link/target.txt"], "undo")?;
    assert_eq!(griz.read("actual/target.txt")?, "one two\n");
    Ok(())
}

#[test]
fn saved_alias_filters_still_restore_a_span() -> TestResult {
    let griz = fixture()?;
    let applied = griz.id(&["apply", &legacy_plan(&griz)?], "apply")?;
    griz.id(
        &["undo", "--since", &applied, "--paths", "link/target.txt"],
        "undo",
    )?;
    assert_eq!(griz.read("actual/target.txt")?, "one two\n");
    Ok(())
}

#[test]
fn a_dangling_directory_link_is_not_a_new_directory() -> TestResult {
    let griz = Griz::new()?;
    symlink(griz.path("missing"), griz.path("link"))?;
    let ops = json!([{"op":"create","path":"link/file.txt","text":"wrong"}]);
    let run = griz.run(&[
        "plan",
        "--ops",
        &ops.to_string(),
        "--purpose",
        "test",
        "--idempotency-key",
        "plan",
    ])?;
    assert_eq!(run.code, Some(1));
    assert_eq!(run.json["code"], "VALIDATION_ERROR");
    assert!(!griz.path("missing").exists());
    Ok(())
}
