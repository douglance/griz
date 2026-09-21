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
const root = "/path/to/repo"; // over MCP, griz runs wherever its host started it
const found = await griz.find({ root, literal: "oldName", glob: ["**/*.rs"], expect_matches: 12 });
const ops = found.matches.map(m => ({
  op: "replace", path: m.path, range: m.range, find: { text: m.text },
  replace: "new_name", expect_hash: m.file_hash,
}));
const plan = await griz.plan({ root, ops, purpose: "rename", idempotency_key: "rename-plan" });
const applied = await griz.apply({ plan: plan.id, purpose: "rename", idempotency_key: "rename-apply" });
const check = await apoc.execution_command({
  executable: "cargo", arg: ["check"], cwd: root, expect_exit_code: [0],
  purpose: "check the rename", idempotency_key: "rename-check",
});
if (check.outcome !== "passed") {
  await griz.undo({ operation: applied.id, purpose: "roll back", idempotency_key: "rename-undo" });
}
return { plan: plan.id, check: check.outcome };
```

Only what the program returns reaches the model. The 12 matches, the plan, and
the check output stay inside the program.

## Guarantees

- **Nothing is written before apply.** `plan` builds every file's new text in
  memory and records it; `diff` shows it.
- **Stale files are refused.** Every match and every planned file carries the
  fingerprint of the text it was computed against. `apply` writes nothing
  unless every file still has that fingerprint, or, with `on_stale: "merge"`,
  unless a three-way merge is clean.
- **All or nothing.** Every file is staged before the first rename, and the
  operation is journaled first. A process killed mid-apply is finished by the
  next griz invocation.
- **Tolerant matches are labeled.** An anchor is matched exactly first, then
  ignoring trailing whitespace, then by relative indentation, then trimmed.
  Only exact matches and find ranges are `machine` confidence; `apply` refuses
  `maybe` edits unless asked, and `select` can keep only the exact ones.
- **Ambiguity is an error.** An anchor matching twice reports every candidate
  line instead of taking the first.
- **Misses show the real text.** A missed anchor records the nearest real window
  of the file, readable with `get`.
- **Undo never clobbers.** Undo restores only files still exactly as the
  operation left them, and is itself an operation that can be undone.
- **Retries are safe.** Mutations take an `idempotency_key`; the same key and
  input replay the original result, and different input with the same key is
  refused.

## Commands

| Command | Kind | Does |
|---|---|---|
| `read PATH` | read | Numbered lines and fingerprint; `--grep`, `--lines`. |
| `find` | read | Literal or regex matches with byte ranges, captures, fingerprints; honors `.gitignore`; `--expect-matches`. |
| `plan` | records | Operations (`--ops`) or Codex patch text (`--patch`, `@file`) into a plan id. |
| `select PLAN` | records | A new plan from part of another, by path, edit id, or confidence. |
| `diff ID` | read | Unified diff of a plan or operation, addressable by `--grep` or `--lines`. |
| `apply PLAN` | writes | All-or-nothing write; `--min-confidence`, `--on-stale merge`, `--expect-files`. |
| `undo OP` | writes | Restore an operation's files; `--paths` for a subset. |
| `log` / `get ID` | read | Operation history; the complete record of a plan or operation. |

Mutations answer `{id, outcome}` with `outcome` one of `passed`, `failed`
(a declared expectation did not hold), or `error` (nothing was done). Add
`--verbosity warn|info|debug|trace` for more; `trace` returns the full record.
A verdict other than `passed` exits nonzero.

Operations, applied in order:

```text
{op:"replace", path, find:{text, after?, whole_lines?} | range:{start,end}, replace, occurrence?, expect_hash?}
{op:"insert",  path, anchor:{text}, after?, text, expect_hash?}
{op:"create",  path, text}
{op:"delete",  path, expect_hash?}
{op:"move",    path, to, expect_hash?}
```

## Use

```sh
cargo install --path crates/griz
griz --mcp            # MCP server: every command is a direct tool
```

State lives in `$GRIZ_HOME`, or the user data directory.

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
cargo test -p griz -- --ignored   # composition with apoc; needs apoc on PATH
```
