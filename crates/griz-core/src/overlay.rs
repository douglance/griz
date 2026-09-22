//! An in-memory copy of every file a plan touches.
//!
//! Operations change the overlay, never the disk. The overlay records each
//! file's text as first read, so the finished plan knows exactly what it was
//! computed against.

use crate::{
    ChangeKind, FileChange, Source, content_hash, find_position::Cursor, splice::SpliceLog,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// One file's state inside the overlay.
#[derive(Debug, Clone, Default)]
pub struct Slot {
    /// Text as first read; `None` when the file did not exist.
    pub before: Option<String>,
    /// Fingerprint of the text as first read, shared by every operation's guard.
    pub before_hash: Option<String>,
    /// Text after every operation so far; `None` when absent.
    pub current: Option<String>,
    /// Splices applied so far, to map find-result ranges.
    pub log: SpliceLog,
    /// Position at the last range edit's start in the current text.
    pub position: Cursor,
}

/// Every file the plan has read or changed.
pub struct Overlay<'a> {
    source: &'a dyn Source,
    slots: BTreeMap<PathBuf, Slot>,
}

impl<'a> Overlay<'a> {
    /// Starts an empty overlay over `source`.
    pub fn new(source: &'a dyn Source) -> Self {
        Self {
            source,
            slots: BTreeMap::new(),
        }
    }

    /// Returns the slot for `path`, reading it on first use.
    ///
    /// # Errors
    /// Returns the source's message when the file cannot be read.
    pub fn slot(&mut self, path: &Path) -> Result<&mut Slot, String> {
        if !self.slots.contains_key(path) {
            let text = self.source.read(path)?;
            self.slots.insert(
                path.to_path_buf(),
                Slot {
                    before: text.clone(),
                    before_hash: text.as_deref().map(content_hash),
                    current: text,
                    log: SpliceLog::default(),
                    position: Cursor::default(),
                },
            );
        }
        self.slots
            .get_mut(path)
            .ok_or_else(|| format!("{}: slot vanished", path.display()))
    }

    /// Discards the cached position before a non-range operation changes this file.
    pub fn reset_position(&mut self, path: &Path) {
        if let Some(slot) = self.slots.get_mut(path) {
            slot.position = Cursor::default();
        }
    }

    /// Every file whose text changed, sorted by path.
    #[must_use]
    pub fn into_changes(self) -> Vec<FileChange> {
        self.slots
            .into_iter()
            .filter(|(_, slot)| slot.before != slot.current)
            .map(|(path, slot)| change(path, slot))
            .collect()
    }
}

fn change(path: PathBuf, slot: Slot) -> FileChange {
    let kind = match (&slot.before, &slot.current) {
        (None, _) => ChangeKind::Create,
        (Some(_), None) => ChangeKind::Delete,
        (Some(_), Some(_)) => ChangeKind::Modify,
    };
    FileChange {
        path,
        kind,
        before_hash: slot.before_hash,
        after_hash: slot.current.as_deref().map(content_hash),
        before: slot.before,
        after: slot.current,
    }
}
