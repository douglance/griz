//! `read` and `find`: the inputs a program turns into operations.

use crate::{
    annotations,
    cmd_plan::resolve_all,
    context::{CmdError, resolve, root},
    lines::{Address, address, merge_lines},
    verdict::{Outcome, unmet},
};
use griz_core::{FindQuery, content_hash, find};
use incurs::command::{CommandDef, TypedContext, TypedResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize, incurs::Args)]
struct PathArgs {
    /// File to read.
    path: String,
}

#[derive(Deserialize, incurs::Options)]
struct ReadOptions {
    /// Return only lines matching this regular expression.
    grep: Option<String>,
    /// Selected of context around each match.
    #[incurs(default = 0)]
    context: usize,
    /// Return only this inclusive line range, such as 10-40.
    lines: Option<String>,
    /// Directory relative paths resolve from. Defaults to the current directory.
    root: Option<String>,
}

/// The `read` command.
pub fn read_command() -> CommandDef {
    CommandDef::typed::<PathArgs, ReadOptions, (), Value, _, _>(
        "read",
        |ctx: TypedContext<PathArgs, ReadOptions, ()>| async move {
            read(&ctx.args.path, ctx.options).unwrap_or_else(CmdError::result)
        },
    )
    .description("Read a file as numbered lines with its fingerprint. Pass the fingerprint as expect_hash to make an edit refuse a changed file.")
    .examples(crate::usage::example("\"src one.rs\" --root /path/to/repo --format json", "Read a path containing spaces."))
    .hint("CLI: the file path is positional. Read-only commands do not take --purpose or --verbosity.")
    .mcp(annotations::read_only())
    .done()
}

fn read(path: &str, options: ReadOptions) -> Result<TypedResult<Value>, CmdError> {
    let path = resolve(&root(options.root.as_deref())?, Path::new(path))?;
    let text = std::fs::read_to_string(&path).map_err(|e| CmdError {
        code: "NOT_FOUND",
        message: format!("{}: {e}", path.display()),
    })?;
    let selected = address(
        &text,
        &Address {
            grep: options.grep,
            context: options.context,
            lines: options.lines,
        },
    )
    .map_err(CmdError::invalid)?;
    let mut body = json!({ "path": path, "hash": content_hash(&text) });
    merge_lines(&mut body, selected);
    Ok(TypedResult::ok(body))
}

#[derive(Deserialize, incurs::Options)]
struct FindOptions {
    /// Files or directories to search. Defaults to the root. Directories honor .gitignore.
    paths: Option<Vec<String>>,
    /// Glob patterns a file must match, such as **/*.rs. Prefix ! to exclude.
    glob: Option<Vec<String>>,
    /// Literal text to find.
    literal: Option<String>,
    /// Regular expression to find; capture groups are returned.
    regex: Option<String>,
    /// Structural pattern with $VAR and $$$VARS metavariables, such as foo($A, $$$REST).
    pattern: Option<String>,
    /// Language for a structural pattern: rust, typescript, tsx, javascript, python, go, or swift.
    /// Defaults to each file's extension.
    language: Option<String>,
    /// Matches to skip, for paging.
    #[incurs(default = 0)]
    offset: usize,
    /// Most matches to return.
    #[incurs(default = 200)]
    limit: usize,
    /// Total matches expected across every file; the outcome fails otherwise.
    expect_matches: Option<usize>,
    /// Directory relative paths resolve from. Defaults to the current directory.
    root: Option<String>,
    /// Only match inside syntax nodes of these kinds, such as comment or
    /// string, in enabled languages. Files in another language are skipped
    /// once this is set. Aliases comment and string resolve per language.
    within: Option<Vec<String>>,
}

/// The `find` command.
pub fn find_command() -> CommandDef {
    CommandDef::typed::<(), FindOptions, (), Value, _, _>(
        "find",
        |ctx: TypedContext<(), FindOptions, ()>| async move {
            run_find(ctx.options).unwrap_or_else(CmdError::result)
        },
    )
    .description("Find literal or regex matches across files. Each match carries its byte range and file fingerprint, so a program can edit exactly what it found.")
    .examples(crate::usage::example("--root /path/to/repo --paths \"src one.rs\" --paths \"src two.rs\" --literal oldName --expect-matches 2 --format json", "Find an expected number of matches."))
    .hint(crate::usage::PATHS)
    .mcp(annotations::read_only())
    .done()
}

fn run_find(options: FindOptions) -> Result<TypedResult<Value>, CmdError> {
    let root = root(options.root.as_deref())?;
    let mut paths = resolve_all(&root, options.paths)?;
    if paths.is_empty() {
        paths.push(root);
    }
    let query = FindQuery {
        paths,
        globs: options.glob.unwrap_or_default(),
        literal: options.literal,
        regex: options.regex,
        pattern: options.pattern,
        language: options.language,
        offset: options.offset,
        limit: options.limit,
        within: options.within.unwrap_or_default(),
    };
    let page = find(&query).map_err(CmdError::invalid)?;
    let mut body = serde_json::to_value(&page).map_err(|e| CmdError::invalid(e.to_string()))?;
    let Some(expected) = options.expect_matches else {
        return Ok(TypedResult::ok(body));
    };
    let miss = unmet("matches", Some(expected), page.total);
    let outcome = if miss.is_some() {
        Outcome::Failed
    } else {
        Outcome::Passed
    };
    body["outcome"] = json!(outcome);
    if let Some(reason) = miss {
        body["reason"] = json!(reason);
        return Ok(TypedResult::ok_with_exit_code(body, 1));
    }
    Ok(TypedResult::ok(body))
}
