//! Restore selection locks only files included by the caller.

use crate::common::{Fixture, TestResult, request};
use griz_store::{OnStale, OperationState, Store};
use std::{sync::mpsc, thread, time::Duration};

#[test]
fn narrowed_restore_does_not_wait_for_an_excluded_file_lock() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.txt", "before\n")?;
    fx.write("b.txt", "before\n")?;
    let plan = fx.plan(vec![
        fx.replace("a.txt", "before", "after"),
        fx.replace("b.txt", "before", "after"),
    ])?;
    let applied = fx.store.apply(&request(&plan))?;
    let held = fx.store.lock_paths(&[fx.path("b.txt")])?;
    let home = fx.store.home().to_path_buf();
    let selected = fx.path("a.txt");
    let (send, receive) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = Store::open(&home)
            .and_then(|store| {
                store.restore_since(&applied.id, &[selected], OnStale::Refuse, "selected")
            })
            .map_err(|error| error.to_string());
        send.send(result).map_err(|error| error.to_string())
    });
    let completed = receive.recv_timeout(Duration::from_secs(2));
    drop(held);
    worker.join().map_err(|_| "restore panicked")??;
    let restored = completed.map_err(|_| "restore waited for an excluded file lock")??;
    assert_eq!(restored.state, OperationState::Applied);
    assert_eq!(fx.read("a.txt")?, "before\n");
    assert_eq!(fx.read("b.txt")?, "after\n");
    Ok(())
}
