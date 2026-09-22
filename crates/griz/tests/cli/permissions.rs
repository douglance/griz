//! Atomic replacements retain an existing file's Unix permission bits.
#![cfg(unix)]

use crate::common::{Griz, TestResult};
use serde_json::json;
use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn mode(path: &Path) -> Result<u32, Box<dyn Error>> {
    Ok(fs::metadata(path)?.permissions().mode() & 0o777)
}

fn script(griz: &Griz, name: &str, permissions: u32) -> TestResult {
    griz.write(name, "#!/bin/sh\necho before\n")?;
    fs::set_permissions(griz.path(name), fs::Permissions::from_mode(permissions))?;
    Ok(())
}

fn plan(griz: &Griz, names: &[&str]) -> Result<String, Box<dyn Error>> {
    let ops = names
        .iter()
        .map(|name| {
            json!({
                "op": "replace", "path": name, "find": "before", "replace": "after"
            })
        })
        .collect::<Vec<_>>();
    griz.id(&["plan", "--ops", &serde_json::to_string(&ops)?], "plan")
}

fn check_replacement(permissions: u32) -> TestResult {
    let griz = Griz::new()?;
    script(&griz, "a.sh", permissions)?;
    let planned = plan(&griz, &["a.sh"])?;
    let operation = griz.id(&["apply", &planned], "apply")?;
    assert_eq!(mode(&griz.path("a.sh"))?, permissions);
    assert_eq!(griz.read("a.sh")?, "#!/bin/sh\necho after\n");
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(mode(&griz.path("a.sh"))?, permissions);
    assert_eq!(griz.read("a.sh")?, "#!/bin/sh\necho before\n");
    Ok(())
}

#[test]
fn replacements_preserve_executable_restricted_and_readonly_modes() -> TestResult {
    for permissions in [0o751, 0o640, 0o440, 0o770] {
        check_replacement(permissions)?;
    }
    Ok(())
}

#[test]
fn an_edited_script_remains_directly_executable() -> TestResult {
    let griz = Griz::new()?;
    script(&griz, "a.sh", 0o751)?;
    let planned = plan(&griz, &["a.sh"])?;
    griz.id(&["apply", &planned], "apply")?;
    let result = Command::new(griz.path("a.sh")).output()?;
    assert!(result.status.success());
    assert_eq!(result.stdout, b"after\n");
    Ok(())
}

#[test]
fn undo_retains_permissions_changed_after_the_apply() -> TestResult {
    let griz = Griz::new()?;
    script(&griz, "a.sh", 0o751)?;
    let planned = plan(&griz, &["a.sh"])?;
    let operation = griz.id(&["apply", &planned], "apply")?;
    fs::set_permissions(griz.path("a.sh"), fs::Permissions::from_mode(0o600))?;
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(mode(&griz.path("a.sh"))?, 0o600);
    assert_eq!(griz.read("a.sh")?, "#!/bin/sh\necho before\n");
    Ok(())
}

fn recover_permissions(point: &str) -> TestResult {
    let griz = Griz::new()?;
    script(&griz, "a.sh", 0o751)?;
    script(&griz, "b.sh", 0o640)?;
    let planned = plan(&griz, &["a.sh", "b.sh"])?;
    let killed = griz
        .command(&[
            "apply",
            &planned,
            "--purpose",
            "test",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", point)
        .output()?;
    assert!(!killed.status.success());
    let log = griz.run(&["log"])?;
    assert_eq!(log.code, Some(0));
    assert_eq!(griz.read("a.sh")?, "#!/bin/sh\necho after\n");
    assert_eq!(griz.read("b.sh")?, "#!/bin/sh\necho after\n");
    assert_eq!(mode(&griz.path("a.sh"))?, 0o751);
    assert_eq!(mode(&griz.path("b.sh"))?, 0o640);
    let id = log.json["operations"][0]["id"]
        .as_str()
        .ok_or("no recovered operation")?;
    assert_eq!(griz.run(&["get", id])?.json["recovered"], true);
    Ok(())
}

#[test]
fn crash_recovery_preserves_permissions_before_and_between_renames() -> TestResult {
    recover_permissions("after_journal")?;
    recover_permissions("after_first_rename")
}

#[test]
fn staged_file_permissions_are_restricted_before_any_text_is_written() -> TestResult {
    let griz = Griz::new()?;
    script(&griz, "a.sh", 0o600)?;
    let planned = plan(&griz, &["a.sh"])?;
    let killed = griz
        .command(&[
            "apply",
            &planned,
            "--purpose",
            "test",
            "--idempotency-key",
            "apply",
        ])
        .env("GRIZ_FAILPOINT", "after_stage_create")
        .output()?;
    assert!(!killed.status.success());
    let staged = fs::read_dir(griz.work.path())?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".a.sh.griz-")
        })
        .ok_or("no staged source file")?;
    assert_eq!(fs::metadata(staged.path())?.len(), 0);
    assert_eq!(mode(&staged.path())? & !0o600, 0);
    assert_eq!(griz.read("a.sh")?, "#!/bin/sh\necho before\n");
    assert_eq!(mode(&griz.path("a.sh"))?, 0o600);
    Ok(())
}

#[test]
fn restoring_a_deleted_file_uses_creation_defaults() -> TestResult {
    let griz = Griz::new()?;
    griz.write("control", "created by the caller")?;
    let expected = mode(&griz.path("control"))?;
    script(&griz, "a.sh", 0o751)?;
    let ops = r#"[{"op":"delete","path":"a.sh"}]"#;
    let planned = griz.id(&["plan", "--ops", ops], "plan")?;
    let operation = griz.id(&["apply", &planned], "apply")?;
    assert!(!griz.path("a.sh").exists());
    griz.id(&["undo", &operation], "undo")?;
    assert_eq!(mode(&griz.path("a.sh"))?, expected);
    assert_eq!(griz.read("a.sh")?, "#!/bin/sh\necho before\n");
    Ok(())
}

#[test]
fn created_files_use_normal_creation_permissions() -> TestResult {
    let griz = Griz::new()?;
    griz.write("control", "created by the caller")?;
    let expected = mode(&griz.path("control"))?;
    let ops = r#"[{"op":"create","path":"new.txt","text":"new"}]"#;
    let planned = griz.id(&["plan", "--ops", ops], "plan")?;
    griz.id(&["apply", &planned], "apply")?;
    assert_eq!(mode(&griz.path("new.txt"))?, expected);
    Ok(())
}
