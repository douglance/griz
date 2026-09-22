//! Journal failures release staged files before returning.

use crate::common::{Fixture, TestResult, request};
use griz_store::{OperationState, StoreError};
use std::fs;

#[test]
fn failed_journal_insert_cleans_stages_without_changing_sources() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "before\n")?;
    fx.write("b.txt", "before\n")?;
    fx.write(".other.griz-keep.tmp", "unrelated")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "before", "after"),
        fx.replace("b.txt", "before", "after"),
    ])?;
    let database = rusqlite::Connection::open(fx.store.home().join("griz.sqlite3"))?;
    database.execute_batch("CREATE TRIGGER reject_operation BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT, 'journal failure'); END;")?;
    assert!(matches!(
        fx.store.apply(&request(&plan)),
        Err(StoreError::Sqlite(_))
    ));
    assert_eq!(fx.read("a.txt")?, "before\n");
    assert_eq!(fx.read("b.txt")?, "before\n");
    check_stages(&fx)?;
    let count: i64 = database.query_row("SELECT count(*) FROM operations", [], |row| row.get(0))?;
    assert_eq!(count, 0);
    database.execute_batch("DROP TRIGGER reject_operation")?;
    assert_eq!(
        fx.store.apply(&request(&plan))?.state,
        OperationState::Applied
    );
    assert_eq!(fx.read("a.txt")?, "after\n");
    check_stages(&fx)
}

fn check_stages(fx: &Fixture) -> TestResult {
    let files = fs::read_dir(fx.work.path())?.collect::<Result<Vec<_>, _>>()?;
    let staged: Vec<_> = files
        .iter()
        .filter(|entry| {
            entry.file_name().to_string_lossy().contains(".griz-")
                && entry.file_name() != ".other.griz-keep.tmp"
        })
        .collect();
    assert_eq!(staged.len(), 0, "owned stages remain");
    assert_eq!(fx.read(".other.griz-keep.tmp")?, "unrelated");
    Ok(())
}
