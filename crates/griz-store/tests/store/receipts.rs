//! Idempotency receipts.

use crate::common::{Fixture, TestResult};
use griz_store::Claim;
use serde_json::json;

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
