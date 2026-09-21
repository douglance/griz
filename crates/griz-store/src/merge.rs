//! Three-way merge resolution shared by apply, undo, and restore.

use crate::{
    Operation, Store, StoreError,
    apply::OnStale,
    blobs::blob_id,
    execute::{Target, read_current},
    journal::{ConflictRegion, MergeConflict},
};
use griz_core::{MergeOutcome, three_way};
use std::path::{Path, PathBuf};

/// Reads the blob at `hash`, or `None` when there is none.
///
/// # Errors
/// Returns an error when a hash is given but no such blob is stored.
pub(crate) fn blob_text(store: &Store, hash: Option<&str>) -> Result<Option<String>, StoreError> {
    hash.map(|hash| store.get_blob(hash)).transpose()
}

/// The base and planned text a candidate write is merged from.
pub struct MergePair<'a> {
    /// Fingerprint the write expects to find on disk.
    pub base_hash: Option<&'a str>,
    /// Text at `base_hash`.
    pub base: Option<&'a str>,
    /// Fingerprint the write wants to leave on disk.
    pub planned_hash: Option<&'a str>,
    /// Text at `planned_hash`.
    pub planned: Option<&'a str>,
}

/// What resolving one file against `on_stale` produced.
pub enum Resolved {
    /// Ready to write.
    Ready(Target),
    /// Left alone; no merge was attempted, or none was possible.
    Stale,
    /// A merge was attempted and conflicted.
    Conflicted(MergeConflict),
}

/// Resolves the write for one file: exact when disk still has `base_hash`,
/// merged onto disk when it does not and `on_stale` allows it, else stale.
///
/// # Errors
/// Returns an error when the current text cannot be read or a blob cannot be
/// stored for a conflict's record.
pub fn resolve_target(
    store: &Store,
    path: &Path,
    pair: &MergePair<'_>,
    on_stale: OnStale,
) -> Result<Resolved, StoreError> {
    let (current, current_hash) = read_current(path)?;
    if current_hash.as_deref() == pair.base_hash {
        let target = ready(path, current_hash, current, pair.planned, false);
        return Ok(Resolved::Ready(target));
    }
    if on_stale == OnStale::Refuse {
        return Ok(Resolved::Stale);
    }
    attempt_merge(
        store,
        path,
        pair,
        current.as_deref(),
        current_hash.as_deref(),
    )
}

fn ready(
    path: &Path,
    current_hash: Option<String>,
    current: Option<String>,
    planned: Option<&str>,
    merged: bool,
) -> Target {
    Target {
        path: path.to_path_buf(),
        current_hash,
        current,
        text: planned.map(str::to_string),
        merged,
    }
}

fn attempt_merge(
    store: &Store,
    path: &Path,
    pair: &MergePair<'_>,
    current: Option<&str>,
    current_hash: Option<&str>,
) -> Result<Resolved, StoreError> {
    let (Some(base_hash), Some(base), Some(planned_hash), Some(planned), Some(now), Some(now_hash)) = (
        pair.base_hash,
        pair.base,
        pair.planned_hash,
        pair.planned,
        current,
        current_hash,
    ) else {
        return Ok(Resolved::Stale);
    };
    match three_way(base, planned, now) {
        MergeOutcome::Clean { text } => {
            let target = ready(
                path,
                Some(now_hash.to_string()),
                Some(now.to_string()),
                Some(text.as_str()),
                true,
            );
            Ok(Resolved::Ready(target))
        }
        MergeOutcome::Conflict { regions } => {
            let hashes = (base_hash, planned_hash);
            let current = (now, now_hash);
            Ok(Resolved::Conflicted(build_conflict(
                store, path, hashes, current, regions,
            )?))
        }
    }
}

/// Records one file's resolved write against the operation being built:
/// queues it when ready, or lists it as a conflict, with merge detail when
/// there is any.
pub(crate) fn record_resolved(
    op: &mut Operation,
    targets: &mut Vec<Target>,
    path: PathBuf,
    resolved: Resolved,
) {
    match resolved {
        Resolved::Ready(target) => targets.push(target),
        Resolved::Stale => op.conflicts.push(path),
        Resolved::Conflicted(detail) => {
            op.conflicts.push(path);
            op.merge_conflicts.push(detail);
        }
    }
}

fn build_conflict(
    store: &Store,
    path: &Path,
    hashes: (&str, &str),
    current: (&str, &str),
    regions: Vec<(usize, usize)>,
) -> Result<MergeConflict, StoreError> {
    let (base_hash, planned_hash) = hashes;
    let (text, hash) = current;
    let current_blob = store.put_blob(text)?;
    Ok(MergeConflict {
        path: path.to_path_buf(),
        base: blob_id(base_hash),
        planned: blob_id(planned_hash),
        current: blob_id(&current_blob),
        current_fingerprint: hash.to_string(),
        regions: regions
            .into_iter()
            .map(|(start, end)| ConflictRegion { start, end })
            .collect(),
    })
}
