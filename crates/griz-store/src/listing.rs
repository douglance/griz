//! Listing operations: one store holds every repository's history, so a
//! caller narrows it to the paths it cares about.

use crate::{Operation, Store, StoreError};
use std::path::PathBuf;

/// One page of operations, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The operations kept, newest first.
    pub operations: Vec<Operation>,
    /// Pass as `before` for the next page; `None` when the history ended.
    pub next: Option<String>,
}

impl Page {
    /// A page of `operations`, continuing from `next`.
    fn new(operations: Vec<Operation>, next: Option<String>) -> Self {
        Self { operations, next }
    }
}

impl Store {
    /// One page of operations that wrote a file at or under any of `under`,
    /// newest first; every operation when `under` is empty. `next` is the
    /// last operation scanned, not the last kept, so paging continues past
    /// operations the paths filtered out.
    ///
    /// # Errors
    /// Returns an error when the records cannot be read.
    pub fn operations_under(
        &self,
        limit: usize,
        before: Option<&str>,
        under: &[PathBuf],
    ) -> Result<Page, StoreError> {
        if under.is_empty() {
            return unfiltered(self, limit, before);
        }
        scan(self, limit, before, under)
    }
}

/// Every operation in the page, with the cursor for the next one.
fn unfiltered(store: &Store, limit: usize, before: Option<&str>) -> Result<Page, StoreError> {
    let operations = store.operations(limit, before)?;
    let next = (operations.len() == limit)
        .then(|| operations.last().map(|op| op.id.clone()))
        .flatten();
    Ok(Page::new(operations, next))
}

/// Reads batches newest first until `limit` operations touch `under` or the
/// history ends.
fn scan(
    store: &Store,
    limit: usize,
    before: Option<&str>,
    under: &[PathBuf],
) -> Result<Page, StoreError> {
    let batch = limit.max(64);
    let mut cursor = before.map(str::to_string);
    let mut kept = Vec::new();
    let mut exhausted = false;
    while kept.len() < limit && !exhausted {
        let page = store.operations(batch, cursor.as_deref())?;
        exhausted = page.len() < batch;
        let (mut taken, scanned) = take_under(page, under, limit - kept.len());
        kept.append(&mut taken);
        cursor = scanned.or(cursor);
    }
    // A full page may have stopped mid-batch, so it can continue even when
    // that batch was the last one; a short page reached the end.
    let next = (kept.len() == limit).then_some(cursor).flatten();
    Ok(Page::new(kept, next))
}

/// Up to `want` operations from `page` that touch `under`, and the id of the
/// last operation looked at.
fn take_under(
    page: Vec<Operation>,
    under: &[PathBuf],
    want: usize,
) -> (Vec<Operation>, Option<String>) {
    let mut kept = Vec::new();
    let mut scanned = None;
    for op in page {
        scanned = Some(op.id.clone());
        if touches(&op, under) {
            kept.push(op);
        }
        if kept.len() == want {
            break;
        }
    }
    (kept, scanned)
}

/// Whether an operation wrote a file at or under any of `under`.
fn touches(op: &Operation, under: &[PathBuf]) -> bool {
    op.files
        .iter()
        .any(|file| under.iter().any(|dir| file.path.starts_with(dir)))
}
