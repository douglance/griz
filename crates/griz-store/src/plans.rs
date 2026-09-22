//! Plan records: the operations, the edits they produced, and every file's
//! before and after text by fingerprint.

use crate::{Store, StoreError, new_id, now_ms};
use griz_core::{
    ChangeKind, Confidence, Edit, FileChange, MapSource, Op, Plan, Problem, build_plan,
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
    /// Fingerprints of unchanged input files; `None` records an absent file.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unchanged_inputs: BTreeMap<PathBuf, Option<String>>,
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
        let confidence = plan.confidence();
        let put_blob = |text: &str| self.put_blob(text);
        let unchanged_inputs = plan
            .unchanged_inputs
            .into_iter()
            .map(|(path, text)| {
                let hash = text.as_deref().map(put_blob).transpose()?;
                Ok((path, hash))
            })
            .collect::<Result<_, StoreError>>()?;
        let record = PlanRecord {
            id: new_id("plan"),
            purpose: purpose.to_string(),
            selected_from,
            confidence,
            ops,
            edits: plan.edits,
            problems: plan.problems,
            files,
            unchanged_inputs,
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

    fn plan_source(&self, record: &PlanRecord, ops: &[Op]) -> Result<MapSource, StoreError> {
        let needed = (ops.len() != record.ops.len()).then(|| input_paths(ops));
        let mut files = record
            .unchanged_inputs
            .iter()
            .filter(|(path, _)| {
                needed
                    .as_ref()
                    .is_none_or(|paths| paths.contains(path.as_path()))
            })
            .map(|(path, hash)| Ok((path.clone(), self.text(hash.as_deref())?)))
            .collect::<Result<BTreeMap<_, _>, StoreError>>()?;
        for file in record.files.iter().filter(|file| {
            needed
                .as_ref()
                .is_none_or(|paths| paths.contains(file.path.as_path()))
        }) {
            files.insert(file.path.clone(), self.text(file.before_hash.as_deref())?);
        }
        Ok(MapSource::new(files))
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
        let ops = selected_ops(&original, selection);
        let source = self.plan_source(&original, &ops)?;
        let plan = build_plan(&ops, &source);
        self.store_plan(purpose, ops, plan, Some(original.id))
    }
}

fn input_paths(ops: &[Op]) -> BTreeSet<&Path> {
    let mut paths = BTreeSet::new();
    for op in ops {
        paths.insert(op.path().as_path());
        if let Op::Move { to, .. } = op {
            paths.insert(to.as_path());
        }
    }
    paths
}

struct Kept {
    by_path: bool,
    by_edit: bool,
    by_confidence: bool,
}

fn selected_ops(plan: &PlanRecord, selection: &Selection) -> Vec<Op> {
    let paths: BTreeSet<&Path> = selection.paths.iter().map(PathBuf::as_path).collect();
    let edits: BTreeSet<&str> = selection.edits.iter().map(String::as_str).collect();
    let mut kept: Vec<_> = plan
        .ops
        .iter()
        .map(|op| Kept {
            by_path: paths.is_empty() || paths.contains(op.path().as_path()),
            by_edit: edits.is_empty(),
            by_confidence: true,
        })
        .collect();
    for edit in &plan.edits {
        let trusted = selection
            .min_confidence
            .is_none_or(|min| edit.confidence >= min);
        let Some(kept) = kept.get_mut(edit.op) else {
            continue;
        };
        kept.by_path |= paths.contains(edit.path.as_path());
        kept.by_edit |= edits.contains(edit.id.as_str());
        kept.by_confidence &= trusted;
    }
    plan.ops
        .iter()
        .zip(kept)
        .filter(|(_, kept)| kept.by_path && kept.by_edit && kept.by_confidence)
        .map(|(op, _)| op.clone())
        .collect()
}
