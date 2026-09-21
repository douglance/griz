//! Text edits inside one overlay slot.

use crate::{
    Anchor, ByteRange, Edit, Occurrence, ProblemKind, Rung,
    matcher::{Found, Located, line_of, locate},
    overlay::Slot,
    plan::OpResult,
    reindent::reindent,
    splice::splice,
};
use std::path::Path;

/// An anchored replacement.
pub struct ReplaceSpec<'a> {
    /// Text to find.
    pub anchor: &'a Anchor,
    /// Replacement text.
    pub replace: &'a str,
    /// Which matches to replace.
    pub occurrence: Occurrence,
}

fn text_of(slot: &Slot) -> Result<&str, ProblemKind> {
    slot.current.as_deref().ok_or(ProblemKind::NotFound)
}

fn check_anchor(anchor: &Anchor) -> Result<(), ProblemKind> {
    if anchor.text.is_empty() {
        return Err(ProblemKind::Invalid {
            message: "anchor text is empty".to_string(),
        });
    }
    Ok(())
}

/// Replaces an exact byte range from a find result, mapped through earlier
/// edits to the same file. When `expected` is given, the range must still hold
/// that text.
pub fn replace_range(
    slot: &mut Slot,
    index: usize,
    path: &Path,
    range: ByteRange,
    (expected, replace): (Option<&Anchor>, &str),
) -> OpResult {
    let text = text_of(slot)?;
    let mapped = slot.log.map(range).ok_or(ProblemKind::BadRange)?;
    let held = text
        .get(mapped.start..mapped.end)
        .ok_or(ProblemKind::BadRange)?;
    if expected.is_some_and(|anchor| anchor.text != held) {
        return Err(ProblemKind::BadRange);
    }
    let line = line_of(text, mapped.start);
    let next = splice(text, mapped, replace);
    slot.current = Some(next);
    slot.log.record(mapped, replace.len());
    Ok(vec![edit(
        format!("e{index}"),
        index,
        path,
        line,
        Rung::Range,
    )])
}

/// Replaces the matches of an anchor selected by its occurrence rule.
pub fn replace_anchor(
    slot: &mut Slot,
    index: usize,
    path: &Path,
    spec: &ReplaceSpec<'_>,
) -> OpResult {
    check_anchor(spec.anchor)?;
    let text = text_of(slot)?;
    let chosen = choose(text, locate(text, spec.anchor), spec.occurrence)?;
    let many = chosen.len() > 1;
    let edits: Vec<_> = chosen
        .iter()
        .enumerate()
        .map(|(n, found)| {
            let id = if many {
                format!("e{index}.{}", n + 1)
            } else {
                format!("e{index}")
            };
            edit(
                id,
                index,
                path,
                line_of(text, found.range.start),
                found.rung,
            )
        })
        .collect();
    let mut next = text.to_string();
    for found in chosen.iter().rev() {
        let with = replacement(spec.replace, found);
        next = splice(&next, found.range, &with);
        slot.log.record(found.range, with.len());
    }
    slot.current = Some(next);
    Ok(edits)
}

/// Inserts text before or after a unique anchor. An empty anchor with `after`
/// appends at the end of the file, starting a new line when needed.
pub fn insert(
    slot: &mut Slot,
    index: usize,
    path: &Path,
    anchor: &Anchor,
    (after, inserted): (bool, &str),
) -> OpResult {
    let text = text_of(slot)?;
    if anchor.text.is_empty() && after {
        return append(slot, index, path, inserted);
    }
    check_anchor(anchor)?;
    let found = choose(text, locate(text, anchor), Occurrence::Unique)?
        .into_iter()
        .next()
        .ok_or(ProblemKind::Missing { nearest: None })?;
    let at = if after {
        found.range.end
    } else {
        found.range.start
    };
    let point = ByteRange { start: at, end: at };
    let line = line_of(text, at);
    let next = splice(text, point, inserted);
    slot.current = Some(next);
    slot.log.record(point, inserted.len());
    Ok(vec![edit(
        format!("e{index}"),
        index,
        path,
        line,
        found.rung,
    )])
}

fn append(slot: &mut Slot, index: usize, path: &Path, inserted: &str) -> OpResult {
    let text = text_of(slot)?;
    let gap = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let point = ByteRange {
        start: text.len(),
        end: text.len(),
    };
    let added = format!("{gap}{inserted}");
    let line = line_of(text, text.len());
    let next = splice(text, point, &added);
    slot.current = Some(next);
    slot.log.record(point, added.len());
    Ok(vec![edit(
        format!("e{index}"),
        index,
        path,
        line,
        Rung::Exact,
    )])
}

fn choose(text: &str, located: Located, occurrence: Occurrence) -> Result<Vec<Found>, ProblemKind> {
    let found = match located {
        Located::Missing(nearest) => return Err(ProblemKind::Missing { nearest }),
        Located::Found(found) => found,
    };
    match occurrence {
        Occurrence::All => Ok(found),
        Occurrence::Unique if found.len() == 1 => Ok(found),
        Occurrence::Unique => Err(ProblemKind::Ambiguous {
            lines: found.iter().map(|f| line_of(text, f.range.start)).collect(),
        }),
        Occurrence::Nth(n) => found
            .get(n.wrapping_sub(1))
            .cloned()
            .map(|found| vec![found])
            .ok_or_else(|| ProblemKind::Invalid {
                message: format!("occurrence {n} requested but {} matched", found.len()),
            }),
    }
}

fn replacement(replace: &str, found: &Found) -> String {
    match &found.indent {
        Some((from, to)) => reindent(replace, from, to),
        None => replace.to_string(),
    }
}

fn edit(id: String, index: usize, path: &Path, line: usize, rung: Rung) -> Edit {
    Edit {
        id,
        op: index,
        path: path.to_path_buf(),
        line,
        rung,
        confidence: rung.confidence(),
    }
}
