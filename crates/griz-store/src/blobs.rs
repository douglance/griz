//! Content-addressed text storage for before and after file contents.

use crate::{Store, StoreError, write::write_atomic};
use griz_core::content_hash;

/// Largest file text the store keeps, in bytes.
pub const MAX_BLOB: usize = 16 * 1024 * 1024;

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
    /// Returns an error when no such text is stored.
    pub fn get_blob(&self, hash: &str) -> Result<String, StoreError> {
        std::fs::read_to_string(self.blob_path(hash))
            .map_err(|_| StoreError::NotFound(format!("text {hash}")))
    }

    fn blob_path(&self, hash: &str) -> std::path::PathBuf {
        let (dir, rest) = hash.split_at(2.min(hash.len()));
        self.home().join("blobs").join(dir).join(rest)
    }
}
