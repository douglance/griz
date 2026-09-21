//! Plan records: the operations, the edits they produced, and every file's
//! before and after text by fingerprint.

use crate::{Store, StoreError, new_id, now_ms};
use griz_core::{
    ChangeKind, Confidence, DiskSource, Edit, FileChange, Op, Plan, Problem, Source, build_plan,
};
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// One changed file in a stored plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileRecord {
    /// File path.
    pub path: PathBuf,
    /// What happens to it.
    pub kind: ChangeKind,
    /// Fingerprint the plan was computed against.
    pub before_hash: Option<String>,
    /// Fingerprint the plan writes.
    pub after_hash: Option<String>,
}

/// A stored plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanRecord {
    /// Identifier.
    pub id: String,
    /// Why the caller asked for it.
    pub purpose: String,
    /// Plan this one was selected from, if any.
    pub selected_from: Option<String>,
    /// Operations, in order.
    pub ops: Vec<Op>,
    /// Edits produced.
    pub edits: Vec<Edit>,
    /// Operations that could not be planned.
    pub problems: Vec<Problem>,
    /// Changed files.
    pub files: Vec<FileRecord>,
    /// Lowest edit confidence.
    pub confidence: Confidence,
    /// Creation time in Unix milliseconds.
    pub created_at: i64,
}

/// Which part of a plan to keep.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Selection {
    /// Keep operations touching these paths; empty keeps all.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    /// Keep operations that produced these edit ids; empty keeps all.
    #[serde(default)]
    pub edits: Vec<String>,
    /// Keep operations whose every edit is at least this confident.
    pub min_confidence: Option<Confidence>,
}

impl Store {
    /// Stores a plan built from `ops`.
    ///
    /// # Errors
    /// Returns an error when a file text or the record cannot be stored.
    pub fn save_plan(
        &self,
        purpose: &str,
        ops: Vec<Op>,
        plan: Plan,
    ) -> Result<PlanRecord, StoreError> {
        self.store_plan(purpose, ops, plan, None)
    }

    fn store_plan(
        &self,
        purpose: &str,
        ops: Vec<Op>,
        plan: Plan,
        selected_from: Option<String>,
    ) -> Result<PlanRecord, StoreError> {
        let files = plan
            .files
            .iter()
            .map(|file| self.file_record(file))
            .collect::<Result<_, _>>()?;
        let record = PlanRecord {
            id: new_id("plan"),
            purpose: purpose.to_string(),
            selected_from,
            confidence: plan.confidence(),
            ops,
            edits: plan.edits,
            problems: plan.problems,
            files,
            created_at: now_ms(),
        };
        self.insert_plan(&record)?;
        Ok(record)
    }

    fn file_record(&self, file: &FileChange) -> Result<FileRecord, StoreError> {
        if let Some(text) = &file.before {
            self.put_blob(text)?;
        }
        if let Some(text) = &file.after {
            self.put_blob(text)?;
        }
        Ok(FileRecord {
            path: file.path.clone(),
            kind: file.kind,
            before_hash: file.before_hash.clone(),
            after_hash: file.after_hash.clone(),
        })
    }

    fn insert_plan(&self, record: &PlanRecord) -> Result<(), StoreError> {
        let body = serde_json::to_string(record)?;
        self.with(|conn| {
            conn.execute(
                "INSERT INTO plans (id, created_at, body) VALUES (?1, ?2, ?3)",
                params![record.id, record.created_at, body],
            )?;
            Ok(())
        })
    }

    /// Reads one plan record.
    ///
    /// # Errors
    /// Returns `NotFound` when no plan has this identifier.
    pub fn plan(&self, id: &str) -> Result<PlanRecord, StoreError> {
        let sql = "SELECT body FROM plans WHERE id = ?1";
        let body = self.with(|conn| crate::body_by_id(conn, sql, id))?;
        let body = body.ok_or_else(|| StoreError::NotFound(format!("plan {id}")))?;
        Ok(serde_json::from_str(&body)?)
    }

    /// The before and after text of every file in a plan.
    ///
    /// # Errors
    /// Returns an error when a recorded text is missing.
    pub fn plan_changes(&self, record: &PlanRecord) -> Result<Vec<FileChange>, StoreError> {
        record
            .files
            .iter()
            .map(|file| {
                Ok(FileChange {
                    path: file.path.clone(),
                    kind: file.kind,
                    before: self.text(file.before_hash.as_deref())?,
                    before_hash: file.before_hash.clone(),
                    after: self.text(file.after_hash.as_deref())?,
                    after_hash: file.after_hash.clone(),
                })
            })
            .collect()
    }

    fn text(&self, hash: Option<&str>) -> Result<Option<String>, StoreError> {
        hash.map(|hash| self.get_blob(hash)).transpose()
    }

    /// Stores a new plan keeping only the selected operations, rebuilt against
    /// the texts the original plan was computed from.
    ///
    /// # Errors
    /// Returns an error when the original plan or its texts cannot be read.
    pub fn select(
        &self,
        id: &str,
        selection: &Selection,
        purpose: &str,
    ) -> Result<PlanRecord, StoreError> {
        let original = self.plan(id)?;
        let ops: Vec<Op> = original
            .ops
            .iter()
            .enumerate()
            .filter(|(index, op)| keeps(&original, selection, *index, op))
            .map(|(_, op)| op.clone())
            .collect();
        let recorded = self.plan_changes(&original)?;
        let source = Recorded {
            files: recorded
                .into_iter()
                .map(|change| (change.path, change.before))
                .collect(),
        };
        let plan = build_plan(&ops, &source);
        self.store_plan(purpose, ops, plan, Some(original.id))
    }
}

fn keeps(plan: &PlanRecord, selection: &Selection, index: usize, op: &Op) -> bool {
    let edits: Vec<&Edit> = plan.edits.iter().filter(|edit| edit.op == index).collect();
    let touched: BTreeSet<&Path> = std::iter::once(op.path().as_path())
        .chain(edits.iter().map(|edit| edit.path.as_path()))
        .collect();
    let by_path = selection.paths.is_empty()
        || selection
            .paths
            .iter()
            .any(|path| touched.contains(path.as_path()));
    let by_edit =
        selection.edits.is_empty() || edits.iter().any(|edit| selection.edits.contains(&edit.id));
    let by_confidence = selection
        .min_confidence
        .is_none_or(|min| edits.iter().all(|edit| edit.confidence >= min));
    by_path && by_edit && by_confidence
}

/// The texts a plan was computed from, falling back to disk for files the
/// plan read but did not change.
struct Recorded {
    files: BTreeMap<PathBuf, Option<String>>,
}

impl Source for Recorded {
    fn read(&self, path: &Path) -> Result<Option<String>, String> {
        match self.files.get(path) {
            Some(text) => Ok(text.clone()),
            None => DiskSource.read(path),
        }
    }
}
