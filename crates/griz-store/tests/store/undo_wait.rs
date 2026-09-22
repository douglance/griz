//! Undo and span restore refresh absorbed metadata after waiting for locks.

use crate::common::{Fixture, TestResult, request, undo_request, wait_for_lock};
use griz_core::content_hash;
use griz_store::{OnStale, Operation, OperationState, Store, UndoRequest};
use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Duration,
};

const BEFORE: &str = "one\ntwo\nthree\n";
const FORMATTED: &str = "ONE\ntwo\nTHREE\n";

fn fixture(fx: &Fixture) -> Result<(String, Vec<PathBuf>), Box<dyn std::error::Error>> {
    let mut ops = Vec::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fx.write(name, BEFORE)?;
        ops.push(fx.replace(name, "one", "ONE"));
    }
    let applied = fx.store.apply(&request(&fx.plan(ops)?))?;
    let mut paths: Vec<_> = applied.files.iter().map(|file| file.path.clone()).collect();
    for path in &paths {
        std::fs::write(path, FORMATTED)?;
    }
    paths.sort_by_cached_key(|path| content_hash(&path.to_string_lossy()));
    Ok((applied.id, paths))
}

fn spawn_undo(
    home: PathBuf,
    id: String,
    path: PathBuf,
    span: bool,
    policy: OnStale,
) -> JoinHandle<Result<Operation, String>> {
    thread::spawn(move || {
        let store = Store::open(&home).map_err(|error| error.to_string())?;
        let result = if span {
            store.restore_since(&id, &[], policy, "undo span")
        } else {
            store.undo(&UndoRequest {
                paths: vec![path],
                on_stale: policy,
                ..undo_request(&id)
            })
        };
        result.map_err(|error| error.to_string())
    })
}

fn run_case(span: bool, policy: OnStale) -> TestResult {
    let fx = Fixture::new()?;
    let (id, paths) = fixture(&fx)?;
    let held = fx.store.lock_paths(&[paths[1].clone()])?;
    let home = fx.store.home().to_path_buf();
    let absorb_id = id.clone();
    let absorb_home = home.clone();
    let absorb = thread::spawn(move || {
        Store::open(&absorb_home)
            .and_then(|store| store.absorb(&absorb_id, &[], "formatter"))
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    wait_for_lock(&fx.store, &paths[0])?;
    let undo = spawn_undo(home, id, paths[0].clone(), span, policy);
    thread::sleep(Duration::from_millis(50));
    let completed_while_locked = undo.is_finished();
    drop(held);
    let absorbed = absorb.join().map_err(|_| "absorb panicked")?;
    let undone = undo.join().map_err(|_| "undo panicked")?;
    absorbed?;
    let undone = undone?;
    assert!(!completed_while_locked);
    assert_eq!(undone.state, OperationState::Applied, "{:?}", undone.reason);
    assert!(undone.conflicts.is_empty());
    assert_eq!(std::fs::read_to_string(&paths[0])?, BEFORE);
    let remaining = if span { BEFORE } else { FORMATTED };
    assert_eq!(std::fs::read_to_string(&paths[1])?, remaining);
    assert_eq!(std::fs::read_to_string(&paths[2])?, remaining);
    Ok(())
}

#[test]
fn waiting_undo_refuse_uses_completed_absorb() -> TestResult {
    run_case(false, OnStale::Refuse)
}

#[test]
fn waiting_undo_merge_restores_absorbed_changes_too() -> TestResult {
    run_case(false, OnStale::Merge)
}

#[test]
fn waiting_span_refuse_uses_completed_absorb() -> TestResult {
    run_case(true, OnStale::Refuse)
}

#[test]
fn waiting_span_merge_restores_absorbed_changes_too() -> TestResult {
    run_case(true, OnStale::Merge)
}

#[test]
fn waiting_span_does_not_include_operations_added_after_selection() -> TestResult {
    let fx = Fixture::new()?;
    let (id, paths) = fixture(&fx)?;
    fx.store.absorb(&id, &[], "formatter")?;
    let held = fx.store.lock_paths(&[paths[1].clone()])?;
    let undo = spawn_undo(
        fx.store.home().to_path_buf(),
        id.clone(),
        paths[0].clone(),
        true,
        OnStale::Refuse,
    );
    let added = (|| -> TestResult {
        wait_for_lock(&fx.store, &paths[0])?;
        fx.write("late.txt", "before\n")?;
        let plan = fx.plan(vec![fx.replace("late.txt", "before", "after")])?;
        let applied = fx.store.apply(&request(&plan))?;
        assert_eq!(applied.state, OperationState::Applied);
        Ok(())
    })();
    drop(held);
    let restored = undo.join().map_err(|_| "restore panicked")??;
    added?;
    assert_eq!(restored.state, OperationState::Applied);
    assert_eq!(
        restored.restores.ok_or("missing span")?.operations,
        vec![id]
    );
    assert_eq!(fx.read("late.txt")?, "after\n");
    assert_eq!(std::fs::read_to_string(&paths[0])?, BEFORE);
    Ok(())
}
