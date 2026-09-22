//! Parent traversal resolves directory links before removing a component.
#![cfg(unix)]

use crate::common::{Griz, TestResult};
use serde_json::json;
use std::{error::Error, fs, os::unix::fs::symlink};

const REQUEST: &str = "link/../target.txt";
const RIGHT: &str = "anchor\nright neighbor\n";
const WRONG: &str = "anchor\nwrong neighbor\n";

fn fixture() -> Result<Griz, Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write("actual/deep/keep", "")?;
    griz.write("actual/target.txt", RIGHT)?;
    griz.write("target.txt", WRONG)?;
    symlink(griz.path("actual/deep"), griz.path("link"))?;
    Ok(griz)
}

fn typed_plan(griz: &Griz) -> Result<String, Box<dyn Error>> {
    let ops =
        json!([{"op":"replace","path":REQUEST,"find":"anchor","replace":"changed"}]).to_string();
    griz.id(&["plan", "--ops", &ops, "--expect-files", "1"], "plan")
}

fn check_files(griz: &Griz, expected: &str) -> TestResult {
    assert_eq!(griz.read("actual/target.txt")?, expected);
    assert_eq!(griz.read("target.txt")?, WRONG);
    Ok(())
}

#[test]
fn plan_retries_do_not_resolve_removed_parent_links_again() -> TestResult {
    for explicit_root in [false, true] {
        let griz = fixture()?;
        let path = if explicit_root { "target.txt" } else { REQUEST };
        let ops =
            json!([{"op":"replace","path":path,"find":"anchor","replace":"changed"}]).to_string();
        let mut args = vec!["plan", "--ops", &ops];
        if explicit_root {
            args.extend(["--root", "link/.."]);
        }
        let original = griz.id(&args, "plan")?;
        fs::remove_file(griz.path("link"))?;
        assert_eq!(griz.id(&args, "plan")?, original);
        check_files(&griz, RIGHT)?;
    }
    Ok(())
}

#[test]
fn filtered_mutation_retries_do_not_resolve_removed_parent_links_again() -> TestResult {
    let griz = fixture()?;
    let plan = typed_plan(&griz)?;
    let selected = griz.id(&["select", &plan, "--paths", REQUEST], "select")?;
    let operation = griz.id(&["apply", &selected], "apply")?;
    griz.write("actual/target.txt", "formatted\nright neighbor\n")?;
    griz.id(&["absorb", &operation, "--paths", REQUEST], "absorb")?;
    let undone = griz.id(&["undo", &operation, "--paths", REQUEST], "undo")?;
    fs::remove_file(griz.path("link"))?;
    for (command, target, expected) in [
        ("select", plan.as_str(), selected.as_str()),
        ("absorb", operation.as_str(), operation.as_str()),
        ("undo", operation.as_str(), undone.as_str()),
    ] {
        assert_eq!(
            griz.id(&[command, target, "--paths", REQUEST], command)?,
            expected
        );
    }
    check_files(&griz, RIGHT)
}

#[test]
fn parent_path_and_lexical_neighbor_are_different_requests() -> TestResult {
    let griz = fixture()?;
    typed_plan(&griz)?;
    let ops = json!([{"op":"replace","path":"target.txt","find":"anchor","replace":"changed"}])
        .to_string();
    let other = griz.run(&[
        "plan",
        "--ops",
        &ops,
        "--expect-files",
        "1",
        "--purpose",
        "test",
        "--idempotency-key",
        "plan",
    ])?;
    assert_eq!(other.json["code"], "IDEMPOTENCY_CONFLICT");
    check_files(&griz, RIGHT)
}

#[test]
fn read_and_find_follow_filesystem_parent_traversal() -> TestResult {
    let griz = fixture()?;
    let read = griz.run(&["read", REQUEST])?;
    assert_eq!(read.code, Some(0));
    assert_eq!(read.json["lines"][1]["text"], "right neighbor");
    assert_eq!(read.json["hash"], griz_core::content_hash(RIGHT));
    let actual = griz.path("actual/target.txt").canonicalize()?;
    assert_eq!(read.json["path"], json!(actual));
    let found = griz.run(&[
        "find",
        "--paths",
        REQUEST,
        "--literal",
        "right",
        "--expect-matches",
        "1",
    ])?;
    assert_eq!(found.code, Some(0), "{}", found.json);
    assert_eq!(found.json["matches"][0]["path"], read.json["path"]);
    Ok(())
}

#[test]
fn an_explicit_root_resolves_links_before_its_parent() -> TestResult {
    let griz = fixture()?;
    let read = griz.run(&["read", "target.txt", "--root", "link/.."])?;
    assert_eq!(read.code, Some(0));
    assert_eq!(read.json["lines"][1]["text"], "right neighbor");
    Ok(())
}

#[test]
fn typed_edits_and_undo_touch_only_the_requested_file() -> TestResult {
    let griz = fixture()?;
    let plan = typed_plan(&griz)?;
    let operation = griz.id(&["apply", &plan], "apply")?;
    check_files(&griz, "changed\nright neighbor\n")?;
    griz.id(&["undo", &operation, "--paths", REQUEST], "undo")?;
    check_files(&griz, RIGHT)
}

#[test]
fn selection_absorb_and_history_use_the_same_resolved_path() -> TestResult {
    let griz = fixture()?;
    let plan = typed_plan(&griz)?;
    let selected = griz.id(&["select", &plan, "--paths", REQUEST], "select")?;
    let operation = griz.id(&["apply", &selected, "--expect-files", "1"], "apply")?;
    griz.write("actual/target.txt", "formatted\nright neighbor\n")?;
    griz.id(&["absorb", &operation, "--paths", REQUEST], "absorb")?;
    let log = griz.run(&["log", "--paths", REQUEST])?;
    assert_eq!(log.json["operations"][0]["id"], operation);
    griz.id(&["undo", &operation], "undo")?;
    check_files(&griz, RIGHT)
}

#[test]
fn patch_targets_follow_directory_links_before_parent_traversal() -> TestResult {
    let griz = fixture()?;
    let patch = "*** Begin Patch\n*** Update File: link/../target.txt\n@@\n-anchor\n+changed\n*** End Patch\n";
    let plan = griz.id(&["plan", "--patch", patch], "plan")?;
    griz.id(&["apply", &plan], "apply")?;
    check_files(&griz, "changed\nright neighbor\n")
}

#[test]
fn input_files_follow_directory_links_before_parent_traversal() -> TestResult {
    let griz = fixture()?;
    let correct =
        json!([{"op":"replace","path":"actual/target.txt","find":"anchor","replace":"chosen"}]);
    let wrong = json!([{"op":"replace","path":"target.txt","find":"anchor","replace":"wrong"}]);
    griz.write("actual/ops.json", &correct.to_string())?;
    griz.write("ops.json", &wrong.to_string())?;
    let plan = griz.id(&["plan", "--ops", "@link/../ops.json"], "plan")?;
    griz.id(&["apply", &plan], "apply")?;
    check_files(&griz, "chosen\nright neighbor\n")
}

#[test]
fn missing_and_file_prefixes_cannot_be_cancelled_by_parent_traversal() -> TestResult {
    let griz = fixture()?;
    for path in ["missing/../target.txt", "target.txt/../target.txt"] {
        let read = griz.run(&["read", path])?;
        assert_eq!(read.code, Some(1));
        assert_eq!(read.json["code"], "VALIDATION_ERROR");
        let ops =
            json!([{"op":"replace","path":path,"find":"anchor","replace":"wrong"}]).to_string();
        let plan = griz.run(&[
            "plan",
            "--ops",
            &ops,
            "--purpose",
            "test",
            "--idempotency-key",
            path,
        ])?;
        assert_eq!(plan.code, Some(1));
        assert_eq!(plan.json["code"], "VALIDATION_ERROR");
    }
    check_files(&griz, RIGHT)
}

#[test]
fn workspace_parent_paths_stay_bound_to_the_planned_directory() -> TestResult {
    let griz = fixture()?;
    let uri = format!("file://{}", griz.path(REQUEST).display());
    let edit = json!({"changes":{uri:[{
        "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":6}},
        "newText":"changed"
    }]}});
    let plan = griz.id(&["plan", "--workspace-edit", &edit.to_string()], "plan")?;
    griz.write("other/deep/keep", "")?;
    griz.write("other/target.txt", RIGHT)?;
    fs::remove_file(griz.path("link"))?;
    symlink(griz.path("other/deep"), griz.path("link"))?;
    griz.id(&["apply", &plan], "apply")?;
    check_files(&griz, "changed\nright neighbor\n")?;
    assert_eq!(griz.read("other/target.txt")?, RIGHT);
    Ok(())
}
