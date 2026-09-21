//! The verdict every mutation answers with, and how much detail comes back.
//!
//! A mutation answers `{id, outcome}` and nothing else unless the caller asks.
//! Raising the verbosity only ever adds keys; `trace` answers with the complete
//! record and still carries `outcome`, so no level costs a caller the verdict.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The result of a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Done, and every declared expectation held.
    Passed,
    /// Done, but a declared expectation did not hold.
    Failed,
    /// Nothing was done: the request could not reach a verdict.
    Error,
}

/// How much a response carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    /// Verdict only.
    Off,
    /// Verdict only. The default.
    Error,
    /// Adds the reason when the outcome is not `passed`.
    Warn,
    /// Adds a summary.
    Info,
    /// Adds per-edit and per-file detail.
    Debug,
    /// The complete record, plus `outcome`.
    Trace,
}

/// Environment variable supplying a default verbosity.
pub const VERBOSITY_ENV: &str = "GRIZ_VERBOSITY";

impl Verbosity {
    /// Resolves the explicit option, then `GRIZ_VERBOSITY`, then `error`.
    ///
    /// # Errors
    /// Returns a message naming the accepted levels.
    pub fn resolve(explicit: Option<&str>) -> Result<Self, String> {
        let env = std::env::var(VERBOSITY_ENV).ok();
        match explicit.or(env.as_deref()).unwrap_or("error") {
            "off" => Ok(Self::Off),
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            other => Err(format!(
                "unknown verbosity `{other}`; use off, error, warn, info, debug, or trace"
            )),
        }
    }
}

/// Everything a response may carry, before verbosity trims it.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// Record identifier.
    pub id: String,
    /// Verdict.
    pub outcome: Outcome,
    /// Why the outcome is not `passed`.
    pub reason: Option<String>,
    /// Counts and short facts, for `info`.
    pub summary: Value,
    /// Per-edit and per-file detail, for `debug`.
    pub detail: Value,
    /// The complete record, for `trace`.
    pub record: Value,
}

impl Rendered {
    /// The response at `level`.
    #[must_use]
    pub fn at(&self, level: Verbosity, replayed: bool) -> Value {
        if level == Verbosity::Trace {
            return self.traced(replayed);
        }
        let mut out = Map::new();
        out.insert("id".into(), json!(self.id));
        out.insert("outcome".into(), json!(self.outcome));
        insert_replayed(&mut out, replayed);
        let reason = self
            .reason
            .as_ref()
            .filter(|_| self.outcome != Outcome::Passed);
        if let Some(reason) = reason.filter(|_| level >= Verbosity::Warn) {
            out.insert("reason".into(), json!(reason));
        }
        if level >= Verbosity::Info {
            out.insert("summary".into(), self.summary.clone());
        }
        if level >= Verbosity::Debug {
            out.insert("detail".into(), self.detail.clone());
        }
        Value::Object(out)
    }

    fn traced(&self, replayed: bool) -> Value {
        let mut record = self.record.clone();
        if let Value::Object(map) = &mut record {
            map.insert("outcome".into(), json!(self.outcome));
            insert_replayed(map, replayed);
        }
        record
    }
}

fn insert_replayed(map: &mut Map<String, Value>, replayed: bool) {
    if replayed {
        map.insert("replayed".into(), json!(true));
    }
}

/// A declared count and the count observed.
#[must_use]
pub fn unmet(label: &str, expected: Option<usize>, actual: usize) -> Option<String> {
    let expected = expected?;
    (expected != actual).then(|| format!("expected {expected} {label}, observed {actual}"))
}
