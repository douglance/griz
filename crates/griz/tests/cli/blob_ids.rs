//! Malformed blob IDs are validation errors, not panics or filesystem paths.

use crate::common::{Griz, TestResult};
use serde_json::Value;

#[test]
fn get_rejects_malformed_blob_ids_without_panicking() -> TestResult {
    let griz = Griz::new()?;
    griz.write("sentinel.txt", "outside blob store\n")?;
    let ids = [
        "blob_".to_string(),
        "blob_😀".to_string(),
        format!("blob_aa{}", griz.path("sentinel.txt").display()),
        format!("blob_{}", "G".repeat(64)),
    ];
    for id in ids {
        let output = griz.command(&["get", &id]).output()?;
        assert_eq!(output.status.code(), Some(1));
        let response: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(response["code"], "VALIDATION_ERROR", "{response}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
    Ok(())
}
