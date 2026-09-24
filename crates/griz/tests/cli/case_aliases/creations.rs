//! Batch creation rejects case and Unicode aliases before source writes.
use super::fixture;
use crate::common::{Griz, Run, TestResult};
use serde_json::json;
use std::fs;

fn creation_pair(griz: &Griz, names: [&str; 2], aliases: bool, padding: usize) -> TestResult {
    let mut ops = vec![
        json!({"op":"replace","path":"file.txt","find":"one","replace":"ONE"}),
        json!({"op":"create","path":names[0],"text":"first\n"}),
    ];
    let extra: Vec<_> = (0..padding).map(|i| format!("batch-{i:02}.txt")).collect();
    ops.extend(
        extra
            .iter()
            .map(|path| json!({"op":"create","path":path,"text":"padding\n"})),
    );
    ops.push(json!({"op":"create","path":names[1],"text":"second\n"}));
    let plan = griz.id(&["plan", "--ops", &json!(ops).to_string()], "plan")?;
    let applied = griz.run(&[
        "apply",
        &plan,
        "--purpose",
        "collision test",
        "--idempotency-key",
        "apply",
    ])?;
    if aliases {
        assert_collision_refused(griz, &applied, names, &extra)?;
    } else {
        assert_distinct_created(griz, &applied, names)?;
    }
    for entry in fs::read_dir(griz.work.path())? {
        assert!(!entry?.file_name().to_string_lossy().contains(".griz-"));
    }
    Ok(())
}

#[test]
fn new_case_aliases_cannot_overwrite_each_other() -> TestResult {
    for padding in [0, 32] {
        let (griz, aliases) = fixture()?;
        creation_pair(&griz, ["new.txt", "NEW.txt"], aliases, padding)?;
    }
    Ok(())
}

#[test]
fn new_unicode_aliases_cannot_overwrite_each_other() -> TestResult {
    let (griz, _) = fixture()?;
    let names = ["caf\u{e9}.txt", "cafe\u{301}.txt"];
    griz.write(names[0], "probe")?;
    let aliases = griz.path(names[1]).exists();
    fs::remove_file(griz.path(names[0]))?;
    creation_pair(&griz, names, aliases, 0)
}

fn assert_collision_refused(
    griz: &Griz,
    applied: &Run,
    names: [&str; 2],
    extra: &[String],
) -> TestResult {
    assert_eq!(
        applied.code,
        Some(1),
        "padding={}: {}",
        extra.len(),
        applied.json
    );
    assert_eq!(griz.read("file.txt")?, "one two\n");
    for name in names.into_iter().chain(extra.iter().map(String::as_str)) {
        assert!(!griz.path(name).exists(), "{name} changed before rejection");
    }
    let log = griz.run(&["log"])?;
    assert_eq!(log.json["operations"][0]["state"], "failed");
    assert_eq!(griz.read("file.txt")?, "one two\n");
    Ok(())
}

fn assert_distinct_created(griz: &Griz, applied: &Run, names: [&str; 2]) -> TestResult {
    assert_eq!(applied.code, Some(0), "{}", applied.json);
    assert_eq!(griz.read(names[0])?, "first\n");
    assert_eq!(griz.read(names[1])?, "second\n");
    assert_eq!(griz.read("file.txt")?, "ONE two\n");
    let id = applied.json["id"].as_str().ok_or("no operation id")?;
    griz.id(&["undo", id], "undo")?;
    assert_eq!(griz.read("file.txt")?, "one two\n");
    assert!(!griz.path(names[0]).exists());
    assert!(!griz.path(names[1]).exists());
    Ok(())
}
