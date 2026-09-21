//! Turns operations into a [`Plan`] without writing anything.

use crate::{
    Edit, Op, Plan, Problem, ProblemKind, Rung, Source, content_hash, edits, overlay::Overlay,
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
    plan.files = overlay.into_changes();
    plan.file_syntax = syntax::annotate(&plan.files);
    plan.syntax = syntax::plan_syntax(&plan.file_syntax);
    plan
}

/// Result of planning one operation.
pub type OpResult = Result<Vec<Edit>, ProblemKind>;

fn plan_op(overlay: &mut Overlay<'_>, index: usize, op: &Op) -> OpResult {
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
        Op::Create { path, text } => create(overlay, index, path, text),
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

fn plan_replace(overlay: &mut Overlay<'_>, index: usize, op: &Op) -> OpResult {
    let Op::Replace {
        path,
        find,
        range,
        replace,
        occurrence,
        expect_hash,
    } = op
    else {
        return Err(invalid("not a replace operation".to_string()));
    };
    guard(overlay, path, expect_hash.as_deref())?;
    let slot = overlay.slot(path).map_err(invalid)?;
    if let Some(range) = range {
        return edits::replace_range(slot, index, path, *range, (find.as_ref(), replace));
    }
    let anchor = find
        .as_ref()
        .ok_or_else(|| invalid("replace needs `find` or `range`".to_string()))?;
    let spec = edits::ReplaceSpec {
        anchor,
        replace,
        occurrence: *occurrence,
    };
    edits::replace_anchor(slot, index, path, &spec)
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
    let actual = slot.before.as_deref().map(content_hash);
    if actual.as_deref() == Some(expected) {
        return Ok(());
    }
    Err(ProblemKind::Stale {
        expected: expected.to_string(),
        actual,
    })
}

fn create(overlay: &mut Overlay<'_>, index: usize, path: &Path, text: &str) -> OpResult {
    let slot = overlay.slot(path).map_err(invalid)?;
    if slot.current.is_some() {
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
