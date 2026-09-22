//! Idempotency receipts.

use crate::common::{Fixture, TestResult};
use griz_store::Claim;
use serde_json::json;

#[test]
fn receipt_snapshots_do_not_apply_the_per_source_file_size_limit() -> TestResult {
    let fx = Fixture::new()?;
    let text = "x".repeat(16 * 1024 * 1024 + 1);
    assert!(fx.store.put_blob(&text).is_err());
    let data = json!({"metadata": text});
    let hash = fx.store.save_receipt_snapshot(&data)?;
    assert_eq!(fx.store.receipt_snapshot(&hash)?, data);
    assert_eq!(fx.store.save_receipt_snapshot(&data)?, hash);
    assert_eq!(fx.store.claim("snapshot", "absorb", "input")?, Claim::New);
    let receipt = json!({"id": "op_saved", "outcome": "passed", "snapshot": hash});
    fx.store.finish_receipt("snapshot", &receipt)?;
    let Claim::Replay(replayed) = fx.store.claim("snapshot", "absorb", "input")? else {
        panic!("finished receipt was not replayed");
    };
    assert!(serde_json::to_string(&replayed)?.len() < 256);
    assert_eq!(replayed, receipt);
    Ok(())
}

#[test]
fn a_key_replays_its_result_and_rejects_different_input() -> TestResult {
    let fx = Fixture::new()?;
    assert_eq!(fx.store.claim("k", "apply", "f1")?, Claim::New);
    assert_eq!(fx.store.claim("k", "apply", "f1")?, Claim::Unknown);
    fx.store.finish_receipt("k", &json!({ "id": "op_1" }))?;
    assert_eq!(
        fx.store.claim("k", "apply", "f1")?,
        Claim::Replay(json!({ "id": "op_1" }))
    );
    assert_eq!(fx.store.claim("k", "apply", "f2")?, Claim::Conflict);
    Ok(())
}
