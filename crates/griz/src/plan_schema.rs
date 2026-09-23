//! Publishes `plan`'s MCP input schema from CLI option metadata and typed edits.
//! Edit definitions stay local to the published schema.

use griz_core::{Op, WorkspaceEdit};
use incurs::schema::{IncurSchema, to_json_schema};
use serde_json::{Map, Value, json};

/// The typed schema for `plan`'s `ops`, `workspace_edit`, and plain options.
/// `$defs` schemars nests under `Op` and `WorkspaceEdit` (`Anchor`,
/// `ByteRange`, `Occurrence`, `PatternLocator`, `TextEdit`, ...) are hoisted
/// to this schema's own root so every `$ref` resolves within the published
/// document.
pub fn plan_input_schema<Options: IncurSchema>() -> Value {
    let mut schema = option_schema::<Options>();
    let (op_schema, op_defs) = schema_and_defs::<Op>();
    let (edit_schema, edit_defs) = schema_and_defs::<WorkspaceEdit>();
    let mut defs = op_defs;
    defs.extend(edit_defs);
    let properties = &mut schema["properties"];
    properties["ops"] = json!({
        "type": "array",
        "description": "Operations as JSON objects, applied in order. Each is a replace, insert, create, delete, or move (or JSON text for CLI callers).",
        "items": { "oneOf": [op_schema, { "type": "string" }] },
    });
    properties["workspace_edit"] = edit_schema;
    properties["position_encoding"]["enum"] = json!(["utf-8", "utf-16", "utf-32"]);
    properties["expect_syntax"]["enum"] = json!(["clean"]);
    schema["$defs"] = json!(defs);
    schema
}

fn option_schema<Options: IncurSchema>() -> Value {
    let mut fields = Options::fields();
    for field in &mut fields {
        field.cli_name = field.name.to_owned();
    }
    to_json_schema(&fields)
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
