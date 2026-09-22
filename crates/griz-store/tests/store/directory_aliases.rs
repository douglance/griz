//! Persisted or direct-store plans must not write one destination twice.
#![cfg(unix)]

use crate::common::{Fixture, TestResult, request};
use griz_core::content_hash;
use griz_store::StoreError;
use std::{
    fs::{self, File, TryLockError},
    os::unix::fs::symlink,
};

#[test]
fn a_resolver_keeps_directory_identity_only_within_its_batch() -> TestResult {
    let fx = Fixture::new()?;
    fs::create_dir(fx.path("actual"))?;
    fs::create_dir(fx.path("other"))?;
    symlink(fx.path("actual"), fx.path("link"))?;
    let actual = fx.path("actual").canonicalize()?;
    let other = fx.path("other").canonicalize()?;
    let mut resolver = griz_store::PathResolver::default();
    assert_eq!(
        resolver.resolve(&fx.path("link/a.txt"))?,
        actual.join("a.txt")
    );
    fs::remove_file(fx.path("link"))?;
    symlink(fx.path("other"), fx.path("link"))?;
    assert_eq!(
        resolver.resolve(&fx.path("link/b.txt"))?,
        actual.join("b.txt")
    );
    assert_eq!(
        griz_store::destination_path(&fx.path("link/b.txt"))?,
        other.join("b.txt")
    );
    Ok(())
}

#[test]
fn stored_aliases_are_rejected_before_either_write() -> TestResult {
    let fx = Fixture::new()?;
    fs::create_dir(fx.path("actual"))?;
    fx.write("actual/target.txt", "one two\n")?;
    symlink(fx.path("actual"), fx.path("link"))?;
    let plan = fx.plan(vec![
        fx.replace("actual/target.txt", "one", "ONE"),
        fx.replace("link/target.txt", "two", "TWO"),
    ])?;
    let result = fx.store.apply(&request(&plan));
    assert!(matches!(result, Err(StoreError::Invalid(_))), "{result:?}");
    assert_eq!(fx.read("actual/target.txt")?, "one two\n");
    Ok(())
}

#[test]
fn directory_aliases_hold_one_canonical_destination_lock() -> TestResult {
    let fx = Fixture::new()?;
    fs::create_dir(fx.path("actual"))?;
    fx.write("actual/target.txt", "before")?;
    symlink(fx.path("actual"), fx.path("link"))?;
    let canonical = fx.path("actual/target.txt").canonicalize()?;
    let key = content_hash(&canonical.to_string_lossy());
    let locks = fx
        .store
        .lock_paths(&[fx.path("link/target.txt"), canonical])?;
    let dir = fx.store.home().join("locks");
    let held = File::options()
        .write(true)
        .open(dir.join(format!("{key}.lock")))?;
    assert!(matches!(held.try_lock(), Err(TryLockError::WouldBlock)));
    assert_eq!(fs::read_dir(dir)?.count(), 1);
    drop(locks);
    held.try_lock()?;
    Ok(())
}
