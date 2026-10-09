//! Content fingerprints.

use crate::ContentHashCache;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const MIN_CACHED_BYTES: usize = 1024 * 1024;
static LARGE_HASH_CACHE: OnceLock<ContentHashCache> = OnceLock::new();

/// Returns the lowercase hex SHA-256 of bytes.
#[must_use]
pub fn content_hash_bytes(bytes: &[u8]) -> String {
    if bytes.len() < MIN_CACHED_BYTES {
        return sha256(bytes);
    }
    LARGE_HASH_CACHE
        .get_or_init(|| ContentHashCache::new(32 * 1024 * 1024))
        .hash(bytes)
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Returns the lowercase hex SHA-256 of `text`.
///
/// Every match and every planned edit carries the fingerprint of the file text
/// it was computed against, so a later write can prove the file is unchanged.
#[must_use]
pub fn content_hash(text: &str) -> String {
    content_hash_bytes(text.as_bytes())
}
