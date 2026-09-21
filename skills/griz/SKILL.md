---
name: griz
description: Edit code through griz primitives — find, plan, diff, select, apply, undo — composed in a Code Mode program with apoc checks, jev judgments, or any other tool. Use for any file edit an agent makes, especially multi-file changes, renames and codemods, edits that must be checked and rolled back, and edits that must not clobber concurrent changes.
---

# griz

griz edits files in steps whose outputs feed the next step. Compose them in one
Code Mode program so matches, plans, and check output never reach the model.

## Rules

- Pass `root` on every call. Over MCP, griz runs wherever its host started it.
- Pass `purpose` and a deterministic `idempotency_key` on `plan`, `select`,
  `apply`, and `undo`. Keys are scoped per command, so `"rename"` can name the
  plan and the apply of one change. A retried program then replays instead of
  writing twice.
- Declare what you expect: `expect_matches` on `find`, `expect_edits` or
  `expect_files` on `plan`, `expect_files` on `apply`. An unmet count is
  `failed`, not silently accepted.
- Mutations answer `{id, outcome}`. Read more with `verbosity: "trace"` or
  `griz.get(id)`; a missed anchor's nearest real text is in `problems[].nearest`.

## Edit exactly what you found

```js
const found = await griz.find({ root, regex: "\\boldName\\b", glob: ["**/*.rs"], expect_matches: 12 });
const ops = found.matches.map(m => ({ op: "replace", path: m.path, range: m.range,
  find: { text: m.text }, replace: "new_name", expect_hash: m.file_hash }));
const plan = await griz.plan({ root, ops, purpose: "rename", idempotency_key: "rename" });
```

A `range` op needs no re-matching, and `expect_hash` refuses a file that changed
since `find`.

## Apply, check, roll back

```js
const applied = await griz.apply({ plan: plan.id, purpose: "rename", idempotency_key: "rename" });
const check = await apoc.execution_command({ executable: "cargo", arg: ["check"], cwd: root,
  expect_exit_code: [0], purpose: "check rename", idempotency_key: "rename-check" });
if (check.outcome !== "passed")
  await griz.undo({ operation: applied.id, purpose: "roll back", idempotency_key: "rename" });
```

To keep the parts that passed, undo only the failing files with
`griz.undo({ operation, paths })`, or build a smaller plan with
`griz.select({ plan, paths | edits | min_confidence })` and apply that.

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

Read `summary.syntax` on a plan at `verbosity: "info"`: `introduced_errors`
means an edit broke the parse. `diff` lists the named items each file adds,
removes, or changes. griz reports these; the program decides.

## Formatters

Run the formatter after `apply`, then `griz.absorb({ operation, purpose,
idempotency_key })`. Undo then restores the text from before the apply instead
of refusing the reformatted files. `absorb` takes any later change to the
written files, not only a formatter's, so run it right after the formatter,
before anything else can touch the same files.

## Patch text

`griz.plan({ root, patch })` accepts Codex patch text (`*** Begin Patch` …).
Several blocks for one file are fine, and several whole documents may follow
one another in the same text. From the command line, `--patch @file`,
`--ops @file`, and `--workspace-edit @file` read the input from a file.

Replace a file's whole contents with
`{ op: "create", path, text, overwrite: true }`; no range, no byte count.

## Tolerant matches

Anchors match exactly, then ignoring trailing whitespace, then by relative
indentation, then trimmed. Anything but exact is `maybe`, and `apply` refuses it
unless `min_confidence: "maybe"`. Prefer `select({ min_confidence: "machine" })`
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

`griz.undo({ since: firstOp, purpose, idempotency_key })` restores every
operation from `firstOp` through the newest as one operation. It refuses when a
file's history was changed outside griz in between; leave such files out with
`paths`. Undo the restore to go forward again.
