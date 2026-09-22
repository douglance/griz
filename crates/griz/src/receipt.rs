//! Idempotent mutations: one caller key, one effect, however often retried.

use crate::{
    context::CmdError,
    verdict::{Outcome, Rendered, Verbosity},
};
use griz_core::content_hash;
use griz_store::{Claim, Store};
use serde_json::{Value, json};

/// A mutation's identity for replay.
pub struct Mutation {
    /// Command name; keys are scoped per command.
    pub command: &'static str,
    /// Caller's idempotency key.
    pub key: String,
    /// Every input that changes the effect, excluding purpose and verbosity.
    pub input: Value,
}

impl Mutation {
    fn scoped_key(&self) -> String {
        format!("{}:{}", self.command, self.key)
    }

    fn fingerprint(&self) -> String {
        content_hash(&format!("{}\n{}", self.command, self.input))
    }
}

/// Runs `work` once per key. A repeated key with the same input answers with
/// the original receipt; additional detail is re-read through `replay` only
/// when requested. Different input is refused.
///
/// # Errors
/// Returns the work's error, or an idempotency error.
pub fn run_mutation(
    store: &Store,
    mutation: &Mutation,
    level: Verbosity,
    work: impl FnOnce(&Store) -> Result<Rendered, CmdError>,
    replay: impl FnOnce(&Store, &str, Outcome) -> Result<Rendered, CmdError>,
) -> Result<(Value, Outcome), CmdError> {
    let key = mutation.scoped_key();
    match store.claim(&key, mutation.command, &mutation.fingerprint())? {
        Claim::New => {
            let rendered = match work(store) {
                Ok(rendered) => rendered,
                Err(error) => {
                    release_if_untouched(store, &key, &error)?;
                    return Err(error);
                }
            };
            let receipt = json!({ "id": rendered.id, "outcome": rendered.outcome });
            store.finish_receipt(&key, &receipt)?;
            Ok((rendered.at(level, false), rendered.outcome))
        }
        Claim::Replay(receipt) => {
            let (id, outcome) = decode(&receipt)?;
            if level <= Verbosity::Error {
                return Ok((
                    json!({"id": id, "outcome": outcome, "replayed": true}),
                    outcome,
                ));
            }
            let rendered = replay(store, &id, outcome)?;
            Ok((rendered.at(level, true), outcome))
        }
        Claim::Conflict => Err(CmdError {
            code: "IDEMPOTENCY_CONFLICT",
            message: format!(
                "key `{}` was already used with different input",
                mutation.key
            ),
        }),
        Claim::Unknown => Err(CmdError {
            code: "IDEMPOTENCY_RESULT_UNKNOWN",
            message: format!(
                "key `{}` was claimed but its result was never recorded; inspect `log` before retrying with a new key",
                mutation.key
            ),
        }),
    }
}

/// Frees a key whose command failed before changing anything, so a corrected
/// retry with the same key can run.
fn release_if_untouched(store: &Store, key: &str, error: &CmdError) -> Result<(), CmdError> {
    if matches!(error.code, "VALIDATION_ERROR" | "NOT_FOUND") {
        store.release_receipt(key)?;
    }
    Ok(())
}

fn decode(receipt: &Value) -> Result<(String, Outcome), CmdError> {
    let id = receipt["id"].as_str().map(str::to_string);
    let outcome = serde_json::from_value(receipt["outcome"].clone()).ok();
    id.zip(outcome)
        .ok_or_else(|| CmdError::invalid("stored receipt is malformed"))
}
