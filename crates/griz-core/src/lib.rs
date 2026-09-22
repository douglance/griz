//! Pure edit engine for griz.
//!
//! Everything here computes; nothing here writes. A caller hands the engine
//! operations and a way to read files, and gets back a [`Plan`]: the exact
//! before and after text of every file, one [`Edit`] per change with the
//! [`Confidence`] it was matched at, and every [`Problem`] that stopped an
//! operation. Writing a plan to disk belongs to `griz-store`.

mod anchor_input;
mod definitions;
mod diff;
mod edits;
mod find;
mod find_position;
mod hash;
mod matcher;
mod merge;
mod model;
mod overlay;
mod patch;
mod pattern_edit;
mod pattern_locator;
mod plan;
mod position;
mod problem;
mod reindent;
mod scope;
mod source;
mod splice;
mod structural;
mod syntax;
mod uri;
mod workspace_edit;
mod workspace_edit_ops;

pub use definitions::{DiffItem, ItemChange, diff_items};
pub use diff::{FileDiff, render_diff};
pub use find::{FindPage, FindQuery, Match, find};
pub use hash::content_hash;
pub use merge::{MergeOutcome, three_way};
pub use model::{
    Anchor, ByteRange, ChangeKind, Confidence, Edit, FileChange, Occurrence, Op, Plan, Problem,
    ProblemKind, Rung, Window,
};
pub use patch::{PatchError, parse_patch};
pub use pattern_locator::PatternLocator;
pub use plan::build_plan;
pub use position::{Position, PositionEncoding};
pub use source::{DiskSource, MapSource, Source};
pub use syntax::{FileSyntax, PlanSyntax, SyntaxPosition, annotate, plan_syntax};
pub use workspace_edit::{
    CreateFile, DeleteFile, DocumentChange, Range, RenameFile, TextDocumentEdit,
    TextDocumentIdentifier, TextEdit, WorkspaceEdit, WorkspaceEditError,
};
pub use workspace_edit_ops::to_ops as workspace_edit_to_ops;
