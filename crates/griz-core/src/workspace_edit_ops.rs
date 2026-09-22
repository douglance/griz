//! Turning a workspace edit's document changes into griz operations.

use crate::{
    ByteRange, Occurrence, Op, Source, content_hash,
    position::{self, Position, PositionEncoding},
    uri,
    workspace_edit::{DocumentChange, Range, TextEdit, WorkspaceEdit, WorkspaceEditError},
};
use std::path::PathBuf;

/// Converts `edit` into operations, resolving positions against each file's
/// text as read through `source`. Every text operation carries `expect_hash`
/// for the text it was converted against, so a file changed since is refused
/// as stale when the plan is built. `document_changes` is preferred over
/// `changes` when both are given, matching the LSP client preference.
///
/// # Errors
/// Returns an error for a URI that is not `file://`, a file that cannot be
/// read, or a position outside the file.
pub fn to_ops(
    edit: &WorkspaceEdit,
    encoding: PositionEncoding,
    source: &dyn Source,
) -> Result<Vec<Op>, WorkspaceEditError> {
    if let Some(changes) = &edit.document_changes {
        return flatten(
            changes
                .iter()
                .map(|change| document_change_ops(change, encoding, source)),
        );
    }
    match &edit.changes {
        Some(changes) => flatten(
            changes
                .iter()
                .map(|(uri, edits)| text_edit_ops(uri, edits, encoding, source)),
        ),
        None => Ok(Vec::new()),
    }
}

fn flatten(
    results: impl Iterator<Item = Result<Vec<Op>, WorkspaceEditError>>,
) -> Result<Vec<Op>, WorkspaceEditError> {
    let mut ops = Vec::new();
    for result in results {
        ops.extend(result?);
    }
    Ok(ops)
}

fn document_change_ops(
    change: &DocumentChange,
    encoding: PositionEncoding,
    source: &dyn Source,
) -> Result<Vec<Op>, WorkspaceEditError> {
    match change {
        DocumentChange::Edit(edit) => {
            text_edit_ops(&edit.text_document.uri, &edit.edits, encoding, source)
        }
        DocumentChange::Create(create) => Ok(vec![Op::Create {
            path: path_of(&create.uri)?,
            text: String::new(),
            overwrite: create.options.as_ref().is_some_and(|o| o.overwrite),
        }]),
        DocumentChange::Rename(rename) => Ok(vec![Op::Move {
            path: path_of(&rename.old_uri)?,
            to: path_of(&rename.new_uri)?,
            expect_hash: None,
        }]),
        DocumentChange::Delete(delete) => Ok(vec![Op::Delete {
            path: path_of(&delete.uri)?,
            expect_hash: None,
        }]),
    }
}

fn text_edit_ops(
    uri: &str,
    edits: &[TextEdit],
    encoding: PositionEncoding,
    source: &dyn Source,
) -> Result<Vec<Op>, WorkspaceEditError> {
    let path = path_of(uri)?;
    let text = read(&path, source)?;
    let hash = content_hash(&text);
    let positions: Vec<_> = edits
        .iter()
        .flat_map(|edit| [edit.range.start, edit.range.end])
        .collect();
    let offsets = position::to_bytes(&text, &positions, encoding);
    edits
        .iter()
        .zip(offsets.chunks_exact(2))
        .map(|(edit, offsets)| text_edit_op(&path, edit, offsets, &hash))
        .collect()
}

fn read(path: &std::path::Path, source: &dyn Source) -> Result<String, WorkspaceEditError> {
    source
        .read(path)
        .map_err(|message| unreadable(path, message))?
        .ok_or_else(|| unreadable(path, "file not found".to_string()))
}

fn unreadable(path: &std::path::Path, message: String) -> WorkspaceEditError {
    WorkspaceEditError::Unreadable {
        path: path.to_path_buf(),
        message,
    }
}

fn text_edit_op(
    path: &std::path::Path,
    edit: &TextEdit,
    offsets: &[Option<usize>],
    hash: &str,
) -> Result<Op, WorkspaceEditError> {
    let range = byte_range(path, edit.range, offsets)?;
    Ok(Op::Replace {
        path: path.to_path_buf(),
        find: None,
        range: Some(range),
        pattern: None,
        replace: edit.new_text.clone(),
        occurrence: Occurrence::default(),
        target: None,
        expect_hash: Some(hash.to_string()),
    })
}

fn byte_range(
    path: &std::path::Path,
    range: Range,
    offsets: &[Option<usize>],
) -> Result<ByteRange, WorkspaceEditError> {
    let start = offsets[0].ok_or_else(|| bad(path, range.start))?;
    let end = offsets[1].ok_or_else(|| bad(path, range.end))?;
    Ok(ByteRange { start, end })
}

fn bad(path: &std::path::Path, position: Position) -> WorkspaceEditError {
    WorkspaceEditError::BadPosition {
        path: path.to_path_buf(),
        line: position.line,
        character: position.character,
    }
}

fn path_of(target: &str) -> Result<PathBuf, WorkspaceEditError> {
    uri::to_path(target).map_err(|uri| WorkspaceEditError::BadUri { uri })
}
