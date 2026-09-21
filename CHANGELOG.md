# Changelog

## 0.2.0 (2026-09-21)

- `plan --workspace-edit`: plan an LSP `WorkspaceEdit` from a language server, rust-analyzer's structural replace, or any tool that emits one. Positions count in UTF-16 by default (`--position-encoding`), and each edit is a fingerprint-guarded byte range.
- A `replace` can be located by a structural `pattern` instead of text, and `target: "$NAME"` edits only that capture. Pattern matches are exact on the parse tree and `machine` confidence. `find` matches carry `var_ranges`.
- Plans report parse facts: whether each file in a supported language still parses, and where the first new error is.
- `find --within comment|string|<node kind>` searches text only inside those syntax nodes.
- `diff` lists the functions, types, and other named items each file adds, removes, or changes.
- `undo --on-stale merge` merges an undo around later edits to other parts of a file.
- A merge conflict hands over `base`, `planned`, and `current` blob ids and the conflicting line regions; `get blob_<hash>` returns a blob's text, so an outside merger can resolve it.
- `undo --since OP` restores every operation from `OP` through the newest as one operation, which can itself be undone. A file whose history was changed outside griz refuses the restore; narrow it out with `--paths`.

## 0.1.0 (2026-09-21)

- `read`, `find`, `plan`, `select`, `diff`, `apply`, `undo`, `log`, `get`, and `absorb` as CLI commands and direct MCP tools.
- Plans from structured operations or Codex patch text, with a reported matching ladder and per-edit confidence. The `plan` MCP tool publishes a typed schema for its operations.
- Structural `find` with `pattern` and `language`: ast-grep patterns with `$VAR` and `$$$VARS`, answered in the same match shape as text queries.
- Structural `find` skips files in languages griz has no parser for, and a malformed pattern or unknown language is an error.
- `absorb`: fold a formatter run into an operation so undo keeps working. Naming a file the operation never wrote is an error.
- A plain string is accepted as an anchor.
- Fingerprint-guarded, all-or-nothing apply with optional three-way merge, journaled for crash recovery.
- Undo that never overwrites a file changed since, and that can itself be undone.
- Idempotency keys with replay and conflict detection on every mutation.
