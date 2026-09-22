//! Content-addressed text storage for before and after file contents.

use crate::{Store, StoreError, write::write_atomic};
use griz_core::content_hash;

/// Largest file text the store keeps, in bytes.
pub const MAX_BLOB: usize = 16 * 1024 * 1024;

/// Prefix that addresses a blob by its content hash, distinct from an
/// operation or plan identifier.
pub const BLOB_PREFIX: &str = "blob_";

/// The addressable identifier for the blob with fingerprint `hash`.
#[must_use]
pub fn blob_id(hash: &str) -> String {
    format!("{BLOB_PREFIX}{hash}")
}

/// The fingerprint `id` addresses, when it is a blob identifier.
#[must_use]
pub fn parse_blob_id(id: &str) -> Option<&str> {
    id.strip_prefix(BLOB_PREFIX)
}

impl Store {
    /// Stores `text` and returns its fingerprint.
    ///
    /// # Errors
    /// Returns an error when the text is too large or cannot be written.
    pub fn put_blob(&self, text: &str) -> Result<String, StoreError> {
        if text.len() > MAX_BLOB {
            return Err(StoreError::Invalid(format!(
                "file text of {} bytes exceeds the {MAX_BLOB}-byte limit",
                text.len()
            )));
        }
        self.store_blob(text)
    }

    pub(crate) fn store_blob(&self, text: &str) -> Result<String, StoreError> {
        let hash = content_hash(text);
        let path = self.blob_path(&hash);
        if !path.exists() {
            write_atomic(&path, text)?;
        }
        Ok(hash)
    }

    /// Reads the text with fingerprint `hash`.
    ///
    /// # Errors
    /// Returns an error when the fingerprint is malformed or no such text is stored.
    pub fn get_blob(&self, hash: &str) -> Result<String, StoreError> {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(StoreError::Invalid(
                "blob fingerprint must be 64 lowercase hexadecimal characters".into(),
            ));
        }
        std::fs::read_to_string(self.blob_path(hash))
            .map_err(|_| StoreError::NotFound(format!("text {hash}")))
    }

    fn blob_path(&self, hash: &str) -> std::path::PathBuf {
        let (dir, rest) = hash.split_at(2.min(hash.len()));
        self.home().join("blobs").join(dir).join(rest)
    }
}
