//! Builds located matches while tracking successive character positions.

use crate::{
    ByteRange,
    find::{Hit, Match},
};
use std::path::Path;

/// Position at the last visited byte, counted from zero.
#[derive(Debug, Clone, Default)]
pub(crate) struct Cursor {
    byte: usize,
    line: usize,
    column: usize,
}

/// Returns the one-based line and character column at a match start.
pub(crate) fn position(cursor: &mut Cursor, text: &str, byte: usize) -> (usize, usize) {
    if byte < cursor.byte {
        *cursor = Cursor::default();
    }
    let skipped = &text[cursor.byte..byte];
    let mut newlines = memchr::memchr_iter(b'\n', skipped.as_bytes());
    if let Some(last) = newlines.next_back() {
        cursor.line += newlines.count() + 1;
        cursor.column = skipped[last + 1..].chars().count();
    } else {
        cursor.column += skipped.chars().count();
    }
    cursor.byte = byte;
    (cursor.line + 1, cursor.column + 1)
}

pub(crate) fn to_match(
    path: &Path,
    text: &str,
    hit: Hit,
    hash: &str,
    (line, column): (usize, usize),
) -> Match {
    let whole = hit.range;
    Match {
        path: path.to_path_buf(),
        line,
        column,
        range: ByteRange {
            start: whole.start,
            end: whole.end,
        },
        text: text[whole].to_string(),
        captures: hit.captures,
        vars: hit.vars,
        var_ranges: hit
            .var_ranges
            .into_iter()
            .map(|(name, range)| {
                (
                    name,
                    ByteRange {
                        start: range.start,
                        end: range.end,
                    },
                )
            })
            .collect(),
        file_hash: hash.to_string(),
    }
}
