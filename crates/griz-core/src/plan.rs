//! Turns operations into a [`Plan`] without writing anything.

use crate::{
    Edit, Op, Plan, Problem, ProblemKind, Rung, Source, edits, overlay::Overlay, pattern_edit,
    syntax,
};
use std::path::Path;

/// Plans `ops` in order against `source`.
///
/// Every operation is attempted, so the plan reports every problem at once
/// rather than stopping at the first. Later operations see the effect of
/// earlier ones, which is how several blocks for one file compose.
#[must_use]
pub fn build_plan(ops: &[Op], source: &dyn Source) -> Plan {
    let mut overlay = Overlay::new(source);
    let mut plan = Plan::default();
    for (index, op) in ops.iter().enumerate() {
        match plan_op(&mut overlay, index, op) {
            Ok(edits) => plan.edits.extend(edits),
            Err(kind) => plan.problems.push(Problem {
                op: index,
                path: op.path().clone(),
                kind,
            }),
        }
    }
    overlay.finish(&mut plan);
    plan.file_syntax = syntax::annotate(&plan.files);
    plan.syntax = syntax::plan_syntax(&plan.file_syntax);
    plan
}

/// Result of planning one operation.
pub type OpResult = Result<Vec<Edit>, ProblemKind>;

fn plan_op(overlay: &mut Overlay<'_>, index: usize, op: &Op) -> OpResult {
    reset_position(overlay, op);
    match op {
        Op::Replace { .. } => plan_replace(overlay, index, op),
        Op::Insert {
            path,
            anchor,
            after,
            text,
            expect_hash,
        } => {
            guard(overlay, path, expect_hash.as_deref())?;
            let slot = overlay.slot(path).map_err(invalid)?;
            edits::insert(slot, index, path, anchor, (*after, text))
        }
        Op::Create {
            path,
            text,
            overwrite,
        } => create(overlay, index, path, text, *overwrite),
        Op::Delete { path, expect_hash } => {
            guard(overlay, path, expect_hash.as_deref())?;
            let slot = overlay.slot(path).map_err(invalid)?;
            slot.current.take().ok_or(ProblemKind::NotFound)?;
            Ok(vec![file_edit(format!("e{index}"), index, path)])
        }
        Op::Move {
            path,
            to,
            expect_hash,
        } => {
            guard(overlay, path, expect_hash.as_deref())?;
            move_file(overlay, index, path, to)
        }
    }
}

fn reset_position(overlay: &mut Overlay<'_>, op: &Op) {
    if matches!(op, Op::Replace { range: Some(_), .. }) {
        return;
    }
    overlay.reset_position(op.path());
    if let Op::Move { to, .. } = op {
        overlay.reset_position(to);
    }
}

fn plan_replace(overlay: &mut Overlay<'_>, index: usize, op: &Op) -> OpResult {
    let Op::Replace {
        path,
        find,
        range,
        pattern,
        replace,
        occurrence,
        target,
        expect_hash,
    } = op
    else {
        return Err(invalid("not a replace operation".to_string()));
    };
    guard(overlay, path, expect_hash.as_deref())?;
    if target.is_some() && pattern.is_none() {
        return Err(invalid("`target` requires `pattern`".to_string()));
    }
    let slot = overlay.slot(path).map_err(invalid)?;
    if let Some(locator) = pattern {
        if range.is_some() || find.is_some() {
            return Err(invalid(
                "give exactly one of `range`, `find`, or `pattern`".to_string(),
            ));
        }
        let spec = pattern_edit::PatternSpec {
            locator,
            replace,
            occurrence: *occurrence,
            target: target.as_deref(),
        };
        return pattern_edit::replace_pattern(slot, index, path, &spec);
    }
    if let Some(range) = range {
        require_range_guard(expect_hash.as_deref(), find.as_ref())?;
        return edits::replace_range(slot, index, path, *range, (find.as_ref(), replace));
    }
    let anchor = find
        .as_ref()
        .ok_or_else(|| invalid("replace needs `find`, `range`, or `pattern`".to_string()))?;
    let spec = edits::ReplaceSpec {
        anchor,
        replace,
        occurrence: *occurrence,
    };
    edits::replace_anchor(slot, index, path, &spec)
}

fn require_range_guard(
    expected: Option<&str>,
    anchor: Option<&crate::Anchor>,
) -> Result<(), ProblemKind> {
    if expected.is_none() && anchor.is_none() {
        return Err(invalid(
            "range needs `expect_hash` from read/find or expected old `find` text".to_string(),
        ));
    }
    Ok(())
}

fn invalid(message: String) -> ProblemKind {
    ProblemKind::Invalid { message }
}

/// Rejects an operation whose file no longer has the caller's fingerprint.
fn guard(
    overlay: &mut Overlay<'_>,
    path: &Path,
    expected: Option<&str>,
) -> Result<(), ProblemKind> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let slot = overlay.slot(path).map_err(invalid)?;
    let actual = &slot.before_hash;
    if actual.as_deref() == Some(expected) {
        return Ok(());
    }
    Err(ProblemKind::Stale {
        expected: expected.to_string(),
        actual: actual.clone(),
    })
}

fn create(
    overlay: &mut Overlay<'_>,
    index: usize,
    path: &Path,
    text: &str,
    overwrite: bool,
) -> OpResult {
    let slot = overlay.slot(path).map_err(invalid)?;
    if slot.current.is_some() && !overwrite {
        return Err(ProblemKind::Exists);
    }
    slot.current = Some(text.to_string());
    Ok(vec![file_edit(format!("e{index}"), index, path)])
}

fn move_file(overlay: &mut Overlay<'_>, index: usize, path: &Path, to: &Path) -> OpResult {
    if overlay.slot(to).map_err(invalid)?.current.is_some() {
        return Err(ProblemKind::Exists);
    }
    let text = overlay
        .slot(path)
        .map_err(invalid)?
        .current
        .take()
        .ok_or(ProblemKind::NotFound)?;
    overlay.slot(to).map_err(invalid)?.current = Some(text);
    Ok(vec![
        file_edit(format!("e{index}.from"), index, path),
        file_edit(format!("e{index}.to"), index, to),
    ])
}

fn file_edit(id: String, index: usize, path: &Path) -> Edit {
    Edit {
        id,
        op: index,
        path: path.to_path_buf(),
        line: 0,
        rung: Rung::File,
        confidence: Rung::File.confidence(),
    }
}
