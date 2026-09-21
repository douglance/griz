# Changelog

## Unreleased

- Structural `find` with `pattern` and `language`: ast-grep patterns with `$VAR` and `$$$VARS`, answered in the same match shape as text queries.
- `absorb`: fold a formatter run into an operation so undo keeps working.
- A plain string is accepted as an anchor.

## 0.1.0

- `read`, `find`, `plan`, `select`, `diff`, `apply`, `undo`, `log`, and `get` as CLI commands and direct MCP tools.
- Plans from structured operations or Codex patch text, with a reported matching ladder and per-edit confidence.
- Fingerprint-guarded, all-or-nothing apply with optional three-way merge, journaled for crash recovery.
- Undo that never overwrites a file changed since, and that can itself be undone.
- Idempotency keys with replay and conflict detection on every mutation.
