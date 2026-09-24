# Advanced griz operations

Load only for the operation you need. For direct calls, follow the
[composition guards](compose.md#guards-for-direct-calls).

## CLI spelling and failure handling

| Task | CLI | MCP |
|---|---|---|
| Apply | `griz apply PLAN --purpose ... --idempotency-key ... --format json` | `griz.apply({ plan, purpose, idempotency_key })` |
| Undo | `griz undo OP --root ROOT --purpose ... --idempotency-key ... --format json` | `griz.undo({ root, operation, purpose, idempotency_key })` |
| Multiple paths | `--paths 'src one.rs' --paths 'src two.rs'` | `paths: ["src one.rs", "src two.rs"]` |
| Full record | `griz get ID --format json` | `griz.get({ id })` |

CLI IDs are positional: `apply PLAN`, not `apply --plan PLAN`.
Repeat `--paths` for each value; a JSON array string is a literal path.
`get` already returns the full record and accepts neither `--purpose` nor
`--verbosity`. Quote paths with spaces.

A CLI program must check both process exit status and the parsed
`outcome == "passed"` before taking an ID or advancing. Retain stdout and stderr
on failure. An error may have no ID; a failed plan may have an inspection ID.
A shell pipeline that extracts only `id` can conceal the producer's failure.
Use exact argument arrays rather than composing a command string.


Mutations answer `{id, outcome}`. Use `verbosity: "trace"` or `griz.get({ id })`
for a full record. A missed anchor's nearest text is in `problems[].nearest`.

## File summaries

Use `griz find --files-only --literal oldName --root ROOT --format json`
or `griz.find({ root, literal: "oldName", files_only: true })` when the caller
needs matching files rather than individual spans. The result has
`file_matches: [{ path, count, file_hash }]`, `total`, `files`, and `next`.
`offset`, `limit`, and `next` count matching files in this mode.
`total` and `expect_matches` still count individual matches across all files,
including files outside the returned page. Search filters remain the same.

The guarded CLI helper uses this mode for constant literal replacements and
retains each file's fingerprint in the plan. Callbacks still receive full matches.

## Match by shape

`griz.find({ root, pattern: "foo($A, $$$REST)" })` matches syntax, not text,
in Rust, TypeScript, JavaScript, Python, Go, and Swift. Each match carries
`vars` (`{ A, REST }`), their `var_ranges`, and the same `range` and
`file_hash` as a text match, so the edit that follows is unchanged.

A `replace` can be located by the same pattern and can edit one capture:
`{ op: "replace", path, pattern: { pattern: "format!($FMT, $A)" }, target: "$FMT", replace: '"hi {}"' }`.
A pattern match is exact on the parse tree, so it is `machine` confidence.

`griz.find({ root, literal: "TODO", within: ["comment"] })` searches only
inside comments; `string` and raw node kinds work too.

## Edits from a language server

Ask the language server, in the same program, for a rename or code action, then
hand its `WorkspaceEdit` to griz so the write is guarded, atomic, and undoable:

```js
const edit = await lsp.rename({ uri, position, newName: "total" });
const plan = await griz.plan({ root, workspace_edit: edit, purpose: "rename", idempotency_key: "rename" });
```

Positions count in UTF-16 unless `position_encoding` says otherwise.

## Check before applying

Pass `expect_syntax: "clean"` to fail when a plan introduces syntax errors,
and stop before apply if its outcome is not `passed`. Read `summary.syntax`
at `verbosity: "info"` for the parse facts. `diff` lists the named items each
file adds, removes, or changes.

A pending check still owns its execution. Wait on its id before deciding whether
to undo. After a terminal failure, inspect the returned undo verdict; files
changed by someone else can prevent restoration.

## Formatters

Run the formatter after `apply`, then `griz.absorb({ root, operation, purpose,
idempotency_key })`. Undo then restores the text from before the apply instead
of refusing the reformatted files. `absorb` takes any later change to the
written files, not only a formatter's, so run it right after the formatter,
before anything else can touch the same files.

## Patch text

`griz.plan({ root, patch })` accepts Codex patch text (`*** Begin Patch` …).
Several blocks for one file are fine, and several whole documents may follow
one another in the same text. From the command line, `--patch @file`,
`--ops @file`, and `--workspace-edit @file` read the input from a file.

For replacement text computed from an existing file, retain the fingerprint
returned by `read` or `find` as `expect_hash`. Byte-range replacements require
that fingerprint or `find` with the exact old text at the range; unchecked ranges
are rejected before apply.

Replace a file's whole contents with
`{ op: "create", path, text, overwrite: true }`; no range, no byte count.

## Tolerant matches

Anchors match exactly, then ignoring trailing whitespace, then by relative
indentation, then trimmed. Anything but exact is `maybe`, and `apply` refuses it
unless `min_confidence: "maybe"`. Prefer `select({ root, min_confidence: "machine" })`
and a second look at the rest.

## Files changed by someone else

`apply` refuses stale files. `on_stale: "merge"` merges three ways and writes
only when the merge is clean. `undo` never overwrites a file changed since the
apply; it lists it under `conflicts`, or merges around the change with
`on_stale: "merge"`.

A conflict at `verbosity: "debug"` carries `merge_conflicts` with `base`,
`planned`, and `current` blob ids. Read them with `griz.get`, run any merger
(for example `mergiraf merge` through apoc), and plan the result as a whole-file
replace guarded by `current_fingerprint`.

## Rewind an attempt

`griz.undo({ root, since: firstOp, purpose, idempotency_key })` restores every
operation from `firstOp` through the newest as one operation. It refuses when a
file's history was changed outside griz in between; leave such files out with
`paths`. Undo the restore to go forward again.
## Partial application and undo

To keep the parts that passed, undo only the failing files with
`griz.undo({ root, operation, paths })`, or build a smaller plan with
`select` with `paths`, `edits`, or `min_confidence` and apply that.
