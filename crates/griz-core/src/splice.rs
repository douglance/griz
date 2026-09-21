//! Tracks splices so byte ranges from a find result stay valid as earlier
//! edits in the same file change its length.

use crate::ByteRange;

#[derive(Debug, Clone, Copy)]
struct Splice {
    start: usize,
    old_len: usize,
    new_len: usize,
}

/// Every splice applied to one file's text, in order.
#[derive(Debug, Clone, Default)]
pub struct SpliceLog {
    splices: Vec<Splice>,
}

impl SpliceLog {
    /// Records that `range` of the current text became `new_len` bytes.
    pub fn record(&mut self, range: ByteRange, new_len: usize) {
        self.splices.push(Splice {
            start: range.start,
            old_len: range.end - range.start,
            new_len,
        });
    }

    /// Maps a range in the original text into the current text, or `None`
    /// when an earlier splice overlaps it.
    #[must_use]
    pub fn map(&self, range: ByteRange) -> Option<ByteRange> {
        self.splices
            .iter()
            .try_fold(range, |range, splice| map_one(range, *splice))
    }
}

fn map_one(range: ByteRange, splice: Splice) -> Option<ByteRange> {
    let splice_end = splice.start + splice.old_len;
    if range.end <= splice.start {
        return Some(range);
    }
    if range.start >= splice_end {
        let shift = |at: usize| (at + splice.new_len).checked_sub(splice.old_len);
        return Some(ByteRange {
            start: shift(range.start)?,
            end: shift(range.end)?,
        });
    }
    None
}

/// Replaces `range` of `text` with `with`.
#[must_use]
pub fn splice(text: &str, range: ByteRange, with: &str) -> String {
    let mut out = String::with_capacity(text.len() + with.len());
    out.push_str(&text[..range.start]);
    out.push_str(with);
    out.push_str(&text[range.end..]);
    out
}
