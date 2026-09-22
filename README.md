# griz

Code edits as composable primitives, for agents and the programs they write.

griz splits an edit into small steps whose outputs feed the next step: **find**
text, **plan** a change without writing it, **diff** or **select** part of the
plan, **apply** it all-or-nothing, and **undo** it later. Served over MCP, every
step is its own tool, so a Code Mode program composes griz with anything else
it can call: run checks through apoc, judge matches with jev, record work in
devsql.

```js
// One apoc Code Mode program: rename across files, check, roll back on failure.
const root = "/path/to/repo";
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

Only what the program returns reaches the model. A failed preflight stops before
the next step. For a pending check, keep its execution id and the operation id,
then wait for the check to finish before deciding whether to undo. A terminal
check failure returns the undo verdict too; undo can refuse later file changes.

## Guarantees

- **Nothing is written before apply.** `plan` builds every file's new text in
  memory and records it; `diff` shows it.
- **Stale files are refused.** Every match and every planned file carries the
  fingerprint of the text it was computed against. `apply` writes nothing
  unless every file still has that fingerprint, or, with `on_stale: "merge"`,
  unless a three-way merge is clean.
- **All or nothing.** Every file is staged before the first rename, and the
  operation is journaled first. A process killed mid-apply is finished by the
  next griz invocation. Recovery skips operations that another writer
  already completed.
- **Tolerant matches are labeled.** An anchor is matched exactly first, then
  ignoring trailing whitespace, then by relative indentation, then trimmed.
  Only exact matches and find ranges are `machine` confidence; `apply` refuses
  `maybe` edits unless asked, and `select` can keep only the exact ones.
- **Ambiguity is an error.** An anchor matching twice reports every candidate
  line instead of taking the first.
- **Misses show the real text.** A missed anchor records the nearest real window
  of the file, readable with `get`.
- **Undo never clobbers.** Undo restores only files still exactly as the
  operation left them, or, with `on_stale: "merge"`, merges cleanly around
  later edits. An undo is itself an operation that can be undone.
- **Restores follow history.** `undo --since` rewinds a span of operations
  only where every file's writes hand off the same fingerprint; a change made
  outside griz in between refuses the restore. The starting operation must
  exist; an unknown ID is rejected before any restoration. History follows
  journal order under file locks, so a delayed concurrent edit appears after
  the writes it builds on, regardless of when its ID was created.
- **Parse facts.** A plan reports whether each file still parses. Request
  `expect_syntax: "clean"` to fail a plan that introduces syntax errors.
- **Formatters do not break undo.** Run a formatter after `apply`, then
  `absorb` the operation; undo restores the text from before the apply.
  Concurrent absorbs retain one another's selected-file updates. Undo and span
  restore refresh selected records after waiting for file locks, including
  absorbs that completed during the wait. A span keeps its original operation IDs.
- **Selection uses saved inputs.** Selecting part of a plan uses its original
  file snapshots, including files whose operations cancelled out. An older plan
  missing a required snapshot is refused; plan again to use current files.
- **Retries are safe.** Mutations take an `idempotency_key`; the same key and
  input replay the original result, and different input with the same key is
  refused.

## Commands

| Command | Kind | Does |
|---|---|---|
| `read PATH` | read | Numbered lines and fingerprint; `--grep`, `--lines`. |
| `find` | read | Literal, regex, or structural (`--pattern 'foo($A, $$$REST)'`) matches with byte ranges, captures, fingerprints; `--within comment`; honors `.gitignore`; `--expect-matches`. |
| `plan` | records | Operations (`--ops`), Codex patch text (`--patch`, `@file`), or an LSP `--workspace-edit` into a plan id, with parse facts. |
| `select PLAN` | records | A new plan from part of another, by path, edit id, or confidence. |
| `diff ID` | read | Unified diff of a plan or operation, with the named items it changes; `--grep` or `--lines`. |
| `apply PLAN` | writes | All-or-nothing write; `--min-confidence`, `--on-stale merge`, `--expect-files`. |
| `undo OP` | writes | Restore an operation's files; `--paths`, `--on-stale merge`, or `--since OP` for a span. |
| `absorb OP` | records | Fold a later formatter run into an operation so undo still works. |
| `log` / `get ID` | read | Operation history, narrowed with `--paths`; the complete record of a plan, operation, or `blob_<hash>`. |

Mutations answer `{id, outcome}` with `outcome` one of `passed`, `failed`
(a declared expectation did not hold), or `error` (nothing was done). Add
`--verbosity warn|info|debug|trace` for more; `trace` returns the full record.
A verdict other than `passed` exits nonzero.

WorkspaceEdit plan retries compare the parsed edit JSON and position encoding
(default `utf-16`) before reading target files. Keep any `@file` input available
and unchanged for a retry. Older versions identified these requests by converted
operations: their WorkspaceEdit keys now return `IDEMPOTENCY_CONFLICT`, while
saved plan IDs remain usable. New planning requests need new keys.

Operations, applied in order:

```text
{op:"replace", path, find:"text" | {text, after?, whole_lines?} | range:{start,end} | pattern:{pattern, language?}, target?, replace, occurrence?, expect_hash?}
{op:"insert",  path, anchor:{text}, after?, text, expect_hash?}
{op:"create",  path, text, overwrite?}
{op:"delete",  path, expect_hash?}
{op:"move",    path, to, expect_hash?}
```

## Use

```sh
cargo install griz
griz --mcp            # MCP server: every command is a direct tool
```

State lives in `$GRIZ_HOME`, or the user data directory. The journal uses schema 2.
Opening a schema-1 store saves a consistent `*.pre-v2-from-v1-*.sqlite3` backup
beside the database and preserves its insertion order during the upgrade.
Older binaries refuse the upgraded store; keep the newer binary for continued
use of that history.

## Workspace

- `griz-core`: the pure engine. It reads files it is handed and never writes;
  `cargo xtask check` enforces that.
- `griz-store`: journal, content store, locks, idempotency receipts, recovery.
- `griz`: the command graph, CLI, and MCP server.
- `xtask`: quality policy. Files at most 300 lines, functions at most 60 code
  lines, narrow test-only lint suppressions, dependency direction
  `griz → griz-store → griz-core`.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check
cargo test -p griz compose -- --ignored   # needs apoc on PATH
cargo test -p griz documented_examples -- --ignored
cargo test -p griz --release cli_round_trip_measurement -- --ignored --nocapture
cargo test -p griz-core --release workspace_conversion_measurement -- --ignored --nocapture
cargo test -p griz-store --release selection_measurement -- --ignored --nocapture
cargo test -p griz-store --release history_pagination_measurement -- --ignored --nocapture
cargo test -p griz-store --release restore_span_measurement -- --ignored --nocapture
```
