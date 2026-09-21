//! Replacing a structural pattern match: griz-core's third locator, next to
//! anchor text and byte ranges.

use crate::{
    ByteRange, Occurrence, ProblemKind, Rung,
    edits::{edit, text_of},
    find::Hit,
    matcher::line_of,
    overlay::Slot,
    pattern_locator::PatternLocator,
    plan::OpResult,
    splice::splice,
    structural::Shape,
};
use std::path::Path;

/// An anchored replacement located by structural pattern instead of text.
pub struct PatternSpec<'a> {
    /// Pattern to locate.
    pub locator: &'a PatternLocator,
    /// Replacement text.
    pub replace: &'a str,
    /// Which matches to replace.
    pub occurrence: Occurrence,
    /// Edit only this metavariable's range, e.g. `$NAME`.
    pub target: Option<&'a str>,
}

/// Replaces the matches of a structural pattern selected by its occurrence
/// rule. With `target`, only that metavariable's range inside each match is
/// replaced. Every produced edit is [`Rung::Pattern`]: exact identity on the
/// parse tree, never a tolerant rung.
pub fn replace_pattern(
    slot: &mut Slot,
    index: usize,
    path: &Path,
    spec: &PatternSpec<'_>,
) -> OpResult {
    let text = text_of(slot)?;
    let hits = pattern_hits(text, path, spec.locator)?;
    let chosen = choose_hits(text, hits, spec.occurrence)?;
    let many = chosen.len() > 1;
    let targets: Vec<ByteRange> = chosen
        .iter()
        .map(|hit| target_range(hit, spec.target))
        .collect::<Result<_, _>>()?;
    let edits: Vec<_> = targets
        .iter()
        .enumerate()
        .map(|(n, range)| {
            let id = if many {
                format!("e{index}.{}", n + 1)
            } else {
                format!("e{index}")
            };
            edit(id, index, path, line_of(text, range.start), Rung::Pattern)
        })
        .collect();
    let mut next = text.to_string();
    for range in targets.iter().rev() {
        next = splice(&next, *range, spec.replace);
        slot.log.record(*range, spec.replace.len());
    }
    slot.current = Some(next);
    Ok(edits)
}

fn pattern_hits(
    text: &str,
    path: &Path,
    locator: &PatternLocator,
) -> Result<Vec<Hit>, ProblemKind> {
    let shape = Shape::new(&locator.pattern, locator.language.as_deref())
        .map_err(|message| ProblemKind::Invalid { message })?;
    shape
        .hits(path, text)
        .map_err(|message| ProblemKind::Invalid { message })
}

/// Chooses among structural hits by occurrence. Never takes the first match
/// on ambiguity: every candidate line is reported instead.
fn choose_hits(
    text: &str,
    hits: Vec<Hit>,
    occurrence: Occurrence,
) -> Result<Vec<Hit>, ProblemKind> {
    if hits.is_empty() {
        return Err(ProblemKind::Missing { nearest: None });
    }
    match occurrence {
        Occurrence::All => Ok(hits),
        Occurrence::Unique if hits.len() == 1 => Ok(hits),
        Occurrence::Unique => Err(ProblemKind::Ambiguous {
            lines: hits.iter().map(|h| line_of(text, h.range.start)).collect(),
        }),
        Occurrence::Nth(n) => hits
            .get(n.wrapping_sub(1))
            .cloned()
            .map(|hit| vec![hit])
            .ok_or_else(|| ProblemKind::Invalid {
                message: format!("occurrence {n} requested but {} matched", hits.len()),
            }),
    }
}

fn target_range(hit: &Hit, target: Option<&str>) -> Result<ByteRange, ProblemKind> {
    let Some(target) = target else {
        return Ok(ByteRange {
            start: hit.range.start,
            end: hit.range.end,
        });
    };
    let name = target.strip_prefix('$').unwrap_or(target);
    hit.var_ranges
        .get(name)
        .map(|range| ByteRange {
            start: range.start,
            end: range.end,
        })
        .ok_or_else(|| ProblemKind::Invalid {
            message: format!("pattern has no capture `{target}`"),
        })
}
