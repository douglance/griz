//! Executes the shipped Python CLI example against the test binary.

use crate::common::TestResult;
use std::{path::Path, process::Command};

#[test]
fn guarded_cli_example() -> TestResult {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli/guarded_cli_test.py");
    let output = Command::new("python3")
        .arg("-B")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_griz"))
        .output()?;
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
