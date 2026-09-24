//! Concurrent absorbs preserve the complete operation's undo record.

use crate::common::{Fixture, TestResult, request, undo_request};
use griz_store::{OperationState, Store};
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
    thread::{self, JoinHandle},
    time::Duration,
};

fn worker(
    fx: &Fixture,
    id: &str,
    path: PathBuf,
    ready: Arc<Barrier>,
) -> JoinHandle<Result<Vec<PathBuf>, String>> {
    let home = fx.store.home().to_path_buf();
    let id = id.to_string();
    thread::spawn(move || {
        let store = Store::open(&home).map_err(|error| error.to_string());
        ready.wait();
        let store = store?;
        store
            .absorb(&id, &[path], "formatted")
            .map(|result| result.absorbed)
            .map_err(|error| error.to_string())
    })
}

fn fixture(fx: &Fixture, count: usize) -> Result<String, Box<dyn std::error::Error>> {
    let mut ops = Vec::new();
    for index in 0..count {
        let name = format!("{index}.txt");
        fx.write(&name, &format!("old {index}\n"))?;
        ops.push(fx.replace(&name, "old", "new"));
    }
    let plan = fx.plan(ops)?;
    let applied = fx.store.apply(&request(&plan))?;
    assert_eq!(applied.state, OperationState::Applied);
    for index in 0..count {
        fx.write(&format!("{index}.txt"), &format!("formatted {index}\n"))?;
    }
    Ok(applied.id)
}

#[test]
fn absorb_waits_for_unselected_files_in_the_same_operation() -> TestResult {
    let fx = Fixture::new()?;
    let id = fixture(&fx, 2)?;
    let held = fx.store.lock_paths(&[fx.path("0.txt")])?;
    let ready = Arc::new(Barrier::new(2));
    let handle = worker(&fx, &id, fx.path("1.txt"), Arc::clone(&ready));
    ready.wait();
    thread::sleep(Duration::from_millis(100));
    let completed_while_locked = handle.is_finished();
    drop(held);
    let absorbed = handle.join().map_err(|_| "absorb panicked")??;
    assert!(
        !completed_while_locked,
        "absorb did not lock the complete record"
    );
    assert_eq!(absorbed, vec![fx.path("1.txt")]);
    Ok(())
}

#[test]
fn concurrent_absorbs_preserve_every_update_and_keep_all_files_undoable() -> TestResult {
    let fx = Fixture::new()?;
    let count = 8;
    let id = fixture(&fx, count)?;
    let paths: Vec<_> = (0..count)
        .map(|index| fx.path(&format!("{index}.txt")))
        .collect();
    let held = fx.store.lock_paths(&paths)?;
    let ready = Arc::new(Barrier::new(count + 1));
    let handles: Vec<_> = paths
        .iter()
        .map(|path| worker(&fx, &id, path.clone(), Arc::clone(&ready)))
        .collect();
    ready.wait();
    thread::sleep(Duration::from_millis(50));
    drop(held);
    for (handle, path) in handles.into_iter().zip(&paths) {
        assert_eq!(
            handle.join().map_err(|_| "absorb panicked")??,
            vec![path.clone()]
        );
    }
    let mut absorbed = fx.store.operation(&id)?.absorbed;
    absorbed.sort();
    assert_eq!(absorbed, paths);
    let undone = fx.store.undo(&undo_request(fx.root(), &id))?;
    assert_eq!(undone.state, OperationState::Applied);
    assert!(undone.conflicts.is_empty());
    for index in 0..count {
        assert_eq!(fx.read(&format!("{index}.txt"))?, format!("old {index}\n"));
    }
    Ok(())
}
