//! A waiting recovery must not replay an operation another writer completed.

use crate::common::{Fixture, TestResult, in_lock_order, request, undo_request, wait_for_lock};
use griz_store::{Operation, OperationState, Store, UndoRequest};
use std::thread;

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

#[test]
fn waiting_recovery_preserves_a_later_undo_of_the_completed_operation() -> TestResult {
    let fx = Fixture::new()?;
    let applied = pending(&fx)?;
    let paths: Vec<_> = applied.files.iter().map(|file| file.path.clone()).collect();
    let paths = in_lock_order(paths)?;
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
            ..undo_request(fx.root(), &applied.id)
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
