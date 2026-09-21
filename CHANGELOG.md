# Changelog

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
