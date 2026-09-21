//! Idempotency receipts: one caller key, one result, however often it is retried.

use crate::{Store, StoreError, now_ms};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;

/// What a key already means.
#[derive(Debug, Clone, PartialEq)]
pub enum Claim {
    /// First use; the caller must run the command and finish the receipt.
    New,
    /// Already finished with this result.
    Replay(Value),
    /// Used before with different input.
    Conflict,
    /// Claimed but never finished, so the outcome is unknown.
    Unknown,
}

impl Store {
    /// Claims `key` for `command` with input `fingerprint`.
    ///
    /// # Errors
    /// Returns an error when the database fails.
    pub fn claim(&self, key: &str, command: &str, fingerprint: &str) -> Result<Claim, StoreError> {
        self.with(|conn| claim_in(conn, key, command, fingerprint))
    }

    /// Records the result for a claimed key.
    ///
    /// # Errors
    /// Returns an error when the database fails.
    pub fn finish_receipt(&self, key: &str, result: &Value) -> Result<(), StoreError> {
        let text = serde_json::to_string(result)?;
        self.with(|conn| {
            conn.execute(
                "UPDATE receipts SET state = 'done', result = ?2 WHERE key = ?1",
                params![key, text],
            )?;
            Ok(())
        })
    }
}

fn claim_in(
    conn: &mut Connection,
    key: &str,
    command: &str,
    fingerprint: &str,
) -> Result<Claim, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT fingerprint, result FROM receipts WHERE key = ?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let claim = match found {
        None => {
            tx.execute(
                "INSERT INTO receipts (key, command, fingerprint, state, created_at)
                 VALUES (?1, ?2, ?3, 'intent', ?4)",
                params![key, command, fingerprint, now_ms()],
            )?;
            Claim::New
        }
        Some((stored, _)) if stored != fingerprint => Claim::Conflict,
        Some((_, Some(result))) => Claim::Replay(serde_json::from_str(&result)?),
        Some(_) => Claim::Unknown,
    };
    tx.commit()?;
    Ok(claim)
}

impl Store {
    /// Forgets a claimed key whose command changed nothing.
    ///
    /// # Errors
    /// Returns an error when the database fails.
    pub fn release_receipt(&self, key: &str) -> Result<(), StoreError> {
        self.with(|conn| {
            conn.execute(
                "DELETE FROM receipts WHERE key = ?1 AND state = 'intent'",
                [key],
            )?;
            Ok(())
        })
    }
}
