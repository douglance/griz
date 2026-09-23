---
name: griz
description: Edit files with guarded griz find/plan/apply/undo primitives. Use for code edits, multi-file transformations, and rollback after checks.
---

# griz

The caller supplies the transformation. Compose in the host's Code Mode when
available; for CLI execution, use the guarded helper below.

## Execution path

- **Host Code Mode/MCP:** read [composition](references/compose.md), then compose
  the primitives and check in one program. Return only the verdict and IDs.
- **CLI:** run the repository's crates/griz/examples/guarded_cli.py directly.
  It performs find, plan, apply, check, and guarded undo after a failed check.
  The recipe below contains routine arguments; do not also read primitive help
  or helper source unless a missing option or reported failure requires it.

Read the skill once. Do not reload an identical guide or repeat a successful
helper check. Supervise long checks with apoc execution start; retain its ID and
wait for a terminal outcome before retrying or undoing.

## CLI recipe

Use an absolute root and a stable key for one attempt. HELPER below is the
repository helper path; --griz BIN optionally selects the griz executable.

| Argument | Meaning |
|---|---|
| --root ROOT --path PATH | File or directory under ROOT; repeat --path for more. |
| --literal TEXT, --regex REGEX, or --pattern PATTERN | Choose one match mode. Patterns match syntax in supported source languages. |
| --replace TEXT or --transform FILE | Constant text, or caller Python defining replace(match) -> str. --transform - reads stdin. |
| --expected-matches N --key KEY | Positive total match count and identity for this attempt. |
| --check CMD ARG... | Check executable and arguments; this option must be last. |
| --language LANG | Optional structural file-language filter, not an extension override. |
| --show-check-output | Optional success logs; failures already retain output. |

Child command for a structural rename:

~~~sh
python3 HELPER --root ROOT --path PATH --pattern 'foo($ARG)' \
  --transform - --expected-matches N --key KEY --check cargo check <<'PY'
def replace(match):
    return "bar(" + match["vars"]["ARG"] + ")"
PY
~~~

The function returns text, never writes source files. Matches expose text,
vars for structural captures, captures for regex groups (group 1 at index 0),
path, and byte range. For whole JSON files, use --regex '(?s)\A.*\z',
parse match["text"], change the selected values, and return serialized text.

The helper checks process status, JSON, verdicts, and IDs. A passed receipt
includes the check result. On failure, inspect its stage and undo verdict;
retain the operation ID if the check could not start. It does not automatically
resume an interrupted workflow.

## Guards for every workflow

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

For patches, new/whole files, partial selection, capture-target edits, language
servers, formatters, tolerant anchors, merge conflicts, or history rewinds, read
[advanced operations](references/advanced.md). Read that reference for direct CLI
primitive spelling or full-record/error inspection too.
