//! Concurrent blob writes retain complete content for every caller.

use crate::common::TestResult;
use griz_store::Store;
use std::{
    sync::{Arc, Barrier},
    thread::{self, JoinHandle},
};

fn writer(
    store: Arc<Store>,
    ready: Arc<Barrier>,
    text: Arc<String>,
) -> JoinHandle<Result<String, String>> {
    thread::spawn(move || {
        ready.wait();
        let hash = store.put_blob(&text).map_err(|error| error.to_string())?;
        let stored = store.get_blob(&hash).map_err(|error| error.to_string())?;
        if stored != *text {
            return Err("blob content differs from the submitted text".into());
        }
        Ok(hash)
    })
}

#[test]
fn concurrent_writes_of_the_same_new_blob_all_succeed() -> TestResult {
    let home = tempfile::tempdir()?;
    let store = Arc::new(Store::open(home.path())?);
    for round in 0..8 {
        let text = Arc::new(format!("round {round}\n{}", "x".repeat(1024 * 1024)));
        let ready = Arc::new(Barrier::new(32));
        let handles: Vec<_> = (0..32)
            .map(|_| writer(Arc::clone(&store), Arc::clone(&ready), Arc::clone(&text)))
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "blob writer panicked"))
            .collect::<Result<_, _>>()?;
        let hashes = results.into_iter().collect::<Result<Vec<_>, _>>()?;
        assert_eq!(hashes.len(), 32);
        assert!(hashes.windows(2).all(|pair| pair[0] == pair[1]));
    }
    Ok(())
}
