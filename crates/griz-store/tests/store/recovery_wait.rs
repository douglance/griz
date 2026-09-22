//! A waiting recovery must not replay an operation another writer completed.

use crate::common::{Fixture, TestResult, request, undo_request};
use griz_core::content_hash;
use griz_store::{Operation, OperationState, Store, UndoRequest};
use std::{
    fs::{File, TryLockError},
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn pending(fx: &Fixture) -> Result<Operation, Box<dyn std::error::Error>> {
    let mut ops = Vec::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fx.write(name, "before\n")?;
        ops.push(fx.replace(name, "before", "after"));
    }
    let plan = fx.plan(ops)?;
    let applied = fx.store.apply(&request(&plan))?;
    assert_eq!(applied.state, OperationState::Applied);
    let mut applying = applied.clone();
    applying.state = OperationState::Applying;
    fx.store.save_operation(&applying)?;
    Ok(applied)
}

fn wait_for_lock(store: &Store, path: &Path) -> TestResult {
    let hash = content_hash(&path.to_string_lossy());
    let file = File::options()
        .write(true)
        .open(store.home().join("locks").join(format!("{hash}.lock")))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match file.try_lock() {
            Ok(()) => file.unlock()?,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err("recovery did not acquire the leading lock".into())
}

#[test]
fn waiting_recovery_preserves_a_later_undo_of_the_completed_operation() -> TestResult {
    let fx = Fixture::new()?;
    let applied = pending(&fx)?;
    let mut paths: Vec<_> = applied.files.iter().map(|file| file.path.clone()).collect();
    paths.sort_by_cached_key(|path| content_hash(&path.to_string_lossy()));
    let held = fx.store.lock_paths(&[paths[1].clone()])?;
    let home = fx.store.home().to_path_buf();
    let recovery = thread::spawn(move || {
        Store::open(&home)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    let during = (|| -> TestResult {
        wait_for_lock(&fx.store, &paths[0])?;
        fx.store.save_operation(&applied)?;
        let undone = fx.store.undo(&UndoRequest {
            paths: vec![paths[2].clone()],
            ..undo_request(&applied.id)
        })?;
        assert_eq!(undone.state, OperationState::Applied);
        assert_eq!(std::fs::read_to_string(&paths[2])?, "before\n");
        Ok(())
    })();
    drop(held);
    recovery.join().map_err(|_| "recovery panicked")??;
    during?;
    assert_eq!(std::fs::read_to_string(&paths[2])?, "before\n");
    assert_eq!(fx.store.operation(&applied.id)?, applied);
    Ok(())
}
