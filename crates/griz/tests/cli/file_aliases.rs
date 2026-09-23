//! File aliases identify their target before a plan is stored.
#![cfg(unix)]

use crate::common::{Griz, TestResult};
use serde_json::json;
use std::{error::Error, fs, os::unix::fs::symlink};

fn fixture() -> Result<Griz, Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write("target.txt", "one two\n")?;
    symlink("target.txt", griz.path("alias.txt"))?;
    Ok(griz)
}

fn plan(griz: &Griz) -> Result<String, Box<dyn Error>> {
    let ops = json!([{
        "op":"replace", "path":"alias.txt", "find":"one", "replace":"ONE"
    }]);
    griz.id(&["plan", "--ops", &ops.to_string()], "plan")
}

#[test]
fn file_alias_apply_and_undo_preserve_the_link() -> TestResult {
    let griz = fixture()?;
    let plan = plan(&griz)?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("target.txt")?, "ONE two\n");
    assert!(griz.path("alias.txt").is_symlink());
    assert_eq!(
        fs::read_link(griz.path("alias.txt"))?,
        std::path::Path::new("target.txt")
    );
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(griz.read("target.txt")?, "one two\n");
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

#[test]
fn file_alias_and_target_edits_compose_in_one_file() -> TestResult {
    let griz = fixture()?;
    let ops = json!([
        {"op":"replace","path":"target.txt","find":"one","replace":"ONE"},
        {"op":"replace","path":"alias.txt","find":"two","replace":"TWO"}
    ]);
    let plan = griz.id(
        &[
            "plan",
            "--ops",
            &ops.to_string(),
            "--expect-files",
            "1",
            "--expect-edits",
            "2",
        ],
        "plan",
    )?;
    griz.id(&["apply", &plan, "--expect-files", "1"], "apply")?;
    assert_eq!(griz.read("target.txt")?, "ONE TWO\n");
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

#[test]
fn file_alias_reads_and_filters_use_the_recorded_target() -> TestResult {
    let griz = fixture()?;
    let canonical = griz.path("target.txt").canonicalize()?;
    let read = griz.run(&["read", "alias.txt"])?;
    assert_eq!(read.json["path"], json!(canonical));
    let found = griz.run(&["find", "--paths", "alias.txt", "--literal", "one"])?;
    assert_eq!(found.json["matches"][0]["path"], json!(canonical));
    let plan = plan(&griz)?;
    let selected = griz.id(&["select", &plan, "--paths", "alias.txt"], "select")?;
    let operation = griz.id(&["apply", &selected], "apply")?;
    let log = griz.run(&["log", "--paths", "alias.txt"])?;
    assert_eq!(log.json["operations"][0]["id"], operation);
    griz.write("target.txt", "formatted\n")?;
    griz.id(&["absorb", &operation, "--paths", "alias.txt"], "absorb")?;
    griz.id(&["undo", &operation, "--paths", "alias.txt"], "undo")?;
    assert_eq!(griz.read("target.txt")?, "one two\n");
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

#[test]
fn file_alias_retargeting_does_not_redirect_a_saved_plan() -> TestResult {
    let griz = fixture()?;
    let plan = plan(&griz)?;
    griz.write("other.txt", "one two\n")?;
    fs::remove_file(griz.path("alias.txt"))?;
    symlink("other.txt", griz.path("alias.txt"))?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("target.txt")?, "ONE two\n");
    assert_eq!(griz.read("other.txt")?, "one two\n");
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(griz.read("target.txt")?, "one two\n");
    assert_eq!(
        fs::read_link(griz.path("alias.txt"))?,
        std::path::Path::new("other.txt")
    );
    Ok(())
}

#[test]
fn whole_file_replacement_through_a_link_preserves_it() -> TestResult {
    let griz = fixture()?;
    let ops = json!([{"op":"create","path":"alias.txt","text":"new\n","overwrite":true}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("target.txt")?, "new\n");
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

#[test]
fn chained_file_aliases_resolve_to_the_same_target() -> TestResult {
    let griz = fixture()?;
    symlink("alias.txt", griz.path("chain.txt"))?;
    let ops = json!([{"op":"insert","path":"chain.txt","anchor":"one","text":"more "}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("target.txt")?, "more one two\n");
    assert!(griz.path("alias.txt").is_symlink());
    assert!(griz.path("chain.txt").is_symlink());
    Ok(())
}

#[test]
fn retry_replays_after_the_file_alias_is_removed() -> TestResult {
    let griz = fixture()?;
    let first = plan(&griz)?;
    fs::remove_file(griz.path("alias.txt"))?;
    assert_eq!(plan(&griz)?, first);
    griz.id(&["apply", &first], "apply")?;
    assert_eq!(griz.read("target.txt")?, "ONE two\n");
    Ok(())
}

#[test]
fn creating_through_a_dangling_file_alias_preserves_it() -> TestResult {
    let griz = Griz::new()?;
    symlink("missing.txt", griz.path("alias.txt"))?;
    let ops = json!([{"op":"create","path":"alias.txt","text":"new\n"}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("missing.txt")?, "new\n");
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

#[test]
fn file_alias_cycles_are_errors_without_changing_links() -> TestResult {
    let griz = Griz::new()?;
    symlink("b.txt", griz.path("a.txt"))?;
    symlink("a.txt", griz.path("b.txt"))?;
    let result = griz.run(&["read", "a.txt"])?;
    assert_ne!(result.code, Some(0));
    assert_eq!(result.json["code"], "VALIDATION_ERROR");
    assert!(griz.path("a.txt").is_symlink());
    assert!(griz.path("b.txt").is_symlink());
    Ok(())
}

#[test]
fn a_new_symlink_at_a_recorded_target_is_refused() -> TestResult {
    let griz = fixture()?;
    let plan = plan(&griz)?;
    griz.write("other.txt", "one two\n")?;
    fs::remove_file(griz.path("target.txt"))?;
    symlink("other.txt", griz.path("target.txt"))?;
    let result = griz.run(&[
        "apply",
        &plan,
        "--purpose",
        "test",
        "--idempotency-key",
        "apply",
    ])?;
    assert_ne!(result.code, Some(0));
    assert!(griz.path("target.txt").is_symlink());
    assert_eq!(griz.read("other.txt")?, "one two\n");
    Ok(())
}

#[test]
fn deleting_an_aliased_file_keeps_the_link_and_undo_restores_its_target() -> TestResult {
    let griz = fixture()?;
    let ops = json!([{"op":"delete","path":"alias.txt"}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    assert!(!griz.path("target.txt").exists());
    assert!(griz.path("alias.txt").is_symlink());
    griz.id(&["undo", &operation, "--paths", "alias.txt"], "undo")?;
    assert_eq!(griz.read("target.txt")?, "one two\n");
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

#[test]
fn moving_an_aliased_file_keeps_the_link_and_undo_restores_its_target() -> TestResult {
    let griz = fixture()?;
    let ops = json!([{"op":"move","path":"alias.txt","to":"moved.txt"}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    assert!(!griz.path("target.txt").exists());
    assert!(griz.path("alias.txt").is_symlink());
    assert_eq!(griz.read("moved.txt")?, "one two\n");
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(griz.read("target.txt")?, "one two\n");
    assert!(!griz.path("moved.txt").exists());
    Ok(())
}
