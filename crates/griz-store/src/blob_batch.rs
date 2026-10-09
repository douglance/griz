//! Persist plan content in bounded groups before making the plan visible.
use crate::{
    Store, StoreError,
    blobs::MAX_BLOB,
    committing::commit_files,
    staging::{FileStage, stage_files},
};
use griz_core::{FileChange, Plan, content_hash};
use std::{borrow::Cow, collections::BTreeSet, path::PathBuf};

const BUFFER_BYTES: usize = 64 * 1024 * 1024;

struct BlobBatch<'a> {
    store: &'a Store,
    seen: BTreeSet<String>,
    pending: Vec<(PathBuf, Cow<'a, [u8]>)>,
    bytes: usize,
}

impl Store {
    pub(crate) fn persist_plan_blobs(&self, plan: &Plan) -> Result<(), StoreError> {
        let mut batch = BlobBatch {
            store: self,
            seen: BTreeSet::new(),
            pending: Vec::new(),
            bytes: 0,
        };
        for file in &plan.files {
            push_side(&mut batch, file, true)?;
            push_side(&mut batch, file, false)?;
        }
        for text in plan.unchanged_inputs.values().flatten() {
            push_blob(&mut batch, Cow::Borrowed(text.as_bytes()))?;
        }
        flush_blobs(&mut batch)
    }
}

fn push_side<'a>(
    batch: &mut BlobBatch<'a>,
    file: &'a FileChange,
    before: bool,
) -> Result<(), StoreError> {
    let text = if before { &file.before } else { &file.after };
    if let Some(text) = text {
        push_blob(batch, Cow::Borrowed(text.as_bytes()))?;
    }
    Ok(())
}

fn push_blob<'a>(batch: &mut BlobBatch<'a>, bytes: Cow<'a, [u8]>) -> Result<(), StoreError> {
    if bytes.len() > MAX_BLOB {
        return Err(StoreError::Invalid(format!(
            "file content of {} bytes exceeds the {MAX_BLOB}-byte limit",
            bytes.len()
        )));
    }
    let hash = content_hash(
        std::str::from_utf8(&bytes).map_err(|error| StoreError::Invalid(error.to_string()))?,
    );
    let path = batch.store.blob_path(&hash);
    if path.exists() || !batch.seen.insert(hash) {
        return Ok(());
    }
    if batch.bytes + bytes.len() > BUFFER_BYTES {
        flush_blobs(batch)?;
    }
    batch.bytes += bytes.len();
    batch.pending.push((path, bytes));
    Ok(())
}

fn flush_blobs(batch: &mut BlobBatch<'_>) -> Result<(), StoreError> {
    let files: Vec<_> = batch
        .pending
        .iter()
        .map(|(path, bytes)| FileStage {
            path,
            bytes: Some(bytes),
        })
        .collect();
    let staged = stage_files(&files)?;
    let paths: Vec<_> = files.iter().map(|file| file.path).collect();
    commit_files(&paths, staged)?;
    batch.pending.clear();
    batch.bytes = 0;
    Ok(())
}
