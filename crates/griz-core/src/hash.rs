//! Content fingerprints.

use sha2::{Digest, Sha256};

/// Returns the lowercase hex SHA-256 of `text`.
///
/// Every match and every planned edit carries the fingerprint of the file text
/// it was computed against, so a later write can prove the file is unchanged.
#[must_use]
pub fn content_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}
