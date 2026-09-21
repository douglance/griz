//! Store failures.

/// Why a store call failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The database failed.
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The filesystem failed.
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    /// A stored record could not be decoded.
    #[error("record: {0}")]
    Json(#[from] serde_json::Error),
    /// No record has this identifier.
    #[error("not found: {0}")]
    NotFound(String),
    /// The request cannot be satisfied as asked.
    #[error("{0}")]
    Invalid(String),
}
