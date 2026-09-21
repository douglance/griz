//! Truthful MCP annotations for each kind of command.

use incurs::command::{McpAnnotations, McpCommandOptions};

/// Reads only.
pub fn read_only() -> McpCommandOptions {
    McpCommandOptions {
        annotations: Some(McpAnnotations {
            read_only_hint: Some(true),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
            ..McpAnnotations::default()
        }),
        ..McpCommandOptions::default()
    }
}

/// Records state but never changes a source file; keyed retries replay.
pub fn records() -> McpCommandOptions {
    McpCommandOptions {
        annotations: Some(McpAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
            ..McpAnnotations::default()
        }),
        ..McpCommandOptions::default()
    }
}

/// Writes source files.
pub fn writes_files() -> McpCommandOptions {
    McpCommandOptions {
        destructive: true,
        annotations: Some(McpAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(true),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
            ..McpAnnotations::default()
        }),
        ..McpCommandOptions::default()
    }
}
