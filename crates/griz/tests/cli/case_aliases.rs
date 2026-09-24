//! Existing case aliases share a plan; separate directory entries stay separate.
use crate::common::{Griz, TestResult};
use serde_json::json;
use std::{error::Error, fs};

fn fixture() -> Result<(Griz, bool), Box<dyn Error>> {
    let griz = Griz::new()?;
    griz.write("file.txt", "one two\n")?;
    let aliases = griz.path("FILE.txt").exists();
    if let Ok(expected) = std::env::var("GRIZ_EXPECT_CASE_SENSITIVE") {
        assert_eq!(aliases, expected != "1");
    }
    if !aliases {
        griz.write("FILE.txt", "one two\n")?;
    }
    Ok((griz, aliases))
}

fn plan(griz: &Griz, aliases: bool) -> Result<String, Box<dyn Error>> {
    let ops = json!([
        {"op":"replace","path":"file.txt","find":"one","replace":"ONE"},
        {"op":"replace","path":"FILE.txt","find":"two","replace":"TWO"}
    ]);
    griz.id(
        &[
            "plan",
            "--ops",
            &ops.to_string(),
            "--expect-files",
            if aliases { "1" } else { "2" },
        ],
        "plan",
    )
}

#[test]
fn existing_case_aliases_compose_without_losing_an_edit() -> TestResult {
    let (griz, aliases) = fixture()?;
    let found = griz.run(&[
        "find",
        "--paths",
        "file.txt",
        "--paths",
        "FILE.txt",
        "--literal",
        "one",
    ])?;
    assert_eq!(found.json["total"], if aliases { 1 } else { 2 });
    let plan = plan(&griz, aliases)?;
    let applied = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(
        griz.read("file.txt")?,
        if aliases { "ONE TWO\n" } else { "ONE two\n" }
    );
    assert_eq!(
        griz.read("FILE.txt")?,
        if aliases { "ONE TWO\n" } else { "one TWO\n" }
    );
    griz.id(&["undo", &applied], "undo")?;
    assert_eq!(griz.read("file.txt")?, "one two\n");
    assert_eq!(griz.read("FILE.txt")?, "one two\n");
    Ok(())
}

#[test]
fn case_alias_reads_and_filters_use_the_existing_entry_name() -> TestResult {
    let (griz, aliases) = fixture()?;
    let spelling = if aliases { "file.txt" } else { "FILE.txt" };
    let expected = griz.work.path().canonicalize()?.join(spelling);
    let read = griz.run(&["read", "FILE.txt"])?;
    assert_eq!(read.json["path"], json!(expected));
    let found = griz.run(&["find", "--paths", "FILE.txt", "--literal", "one"])?;
    assert_eq!(found.json["matches"][0]["path"], json!(expected));
    let plan = plan(&griz, aliases)?;
    let selected = griz.id(&["select", &plan, "--paths", "FILE.txt"], "select")?;
    let applied = griz.id(&["apply", &selected], "apply")?;
    assert_eq!(
        griz.read("file.txt")?,
        if aliases { "ONE TWO\n" } else { "one two\n" }
    );
    let log = griz.run(&["log", "--paths", "FILE.txt"])?;
    assert_eq!(log.json["operations"][0]["id"], applied);
    griz.write(spelling, "formatted\n")?;
    griz.id(&["absorb", &applied, "--paths", "FILE.txt"], "absorb")?;
    griz.id(&["undo", &applied, "--paths", "FILE.txt"], "undo")?;
    assert_eq!(griz.read(spelling)?, "one two\n");
    Ok(())
}

#[test]
fn distinct_hard_link_entries_are_not_merged() -> TestResult {
    let griz = Griz::new()?;
    griz.write("first.txt", "one two\n")?;
    fs::hard_link(griz.path("first.txt"), griz.path("second.txt"))?;
    let ops = json!([
        {"op":"replace","path":"first.txt","find":"one","replace":"ONE"},
        {"op":"replace","path":"second.txt","find":"two","replace":"TWO"}
    ]);
    let plan = griz.id(
        &["plan", "--ops", &ops.to_string(), "--expect-files", "2"],
        "plan",
    )?;
    let applied = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("first.txt")?, "ONE two\n");
    assert_eq!(griz.read("second.txt")?, "one TWO\n");
    griz.id(&["undo", &applied], "undo")?;
    assert_eq!(griz.read("first.txt")?, "one two\n");
    assert_eq!(griz.read("second.txt")?, "one two\n");
    Ok(())
}

#[test]
fn created_unicode_name_can_be_undone_after_filesystem_normalization() -> TestResult {
    let griz = Griz::new()?;
    let ops = json!([{"op":"create","path":"caf\u{e9}.txt","text":"one\n"}]);
    let plan = griz.id(&["plan", "--ops", &ops.to_string()], "plan")?;
    let applied = griz.id(&["apply", &plan], "apply")?;
    assert_eq!(griz.read("caf\u{e9}.txt")?, "one\n");
    let undone = griz.id(&["undo", &applied], "undo")?;
    assert!(!griz.path("caf\u{e9}.txt").exists());
    griz.id(&["undo", &undone], "redo")?;
    assert_eq!(griz.read("caf\u{e9}.txt")?, "one\n");
    Ok(())
}

#[test]
fn existing_case_aliases_share_one_lock() -> TestResult {
    let (griz, aliases) = fixture()?;
    let store = griz.store()?;
    drop(store.lock_paths(&[griz.path("file.txt")])?);
    let held = store.lock_paths(&[griz.path("FILE.txt")])?;
    assert_eq!(
        fs::read_dir(store.home().join("locks"))?.count(),
        if aliases { 1 } else { 2 }
    );
    drop(held);
    Ok(())
}
#[cfg(unix)]
#[test]
fn file_link_targets_also_resolve_existing_case_aliases() -> TestResult {
    let (griz, aliases) = fixture()?;
    std::os::unix::fs::symlink("FILE.txt", griz.path("alias.txt"))?;
    let ops = json!([
        {"op":"replace","path":"file.txt","find":"one","replace":"ONE"},
        {"op":"replace","path":"alias.txt","find":"two","replace":"TWO"}
    ]);
    let plan = griz.id(
        &[
            "plan",
            "--ops",
            &ops.to_string(),
            "--expect-files",
            if aliases { "1" } else { "2" },
        ],
        "plan",
    )?;
    griz.id(&["apply", &plan], "apply")?;
    assert_eq!(
        griz.read("file.txt")?,
        if aliases { "ONE TWO\n" } else { "ONE two\n" }
    );
    assert_eq!(
        griz.read("alias.txt")?,
        if aliases { "ONE TWO\n" } else { "one TWO\n" }
    );
    assert!(griz.path("alias.txt").is_symlink());
    Ok(())
}

mod creations;
