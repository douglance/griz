//! Shared fixtures.

use griz_core::{Anchor, DiskSource, Occurrence, Op, build_plan};
use griz_store::{ApplyRequest, OnStale, PlanRecord, Store, UndoRequest};
use std::{
    error::Error,
    path::{Path, PathBuf},
};

pub type TestResult = Result<(), Box<dyn Error>>;

/// A store and a workspace directory, both temporary.
pub struct Fixture {
    pub store: Store,
    pub work: tempfile::TempDir,
    _home: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let home = tempfile::tempdir()?;
        let work = tempfile::tempdir()?;
        Ok(Self {
            store: Store::open(home.path())?,
            work,
            _home: home,
        })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.work.path().join(name)
    }

    pub fn write(&self, name: &str, text: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.path(name), text)?;
        Ok(())
    }

    pub fn read(&self, name: &str) -> Result<String, Box<dyn Error>> {
        Ok(std::fs::read_to_string(self.path(name))?)
    }

    pub fn plan(&self, ops: Vec<Op>) -> Result<PlanRecord, Box<dyn Error>> {
        let plan = build_plan(&ops, &DiskSource);
        Ok(self.store.save_plan("test", ops, plan)?)
    }

    pub fn replace(&self, name: &str, find: &str, with: &str) -> Op {
        Op::Replace {
            path: self.path(name),
            find: Some(Anchor {
                text: find.to_string(),
                after: None,
                whole_lines: false,
            }),
            range: None,
            replace: with.to_string(),
            occurrence: Occurrence::Unique,
            expect_hash: None,
        }
    }
}

/// A default apply request for `plan`.
pub fn request(plan: &PlanRecord) -> ApplyRequest {
    ApplyRequest {
        plan: plan.id.clone(),
        min_confidence: griz_core::Confidence::Machine,
        on_stale: OnStale::Refuse,
        purpose: "test".to_string(),
    }
}

/// A default undo request restoring every file, refusing on any stale one.
pub fn undo_request(operation: &str) -> UndoRequest {
    UndoRequest {
        operation: operation.to_string(),
        paths: Vec::new(),
        on_stale: OnStale::Refuse,
        purpose: "test".to_string(),
    }
}
/// Whether `path` exists.
pub fn exists(path: &Path) -> bool {
    path.exists()
}
