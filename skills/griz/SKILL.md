---
name: griz
description: Edit files with guarded griz find/plan/apply/undo primitives. Use for code edits, multi-file transformations, and rollback after checks.
---

# griz

Supply the transformation; griz guards the write.

- **Code Mode/MCP:** read [composition](references/compose.md).
- **CLI:** use `crates/griz/examples/guarded_cli.py` (HELPER below). It finds,
  plans, applies, checks, and attempts guarded undo when the check fails.

Read this guide once. Batch independent context reads; do not rediscover supplied
paths, repeat successful checks, or read helper source/help without a missing
option or reported failure. Supervise long checks with apoc execution start;
wait on their ID before retrying or undoing.

## CLI

Use an absolute ROOT, a stable KEY per attempt, and optionally `--griz BIN`.
For one step, run `python3 HELPER` with these arguments:

| Arguments | Meaning |
|---|---|
| `--root ROOT`, `--path PATH` | Root and literal file/directory; repeat path. |
| `--glob GLOB` | Repeatable filename filter; prefix `!` to exclude. Searches ROOT without path. Quote globs. |
| `--literal TEXT` / `--regex REGEX` / `--pattern PATTERN` | Choose one; patterns match syntax. Optional `--language LANG` filters structural files. |
| `--replace TEXT` / `--transform FILE` | Constant or Python `replace(match) -> str`; `--transform -` reads stdin. |
| `--expected-matches N --key KEY` | Positive total match count and attempt identity. |
| `--check CMD ARG...` | Check executable and arguments; must be last. |

Paths are literal; use globs for patterns. Inclusion globs can select gitignored
files; exclude them explicitly when needed.

Compose known edit/check steps in one Python program:
~~~python
import json, runpy
helper = runpy.run_path("HELPER")
common = ["--root", "ROOT", "--griz", "BIN"]
result = helper["edit"](
    common + ["--glob", "**/*.ts", "--pattern", "foo($ARG)",
              "--expected-matches", "N", "--key", "KEY",
              "--check", "cargo", "check"],
    transform=lambda m: "bar(" + m["vars"]["ARG"] + ")",
)
print(json.dumps(result))
~~~
Call `edit` again for the next step with a new key. Transforms return text;
they never write files. Matches expose `text`, `vars` (structural captures),
`captures` (regex group 1 at index 0), `path`, and `range`. For whole JSON,
use `--regex '(?s)\A.*\z'`, parse `text`, change values, and return serialized text.
Do not combine a callable with replace, transform-file, patch, or ops input.

`edit` returns a passed check receipt or raises `helper["CommandFailure"]` with
the receipt in `.report`. Exceptions stop later steps. Continue after an expected
rejection only when the check exit code is the intended one AND
`report["undo"]["outcome"] == "passed"`. Inspect stage/undo on other failures;
retain the operation ID if a check cannot start or is interrupted. The helper
does not automatically resume interrupted work. Replays verify recorded file
fingerprints; on refusal, inspect current files and use a new key for a new edit.
Success logs are omitted unless `--show-check-output`; failures retain them.

For patches, operation arrays, whole-file creation, direct primitives, partial
selection, capture-target or language-server edits, formatters, merges, and
history, read [advanced operations](references/advanced.md).
