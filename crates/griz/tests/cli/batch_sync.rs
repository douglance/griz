//! Flush failures must leave every source unchanged and every stage cleaned.
use crate::{
    common::TestResult,
    parallel_stage::{check_files, check_stages, fixture},
};

#[test]
fn full_flush_errors_leave_no_source_changed_or_applying_record() -> TestResult {
    for failpoint in ["before_batch_full_sync", "after_batch_full_sync"] {
        let (griz, plan) = fixture()?;
        let output = griz
            .command(&[
                "apply",
                &plan,
                "--purpose",
                "test",
                "--idempotency-key",
                "failed-flush",
            ])
            .env("GRIZ_FAILPOINT", failpoint)
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        check_files(&griz, "before")?;
        check_stages(&griz)?;
        let log = griz.run(&["log"])?;
        assert_eq!(log.json["operations"][0]["state"], "failed");
        griz.id(&["apply", &plan], "retry-flush")?;
        check_files(&griz, "after")?;
        check_stages(&griz)?;
    }
    Ok(())
}

#[test]
fn a_first_group_flush_error_publishes_no_plan_or_blob_name() -> TestResult {
    for failpoint in ["before_batch_full_sync", "after_batch_full_sync"] {
        let griz = crate::common::Griz::new()?;
        let ops: Vec<_> = (0..64)
            .map(|index| {
                serde_json::json!({
                    "op": "create", "path": format!("f{index:03}.txt"),
                    "text": format!("content {index}\n")
                })
            })
            .collect();
        let output = griz
            .command(&[
                "plan",
                "--ops",
                &serde_json::to_string(&ops)?,
                "--purpose",
                "test",
                "--idempotency-key",
                "failed-plan-flush",
            ])
            .env("GRIZ_FAILPOINT", failpoint)
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        let store = griz.store()?;
        let database = rusqlite::Connection::open(store.home().join("griz.sqlite3"))?;
        let count: i64 = database.query_row("SELECT count(*) FROM plans", [], |row| row.get(0))?;
        assert_eq!(count, 0);
        check_empty_blobs(&store.home().join("blobs"))?;
        assert!(!griz.path("f000.txt").exists());
        griz.id(
            &["plan", "--ops", &serde_json::to_string(&ops)?],
            "retry-plan-flush",
        )?;
    }
    Ok(())
}

fn check_empty_blobs(path: &std::path::Path) -> TestResult {
    if !path.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        assert!(
            entry.file_type()?.is_dir(),
            "published blob or owned stage remains"
        );
        check_empty_blobs(&entry.path())?;
    }
    Ok(())
}
