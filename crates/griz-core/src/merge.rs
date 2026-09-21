//! Three-way merge for files that changed after they were planned.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Result of merging a planned change onto a file that moved on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MergeOutcome {
    /// Both sides combined without overlap.
    Clean {
        /// Merged text.
        text: String,
    },
    /// The plan and the newer text changed the same lines.
    Conflict {
        /// 1-based, inclusive line ranges in `current` that conflicted.
        regions: Vec<(usize, usize)>,
    },
}

/// Merges the plan's change (`base` to `planned`) onto `current`.
#[must_use]
pub fn three_way(base: &str, planned: &str, current: &str) -> MergeOutcome {
    match diffy::merge(base, planned, current) {
        Ok(text) => MergeOutcome::Clean { text },
        Err(marked) => MergeOutcome::Conflict {
            regions: conflict_regions(&marked, current),
        },
    }
}

/// Reads diffy's diff3 conflict-marker text and locates each conflict's
/// `theirs` segment back in `current`, so a region is `current`'s own line
/// numbers rather than a position in the marked-up text.
fn conflict_regions(marked: &str, current: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut search_from = 0usize;
    let mut segment: Option<String> = None;
    for text in marked.lines() {
        if text.starts_with("=======") {
            segment = Some(String::new());
        } else if text.starts_with(">>>>>>>") {
            close_segment(current, segment.take(), &mut search_from, &mut regions);
        } else if text.starts_with("<<<<<<<") || text.starts_with("|||||||") {
            segment = None;
        } else if let Some(buffer) = &mut segment {
            buffer.push_str(text);
            buffer.push('\n');
        }
    }
    regions
}

/// Closes one conflict's accumulated `theirs` text, locating it in `current`
/// and recording the region, unless the side held none of `current`'s lines.
fn close_segment(
    current: &str,
    segment: Option<String>,
    search_from: &mut usize,
    regions: &mut Vec<(usize, usize)>,
) {
    let Some(text) = segment.filter(|text| !text.is_empty()) else {
        return;
    };
    if let Some(range) = locate(current, &text, search_from) {
        regions.push(range);
    }
}

/// Finds `needle` as a contiguous run of lines in `current` at or after byte
/// `search_from`, returning its 1-based inclusive line range and advancing
/// `search_from` past the match.
fn locate(current: &str, needle: &str, search_from: &mut usize) -> Option<(usize, usize)> {
    let relative = current.get(*search_from..)?.find(needle)?;
    let start_byte = *search_from + relative;
    let start_line = 1 + current[..start_byte].matches('\n').count();
    let end_line = start_line + needle.matches('\n').count().saturating_sub(1);
    *search_from = start_byte + needle.len();
    Some((start_line, end_line))
}
