//! One store holds every repository's history. An undo scoped to one root
//! must never touch another root's file, even when both share the store and
//! a `since` span would otherwise sweep both in by sequence alone.

use crate::common::{TestResult, request};
use griz_core::{Anchor, DiskSource, Occurrence, Op, build_plan};
use griz_store::{OnStale, OperationState, PlanRecord, RestoreScope, Store, UndoRequest};
use std::path::PathBuf;

/// A workspace directory standing in for one repository's root.
struct Root {
    /// Kept alive for its `Drop`; the canonical path is precomputed below.
    _dir: tempfile::TempDir,
    canonical: PathBuf,
}

impl Root {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let canonical = dir.path().canonicalize()?;
        Ok(Self {
            _dir: dir,
            canonical,
        })
    }

    /// The canonical root, matching how `Store::undo` and `restore_since`
    /// compare it against a stored file's already-canonical path.
    fn path(&self) -> PathBuf {
        self.canonical.clone()
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path().join(name)
    }

    fn write(&self, name: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(self.file(name), text)?;
        Ok(())
    }

    fn read(&self, name: &str) -> Result<String, Box<dyn std::error::Error>> {
        Ok(std::fs::read_to_string(self.file(name))?)
    }

    fn replace(&self, name: &str, find: &str, with: &str) -> Op {
        Op::Replace {
            path: self.file(name),
            find: Some(Anchor {
                text: find.to_string(),
                after: None,
                whole_lines: false,
            }),
            range: None,
            pattern: None,
            replace: with.to_string(),
            occurrence: Occurrence::Unique,
            target: None,
            expect_hash: None,
        }
    }

    fn plan(store: &Store, ops: Vec<Op>) -> Result<PlanRecord, Box<dyn std::error::Error>> {
        let plan = build_plan(&ops, &DiskSource);
        Ok(store.save_plan("test", ops, plan)?)
    }
}

#[test]
fn restore_since_from_one_root_never_touches_another_roots_file() -> TestResult {
    let home = tempfile::tempdir()?;
    let store = Store::open(home.path())?;
    let root_a = Root::new()?;
    let root_b = Root::new()?;

    root_a.write("a.txt", "one\n")?;
    root_b.write("b.txt", "two\n")?;

    let plan_a = Root::plan(&store, vec![root_a.replace("a.txt", "one", "ONE")])?;
    let op_a = store.apply(&request(&plan_a))?;
    assert_eq!(op_a.state, OperationState::Applied, "{:?}", op_a.reason);

    // root_b applies after root_a, so its operation's sequence follows
    // op_a's: an unscoped `since` scan would otherwise sweep it in.
    let plan_b = Root::plan(&store, vec![root_b.replace("b.txt", "two", "TWO")])?;
    let op_b = store.apply(&request(&plan_b))?;
    assert_eq!(op_b.state, OperationState::Applied, "{:?}", op_b.reason);

    let restored = store.restore_since(
        &op_a.id,
        RestoreScope {
            root: &root_a.path(),
            paths: &[],
        },
        OnStale::Refuse,
        "undo root a",
    )?;
    assert_eq!(
        restored.state,
        OperationState::Applied,
        "{:?}",
        restored.reason
    );

    assert_eq!(
        root_a.read("a.txt")?,
        "one\n",
        "root a's file should be restored"
    );
    assert_eq!(
        root_b.read("b.txt")?,
        "TWO\n",
        "an undo scoped to root a must never touch root b's file"
    );
    Ok(())
}

#[test]
fn undo_of_an_operation_outside_the_callers_root_is_refused() -> TestResult {
    let home = tempfile::tempdir()?;
    let store = Store::open(home.path())?;
    let root_a = Root::new()?;
    let root_b = Root::new()?;

    root_b.write("b.txt", "two\n")?;
    let plan_b = Root::plan(&store, vec![root_b.replace("b.txt", "two", "TWO")])?;
    let op_b = store.apply(&request(&plan_b))?;
    assert_eq!(op_b.state, OperationState::Applied, "{:?}", op_b.reason);

    let refused = store.undo(&UndoRequest {
        operation: op_b.id.clone(),
        root: root_a.path(),
        paths: Vec::new(),
        on_stale: OnStale::Refuse,
        purpose: "undo from the wrong root".to_string(),
    })?;
    assert_eq!(refused.state, OperationState::Failed);
    assert!(
        refused
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("root"),
        "{:?}",
        refused.reason
    );
    assert_eq!(
        root_b.read("b.txt")?,
        "TWO\n",
        "a cross-root undo must never write"
    );
    Ok(())
}

#[test]
fn restore_since_scoped_to_root_a_still_spans_multiple_directories_under_it() -> TestResult {
    // Multiple directories under the SAME root are not "different repos":
    // an undo scoped to that shared root restores both, matching the CLI's
    // existing multi-directory restore behavior.
    let home = tempfile::tempdir()?;
    let store = Store::open(home.path())?;
    let root = Root::new()?;
    std::fs::create_dir_all(root.file("repo-a"))?;
    std::fs::create_dir_all(root.file("repo-b"))?;

    root.write("repo-a/a.txt", "one\n")?;
    root.write("repo-b/b.txt", "two\n")?;
    let plan = Root::plan(
        &store,
        vec![
            root.replace("repo-a/a.txt", "one", "ONE"),
            root.replace("repo-b/b.txt", "two", "TWO"),
        ],
    )?;
    let applied = store.apply(&request(&plan))?;
    assert_eq!(
        applied.state,
        OperationState::Applied,
        "{:?}",
        applied.reason
    );

    let restored = store.restore_since(
        &applied.id,
        RestoreScope {
            root: &root.path(),
            paths: &[],
        },
        OnStale::Refuse,
        "roll back both",
    )?;
    assert_eq!(
        restored.state,
        OperationState::Applied,
        "{:?}",
        restored.reason
    );
    assert_eq!(root.read("repo-a/a.txt")?, "one\n");
    assert_eq!(root.read("repo-b/b.txt")?, "two\n");
    Ok(())
}
