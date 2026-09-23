---
name: griz
description: Edit code through griz primitives — find, plan, diff, select, apply, undo — composed in a Code Mode program with apoc checks, jev judgments, or any other tool. Use for any file edit an agent makes, especially multi-file changes, renames and codemods, edits that must be checked and rolled back, and edits that must not clobber concurrent changes.
---

# griz

griz edits files in steps whose outputs feed the next step. Compose them in one
Code Mode program so matches, plans, and check output never reach the model.

## Rules

- Pass `root` on calls that resolve or display paths: `read`, `find`, `plan`,
  `select`, `undo`, `absorb`, `log`, and `diff`. Over MCP, griz runs wherever
  its host started it. `apply` and `get` use recorded ids and do not accept `root`.
- For CLI programs that parse responses as JSON, pass `--format json`.
- Pass `purpose` and a deterministic `idempotency_key` on `plan`, `select`,
  `apply`, and `undo`. Keys are scoped per command, so `"rename"` can name the
  plan and the apply of one change. A retried program then replays instead of
  writing twice.
- Declare what you expect: `expect_matches` on `find`, `expect_edits` or
  `expect_files` on `plan`, `expect_files` on `apply`. An unmet count is
  `failed`, not silently accepted. An `apply` file-count mismatch is checked
  before source writes, so it needs no undo.
- Mutations answer `{id, outcome}`. Read more with `verbosity: "trace"` or
  `griz.get({ id })`; a missed anchor's nearest real text is in `problems[].nearest`.

Stop the program when a declared expectation fails. A failed plan still has an
id for inspection; passing that id to `apply` does not carry the plan's count or
syntax expectations into the apply. Set `root` to the repository's absolute path.

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

For a fully specified literal edit, run the repository's
`crates/griz/examples/guarded_cli.py` directly. It performs the match-count,
fingerprint, and syntax checks before applying and runs the supplied check once.
Use `--help` for arguments. A passed receipt includes the check result; avoid
dumping the helper source or adding another copy of its checks on the successful
path. Inspect the implementation when diagnosing a reported failure or when the
transformation needs different behavior.

The helper checks process status, JSON shape, verdict, and returned IDs; on check
failure it reports the original output and the undo verdict. Run it with
`apoc execution start python3 ... -- /path/to/guarded_cli.py ...` so an unfinished
check retains a durable execution ID. See the README for the complete invocation.

## Edit exactly what you found

```js
const expectedMatches = 12;
const found = await griz.find({
  root, regex: "\\boldName\\b", glob: ["**/*.rs"], limit: expectedMatches, expect_matches: expectedMatches,
});
if (found.outcome !== "passed")
  return { stage: "find", outcome: found.outcome, reason: found.reason, total: found.total };
const ops = found.matches.map(m => ({
  op: "replace", path: m.path, range: m.range, find: { text: m.text },
  replace: "new_name", expect_hash: m.file_hash,
}));
const plan = await griz.plan({
  root, ops, expect_edits: expectedMatches, expect_syntax: "clean",
  purpose: "rename", idempotency_key: "rename-plan",
});
if (plan.outcome !== "passed") return { stage: "plan", ...plan };
```

A `range` op needs no re-matching, and `expect_hash` refuses a file that changed
since `find`.

## Apply, check, roll back

```js
const applied = await griz.apply({
  plan: plan.id, expect_files: found.files,
  purpose: "rename", idempotency_key: "rename-apply",
});
if (applied.outcome !== "passed") return { stage: "apply", ...applied };
const check = await apoc.execution_command({
  executable: "cargo", arg: ["check"], cwd: root, expect_exit_code: [0],
  purpose: "check the rename", idempotency_key: "rename-check",
});
const result = { stage: "check", operation: applied.id, check };
if (check.outcome === "pending" || check.outcome === "passed") return result;
const undone = await griz.undo({
  root, operation: applied.id, purpose: "roll back", idempotency_key: "rename-undo",
});
return { ...result, undo: undone };
```

To keep the parts that passed, undo only the failing files with
`griz.undo({ root, operation, paths })`, or build a smaller plan with
`select` with `paths`, `edits`, or `min_confidence` and apply that.

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
