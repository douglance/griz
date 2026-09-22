//! First-time store setup is safe for concurrent callers.

use crate::common::TestResult;
use griz_store::Store;
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
    thread::{self, JoinHandle},
};

fn opener(home: PathBuf, ready: Arc<Barrier>) -> JoinHandle<Result<(), String>> {
    thread::spawn(move || {
        let store = Store::open(&home).map_err(|error| error.to_string());
        ready.wait();
        let store = store?;
        store
            .operations(1, None)
            .map_err(|error| error.to_string())?;
        Ok(())
    })
}

#[test]
fn concurrent_openers_initialize_a_fresh_store() -> TestResult {
    for _ in 0..8 {
        let home = tempfile::tempdir()?;
        let ready = Arc::new(Barrier::new(32));
        let handles: Vec<_> = (0..32)
            .map(|_| opener(home.path().to_path_buf(), Arc::clone(&ready)))
            .collect();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "store opener panicked"))
            .collect::<Result<Vec<_>, _>>()?;
        results.into_iter().collect::<Result<Vec<_>, _>>()?;
        let connection = rusqlite::Connection::open(home.path().join("griz.sqlite3"))?;
        let integrity: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        assert_eq!(integrity, "ok");
        let mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!(mode, "wal");
    }
    Ok(())
}
fn check_setup_lock(home: &std::path::Path, should_wait: bool) -> TestResult {
    use std::{fs::File, sync::mpsc, time::Duration};

    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("griz.init.lock"))?;
    lock.lock()?;
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let path = home.to_path_buf();
    let handle = thread::spawn(move || {
        let _ = started_tx.send(());
        let result = Store::open(&path)
            .map(|_| ())
            .map_err(|error| error.to_string());
        let _ = done_tx.send(());
        result
    });
    started_rx.recv_timeout(Duration::from_secs(5))?;
    let completed = done_rx.recv_timeout(Duration::from_millis(200)).is_ok();
    drop(lock);
    handle.join().map_err(|_| "store opener panicked")??;
    assert_eq!(completed, !should_wait);
    Ok(())
}

#[test]
fn first_wal_setup_waits_for_the_initialization_lock() -> TestResult {
    let home = tempfile::tempdir()?;
    check_setup_lock(home.path(), true)
}

#[test]
fn initialized_store_does_not_take_the_initialization_lock() -> TestResult {
    let home = tempfile::tempdir()?;
    let store = Store::open(home.path())?;
    check_setup_lock(store.home(), false)
}
