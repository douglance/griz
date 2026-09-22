//! Blob lookup accepts only canonical content fingerprints.

use crate::common::{Fixture, TestResult};
use griz_store::{StoreError, blob_id, parse_blob_id};

#[test]
fn blob_lookup_rejects_malformed_fingerprints() -> TestResult {
    let fx = Fixture::new()?;
    let malformed = [
        String::new(),
        "a".into(),
        "0".repeat(63),
        "0".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        "😀".into(),
        "é".repeat(32),
    ];
    for hash in malformed {
        assert!(
            matches!(fx.store.get_blob(&hash), Err(StoreError::Invalid(_))),
            "{hash:?}"
        );
    }
    Ok(())
}

#[test]
fn blob_lookup_cannot_treat_a_path_as_a_fingerprint() -> TestResult {
    let fx = Fixture::new()?;
    fx.write("sentinel.txt", "outside blob store\n")?;
    let hash = format!("aa{}", fx.path("sentinel.txt").display());
    assert!(matches!(
        fx.store.get_blob(&hash),
        Err(StoreError::Invalid(_))
    ));
    Ok(())
}

#[test]
fn canonical_blob_ids_round_trip_and_missing_hashes_stay_not_found() -> TestResult {
    let fx = Fixture::new()?;
    let hash = fx.store.put_blob("stored text 🦀\n")?;
    let id = blob_id(&hash);
    assert_eq!(parse_blob_id(&id), Some(hash.as_str()));
    assert_eq!(fx.store.get_blob(&hash)?, "stored text 🦀\n");
    assert!(matches!(
        fx.store.get_blob(&"0".repeat(64)),
        Err(StoreError::NotFound(_))
    ));
    Ok(())
}
