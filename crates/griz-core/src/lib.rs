//! Pure edit engine for griz.
//!
//! Everything here computes; nothing here writes. A caller hands the engine
//! operations and a way to read files, and gets back a [`Plan`]: the exact
//! before and after text of every file, one [`Edit`] per change with the
//! [`Confidence`] it was matched at, and every [`Problem`] that stopped an
//! operation. Writing a plan to disk belongs to `griz-store`.

mod anchor_input;
mod diff;
mod edits;
mod find;
mod hash;
mod matcher;
mod merge;
mod model;
mod overlay;
mod patch;
mod plan;
mod reindent;
mod source;
mod splice;
mod structural;

pub use diff::{FileDiff, render_diff};
pub use find::{FindPage, FindQuery, Match, find};
pub use hash::content_hash;
pub use merge::{MergeOutcome, three_way};
pub use model::{
    Anchor, ByteRange, ChangeKind, Confidence, Edit, FileChange, Occurrence, Op, Plan, Problem,
    ProblemKind, Rung, Window,
};
pub use patch::{PatchError, parse_patch};
pub use plan::build_plan;
pub use source::{DiskSource, MapSource, Source};
