//! Publishes `plan`'s MCP input schema, typed from [`Op`] and
//! [`WorkspaceEdit`] instead of hand-derived, so a new field on either shows
//! up here automatically.

use griz_core::{Op, WorkspaceEdit};
use serde_json::{Map, Value, json};

/// The typed schema for `plan`'s `ops`, `workspace_edit`, and plain options.
/// `$defs` schemars nests under `Op` and `WorkspaceEdit` (`Anchor`,
/// `ByteRange`, `Occurrence`, `PatternLocator`, `TextEdit`, ...) are hoisted
/// to this schema's own root so every `$ref` resolves within the published
/// document.
pub fn plan_input_schema() -> Value {
    let (op_schema, op_defs) = schema_and_defs::<Op>();
    let (edit_schema, edit_defs) = schema_and_defs::<WorkspaceEdit>();
    let mut defs = op_defs;
    defs.extend(edit_defs);
    json!({
        "type": "object",
        "properties": {
            "ops": {
                "type": "array",
                "description": "Operations as JSON objects, applied in order. Each is a replace, insert, create, delete, or move (or JSON text for CLI callers).",
                "items": {
                    "oneOf": [op_schema, { "type": "string" }],
                },
            },
            "patch": {
                "type": "string",
                "description": "Codex patch text, or `@path` to read the patch from a file.",
            },
            "workspace_edit": edit_schema,
            "position_encoding": {
                "type": "string",
                "enum": ["utf-8", "utf-16", "utf-32"],
                "description": "How workspace_edit positions count into a line. Defaults to utf-16.",
            },
            "root": {
                "type": "string",
                "description": "Directory relative paths resolve from. Defaults to the current directory.",
            },
            "purpose": {
                "type": "string",
                "description": "Why this plan is being made.",
            },
            "idempotency_key": {
                "type": "string",
                "description": "Key that makes a retry return the original plan instead of a new one.",
            },
            "expect_edits": {
                "type": "number",
                "description": "Number of edits the plan must contain.",
            },
            "expect_files": {
                "type": "number",
                "description": "Number of files the plan must change.",
            },
            "verbosity": {
                "type": "string",
                "description": "Response detail: off, error, warn, info, debug, or trace.",
            },
        },
        "required": ["purpose", "idempotency_key"],
        "$defs": defs,
    })
}

/// A type's schema with `$defs`/`$schema` split off, ready to nest under a
/// bigger document's own root.
fn schema_and_defs<T: schemars::JsonSchema>() -> (Value, Map<String, Value>) {
    let mut schema = serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|e| unreachable!("schema derives JsonSchema: {e}"));
    let defs = schema
        .as_object_mut()
        .and_then(|schema| schema.remove("$defs"))
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if let Some(schema) = schema.as_object_mut() {
        schema.remove("$schema");
    }
    (schema, defs)
}
