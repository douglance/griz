//! CLI examples shared with generated command skills.

use incurs::command::Example;

/// Arguments after the command name, which help and skills prepend.
pub fn example(command: &str, description: &str) -> Vec<Example> {
    vec![Example {
        command: command.into(),
        description: Some(description.into()),
    }]
}

/// Guards needed before using a mutation's returned identifier.
pub const MUTATION: &str = "CLI programs: pass --format json, check exit status and outcome == passed, then read id. On failure, preserve stdout/stderr; use get ID for a saved record.";

/// Scalar array options use repeated flags on the CLI.
pub const PATHS: &str = "CLI: repeat --paths for each path, such as --paths 'src one' --paths 'src two'. MCP: pass paths as a JSON array. Quote paths containing spaces.";
