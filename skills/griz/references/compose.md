# Compose griz in host Code Mode

Use the calls below as the contract for this workflow. On apoc, submit the
program directly to `codemode_execute`; routine schema discovery is unnecessary
for these documented calls. Search for a specific method only when an option is
missing here or the server reports a schema mismatch.

Set `root` to the absolute project path. Keep intermediate matches and plans
inside the program. Batch independent context reads. When the requested change
already specifies the paths and transformation, read those files with `find`
inside the same program that constructs the edits. Return only the verdict and
IDs; inspect matches or source text when choosing a transformation or diagnosing
a failure requires them.

If `codemode_execute` returns `status: "running"`, inspect that execution ID with
`codemode_execution` until terminal. Do not submit the same edit as another run.

## Guards for direct calls

The helper enforces these checks and derives mutation keys from --key. Supply
them yourself when calling primitives directly.

- Pass absolute root to path-based primitives; apply and get use recorded IDs
  and accept no root. Mutations need purpose and idempotency_key, scoped per
  command. Same input/key replays; new edits need new keys.
- Declare match, edit, and file counts. Preserve found byte ranges and file_hash
  as expect_hash in edits. Plan with expect_syntax: "clean".
- Stop on any non-passed verdict. A failed plan's ID is for inspection; applying
  it does not carry its count or syntax expectations forward. An apply count
  mismatch writes nothing and needs no undo.
- Direct griz CLI calls use --format json; the helper already emits JSON.
  Require exit zero and outcome == "passed" before using an ID. Keep stdout/stderr
  on failure; errors may have no ID. Do not extract only id through a pipeline.
- After a terminal check failure, inspect undo's verdict. Concurrent changes can
  prevent restoration. Never undo while the check is still pending.

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

## New files and JSON transforms

Add `{ op: "create", path: "new.txt", text: "hello\n" }` to the same `ops`
array for a new file. Include it in the expected edit and file counts.

For whole-file transforms, use `find` with `paths: ["data.json"]`,
`regex: "(?s)\\A.*\\z"`, `limit: 1`, and `expect_matches: 1`. Each match
contains `path`, `text`, `range`, and `file_hash`. Parse `m.text`, transform the
value in the caller's JavaScript, then set `replace` to
`JSON.stringify(value, null, 2) + "\n"` in the guarded range operation above.
Read all required input files in that program; keep their text and fingerprints
there instead of printing and copying them through another turn.

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



A pending check still owns its execution. Wait on its ID before deciding whether
to undo. After a terminal failure, inspect the returned undo verdict.
