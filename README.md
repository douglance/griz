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
- **Search failures are explicit.** Missing search paths, traversal errors, and
  file read failures return an error instead of an empty or partial result.
  Non-UTF-8 files are skipped; empty readable directories still return no matches.
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
  line instead of taking the first. Selecting `occurrence: "all"` also refuses
  overlapping tolerant matches and lists their lines; select one occurrence or
  narrow the anchor.
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
- **Parent paths follow the filesystem.** Before traversing `..`, griz resolves
  the preceding directory, including directory symlinks. Missing or non-directory
  prefixes are rejected instead of silently choosing a different file. Mutation
  retries consult their original receipt before resolving target paths again.
- **Directory aliases compose.** New plans resolve existing parent directories,
  so a directory symlink and its target share one planned file and one lock.
  Retargeting the link later does not redirect a new saved plan. Missing directory
  suffixes stay creatable; dangling directory links are rejected. Older saved
  paths and their original filters remain usable, but a write containing multiple
  paths to the same destination is refused before staging. This identifies parent
  directories; final file-name case aliases and file symlinks are separate.
- **Replacement permissions stay intact.** Replacing an existing file retains
  its Unix access and executable bits, including during undo and crash recovery.
  A later permission change is retained when undo replaces that file. Newly
  created files, including restores of deleted files, use creation defaults.
- **Large write batches stage concurrently.** Batches with at least 32 writes
  use at most four staging workers. Every file keeps its full sync, and every
  worker finishes before the operation is journaled. Final renames retain plan
  order. On staging errors, workers finish and attempt cleanup before returning.
- **Formatters do not break undo.** Run a formatter after `apply`, then
  `absorb` the operation; undo restores the text from before the apply.
  Concurrent absorbs retain one another's selected-file updates. Undo and span
  restore refresh selected records after waiting for file locks, including
  absorbs that completed during the wait. A span keeps its original operation IDs.
- **Selection uses saved inputs.** Selecting part of a plan uses its original
  file snapshots, including files whose operations cancelled out. An older plan
  missing a required snapshot is refused; plan again to use current files.
  Selection loads only snapshots needed by the operations it keeps, including
  both ends of a move; missing discarded snapshots do not block selection.
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

`diff` items keep `kind`, `name`, and `change`. Nested definitions also include
`scope`, an outer-to-inner list such as `["left", "impl A"]`. File-level items
omit it. Same-named definitions in different scopes remain distinct; overloads
can produce multiple items with the same kind, name, and scope. Rust callers
constructing `DiffItem` values must initialize `scope`; use `Vec::new()` at file
scope.

Mutations answer `{id, outcome}` with `outcome` one of `passed`, `failed`
(a declared expectation did not hold), or `error` (nothing was done). Add
`--verbosity warn|info|debug|trace` for more. Each level retains the fields and
values below it; `trace` also includes the full record without dropping the
reason, summary, or detail.
A verdict other than `passed` exits nonzero.

Absorb retries retain the original call's counts, skipped-file reason, and
operation snapshot, even after another absorb updates the operation. Use `get`
for the operation's current state. Older absorb receipts saved only the ID and
verdict: they still replay at `off` or `error` verbosity, but requesting details
returns `IDEMPOTENCY_DETAIL_UNAVAILABLE` instead of reconstructing missing facts.

WorkspaceEdit plan retries compare the parsed edit JSON and position encoding
(default `utf-16`) before reading target files. Keep any `@file` input available
and unchanged for a retry. Older versions identified these requests by converted
operations: their WorkspaceEdit keys now return `IDEMPOTENCY_CONFLICT`, while
saved plan IDs remain usable. New planning requests need new keys.

Parent-traversal paths retain `..` in mutation request identities. Older keys
whose recorded inputs removed those components can return `IDEMPOTENCY_CONFLICT`;
their saved plan and operation IDs remain available. Use a new key for a new request.

Operations, applied in order:

```text
{op:"replace", path, find:"text" | {text, after?, whole_lines?} | range:{start,end} | pattern:{pattern, language?}, target?, replace, occurrence?, expect_hash?}
{op:"insert",  path, anchor:{text}, after?, text, expect_hash?}
{op:"create",  path, text, overwrite?}
{op:"delete",  path, expect_hash?}
{op:"move",    path, to, expect_hash?}
```

## CLI programs

The CLI uses positional identifiers and repeated flags; MCP uses named JSON fields.

| Task | CLI | MCP |
|---|---|---|
| Apply a passed plan | `griz apply PLAN ...` | `griz.apply({ plan, ... })` |
| Undo an operation | `griz undo OP --root ROOT ...` | `griz.undo({ root, operation, ... })` |
| Select two paths | `--paths 'src one.rs' --paths 'src two.rs'` | `paths: ["src one.rs", "src two.rs"]` |
| Inspect a record | `griz get ID --format json` | `griz.get({ id })` |

The ellipses above stand for the mutation's required purpose and idempotency key.
`get` returns the complete record; it does not take `--purpose` or `--verbosity`.
`--paths '["src one.rs"]'` is one literal path on the CLI, not an array.

Programs must pass `--format json`, check process exit status and
`outcome == "passed"`, and only then extract the identifier. Parsing a particular
output line or extracting `id` alone can hide a failure. A failed plan can retain
an ID for inspection; its count and syntax expectations are not carried into
`apply`. Stop before apply when planning fails. Preserve stdout and stderr when
the process fails or its response is malformed, including errors without an ID.

[The guarded CLI example](crates/griz/examples/guarded_cli.py) uses Python's
standard library and exact argument arrays to find, plan, apply, check, and
report a guarded undo after a failed check. A complete literal search becomes
one `occurrence: "all"` operation per file, retaining its fingerprint and the
total expected edit count. Conflicting or missing fingerprints are refused.
It never starts the check after a refused apply. Run it under apoc to supervise the whole attempt:

```sh
apoc execution start python3 --purpose "Rename and check" --idempotency-key rename-attempt-1 --expect-exit-code 0 -- /path/to/griz/crates/griz/examples/guarded_cli.py --root /path/to/repo --path "src one.rs" --path "src two.rs" --literal oldName --replace new_name --expected-matches 12 --key rename-attempt-1 --check cargo check
```

Put `--check` last: everything after it is the check's executable and arguments.
Use a stable key for an attempt; a different edit needs a new key. The example
does not resume an interrupted workflow automatically. Retain apoc's execution
ID and inspect its terminal result before starting another attempt. Its report
retains the applied operation ID if the check cannot start, and includes the
original check output and undo verdict when a check fails. A successful undo
does not turn a failed check into success. Successful checks report exit status
and output byte counts without printing the check logs. Put `--show-check-output`
before `--check` to include those logs on success too; failed checks always
include both streams. Text is decoded as UTF-8, with undecodable bytes shown as
`\xNN`; byte counts use the captured bytes. Operation JSON travels through a temporary `--ops @file`,
so large plans do not exceed command-line argument limits. The input file is
removed after planning, including when planning fails.

## Compose with apoc

Register the installed griz executable as a provider to make its primitives
available immediately in apoc Code Mode:

```sh
apoc provider add-mcp-stdio griz /path/to/griz --arg=--mcp --purpose "Enable griz composition" --idempotency-key griz-provider
```

Then `apoc capability search griz.find` should include `griz.find`. Pass
`root` explicitly on path-resolving griz calls. A configured server appearing
in `apoc mcp check` is not proof that the running Code Mode actor exposes its
namespace; verify a direct call before building a workflow around it.

## Measure workflow text

[The workflow-text benchmark](crates/griz/examples/measure_workflow_tokens.py)
executes the same literal edits through search plus a patch, a short Python
script invoked from the shell, and the guarded griz example. It verifies every
result against independently rendered expected file contents, then counts
request and response text with `cl100k_base` and `o200k_base`:

```sh
uv run --with tiktoken==0.12.0 python crates/griz/examples/measure_workflow_tokens.py --griz /path/to/griz --output /tmp/griz-token-results
```

The report retains transcripts and the tested binary's hash. Counts exclude
model reasoning, schemas, transport envelopes, caching, billing, and common
fixture/check setup. The shell baseline has no snapshot guard or journal;
these measurements compare successful edits, not equivalent recovery guarantees.
Absolute command paths and generated IDs affect the counts.

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
