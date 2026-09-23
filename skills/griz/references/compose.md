# Compose griz in host Code Mode

Set `root` to the absolute project path. Keep intermediate matches and plans
inside the program.

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
