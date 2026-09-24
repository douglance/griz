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
  It plans and applies guarded edits, runs the check, and attempts guarded undo
  after a failed check.
  The recipe below contains routine arguments; do not also read primitive help
  or helper source unless a missing option or reported failure requires it.

Read the skill once. Batch independent context reads; do not list files only to
rediscover supplied paths. Do not reload an identical guide or repeat a successful
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

For several known edit/check steps, compose `edit` calls in one Python program.
It takes the same CLI argument list and an optional callable transform:

~~~python
import json, runpy
helper = runpy.run_path("HELPER")
common = ["--root", "ROOT", "--griz", "BIN"]
result = helper["edit"](
    common + ["--path", "PATH", "--pattern", "foo($ARG)",
              "--expected-matches", "N", "--key", "KEY",
              "--check", "cargo", "check"],
    transform=lambda match: "bar(" + match["vars"]["ARG"] + ")",
)
print(json.dumps(result))
~~~

Call `edit` again in that program for the next known step, using a new key.
It returns only on success; a non-passed result raises `helper["CommandFailure"]`
with the complete receipt in `.report` and as compact JSON in the exception text.
An unhandled failure stops later steps.
Continue after an expected rejection only when its check exit code is the one
you intended and `report["undo"]["outcome"] == "passed"`. Parser errors also stop.
Do not combine a callable with `--replace`, `--transform`, `--patch`, or `--ops`.
For one standalone step, pass the table's arguments directly to `python3 HELPER`.

The function returns text, never writes source files. Matches expose text,
vars for structural captures, captures for regex groups (group 1 at index 0),
path, and byte range. For whole JSON files, use --regex '(?s)\A.*\z',
parse match["text"], change the selected values, and return serialized text.

For a caller-written Codex patch, replace match/replacement options with
`--patch FILE --expected-files N --expected-edits N`; `--patch -` reads stdin.
Patch paths resolve under ROOT; FILE resolves from the caller's directory.
JSON operation arrays use `--ops FILE` with the same counts; `--ops -` reads stdin.
Pass both flags to combine a patch and operations in one batch: patch first,
then operations. Only one input may read stdin. For example:

~~~json
[{"op":"create","path":"new.txt","text":"hello\n"},
 {"op":"replace","path":"old.txt","find":"before","replace":"after"}]
~~~

Use `griz find --root ROOT --paths PATH --regex '(?s)\A.*\z' --format json`
when you need whole-file observations; repeat `--paths` for more files.
Read-only `find` takes no purpose or idempotency key. Its matches carry
`path`, `text`, `range`, and `file_hash`; retain these in the caller program,
build operations, and invoke the helper there. Do not add a round trip just to
print and copy fingerprints. Supply a fingerprint or old text for byte ranges.
Keep match/transform options separate from patch/ops input. The same check and
guarded undo run afterward.

The helper checks process status, JSON, verdicts, and IDs. A passed receipt
includes the check result. On failure, inspect its stage and undo verdict;
retain the operation ID if the check could not start. It does not automatically
resume an interrupted workflow.

For direct patch planning, new/whole files, partial selection, capture-target edits, language
servers, formatters, tolerant anchors, merge conflicts, or history rewinds, read
[advanced operations](references/advanced.md). Read that reference for direct CLI
primitive spelling or full-record/error inspection too.
