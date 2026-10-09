//! Reuses SHA-256 results only after comparing every input byte.

use crate::hash::sha256;
use std::{collections::VecDeque, sync::Mutex};

/// An owned SHA-256 result cache with a byte capacity chosen by its caller.
pub struct ContentHashCache {
    capacity: usize,
    entries: Mutex<VecDeque<CachedDigest>>,
}

struct CachedDigest {
    bytes: Vec<u8>,
    hash: String,
}

impl ContentHashCache {
    /// Creates an empty cache retaining at most `capacity` input bytes.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    /// Returns the lowercase hexadecimal SHA-256 of all `bytes`.
    ///
    /// A cached digest is reused only for an exactly equal byte buffer.
    /// Oversized inputs and an unavailable cache are hashed directly.
    #[must_use]
    pub fn hash(&self, bytes: &[u8]) -> String {
        if bytes.len() > self.capacity {
            return sha256(bytes);
        }
        if let Some(hash) = self
            .entries
            .lock()
            .ok()
            .and_then(|mut entries| lookup(&mut entries, bytes))
        {
            return hash;
        }
        let hash = sha256(bytes);
        if let Ok(mut entries) = self.entries.lock() {
            remember(&mut entries, bytes, &hash, self.capacity);
        }
        hash
    }

    /// Returns the actual input bytes retained, or `None` if the cache is unavailable.
    #[must_use]
    pub fn cached_bytes(&self) -> Option<usize> {
        self.entries
            .lock()
            .ok()
            .map(|entries| resident_bytes(&entries))
    }
}

fn lookup(entries: &mut VecDeque<CachedDigest>, bytes: &[u8]) -> Option<String> {
    let index = entries
        .iter()
        .position(|entry| entry.bytes.as_slice() == bytes)?;
    let entry = entries.remove(index)?;
    let hash = entry.hash.clone();
    entries.push_back(entry);
    Some(hash)
}

fn remember(entries: &mut VecDeque<CachedDigest>, bytes: &[u8], hash: &str, capacity: usize) {
    if lookup(entries, bytes).is_some() {
        return;
    }
    while resident_bytes(entries).saturating_add(bytes.len()) > capacity {
        entries.pop_front();
    }
    entries.push_back(CachedDigest {
        bytes: bytes.to_vec(),
        hash: hash.to_string(),
    });
}

fn resident_bytes(entries: &VecDeque<CachedDigest>) -> usize {
    entries.iter().map(|entry| entry.bytes.len()).sum()
}
