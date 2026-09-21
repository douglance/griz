//! Finishing interrupted operations, and serializing concurrent ones.

use crate::common::{Fixture, TestResult, request};
use griz_core::content_hash;
use griz_store::{FileWrite, Operation, OperationKind, OperationState, Store};

#[test]
fn reopening_rolls_an_interrupted_apply_forward() -> TestResult {
    let home = tempfile::tempdir()?;
    let work = tempfile::tempdir()?;
    let done = work.path().join("done.rs");
    let pending = work.path().join("pending.rs");
    let foreign = work.path().join("foreign.rs");
    std::fs::write(&done, "new\n")?;
    std::fs::write(&pending, "old\n")?;
    std::fs::write(&foreign, "someone else\n")?;
    let store = Store::open(home.path())?;
    let after = store.put_blob("new\n")?;
    store.put_blob("old\n")?;
    let mut op = Operation::new(OperationKind::Apply, "interrupted");
    op.files = [&done, &pending, &foreign]
        .into_iter()
        .map(|path| FileWrite {
            path: path.clone(),
            before_hash: Some(content_hash("old\n")),
            after_hash: Some(after.clone()),
            merged: false,
        })
        .collect();
    op.state = OperationState::Applying;
    store.save_operation(&op)?;
    drop(store);

    let store = Store::open(home.path())?;
    let recovered = store.operation(&op.id)?;
    assert_eq!(recovered.state, OperationState::Applied);
    assert!(recovered.recovered);
    assert_eq!(recovered.conflicts, vec![foreign.clone()]);
    assert_eq!(std::fs::read_to_string(&pending)?, "new\n");
    assert_eq!(std::fs::read_to_string(&foreign)?, "someone else\n");
    Ok(())
}

#[test]
fn concurrent_applies_to_one_file_serialize_and_the_loser_is_refused() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("a.rs", "base\n")?;
    let first = fx.plan(vec![fx.replace("a.rs", "base", "first")])?;
    let second = fx.plan(vec![fx.replace("a.rs", "base", "second")])?;
    let home = fx.store.home().to_path_buf();
    let handles: Vec<_> = [first, second]
        .into_iter()
        .map(|plan| {
            let home = home.clone();
            std::thread::spawn(move || -> Result<OperationState, String> {
                let store = Store::open(&home).map_err(|e| e.to_string())?;
                store
                    .apply(&request(&plan))
                    .map(|op| op.state)
                    .map_err(|e| e.to_string())
            })
        })
        .collect();
    let mut states = Vec::new();
    for handle in handles {
        states.push(handle.join().map_err(|_| "thread panicked")??);
    }
    states.sort_by_key(|state| format!("{state:?}"));
    assert_eq!(
        states,
        vec![OperationState::Applied, OperationState::Failed]
    );
    let text = fx.read("a.rs")?;
    assert!(text == "first\n" || text == "second\n", "{text}");
    Ok(())
}
