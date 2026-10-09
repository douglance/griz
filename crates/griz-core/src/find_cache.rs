//! Bounded search-page reuse, validated against freshly read file contents.
use crate::{FindPage, FindQuery, content_hash};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
const ENTRIES: usize = 8;
const PAGE_BYTES: usize = 64 * 1024;
static CACHE: OnceLock<Mutex<VecDeque<Entry>>> = OnceLock::new();
#[derive(Clone, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    literal: Option<String>,
    regex: Option<String>,
    window: (usize, usize),
}
struct Entry {
    key: Key,
    page: FindPage,
}
fn key(query: &FindQuery, path: &Path, window: (usize, usize)) -> Option<Key> {
    if query.pattern.is_some() || !query.within.is_empty() || window.1 == 0 || window.1 > 256 {
        return None;
    }
    let expression = query.literal.as_ref().or(query.regex.as_ref())?;
    if expression.len() > 4096 {
        return None;
    }
    Some(Key {
        path: path.to_path_buf(),
        literal: query.literal.clone(),
        regex: query.regex.clone(),
        window,
    })
}
pub(crate) fn lookup(
    query: &FindQuery,
    path: &Path,
    text: &str,
    window: (usize, usize),
) -> Option<FindPage> {
    let key = key(query, path, window)?;
    let page = CACHE
        .get()?
        .lock()
        .ok()?
        .iter()
        .find(|entry| entry.key == key)?
        .page
        .clone();
    let expected = &page.matches.first()?.file_hash;
    (content_hash(text) == *expected).then_some(page)
}
pub(crate) fn remember(query: &FindQuery, path: &Path, window: (usize, usize), page: &FindPage) {
    let Some(key) = key(query, path, window) else {
        return;
    };
    if page.matches.is_empty() || serde_json::to_writer(PageSize(0), page).is_err() {
        return;
    }
    let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(VecDeque::new())).lock() else {
        return;
    };
    cache.retain(|entry| entry.key != key);
    if cache.len() == ENTRIES {
        cache.pop_front();
    }
    cache.push_back(Entry {
        key,
        page: page.clone(),
    });
}

struct PageSize(usize);
impl std::io::Write for PageSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > PAGE_BYTES {
            return Err(std::io::Error::other("cached page exceeds size limit"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
