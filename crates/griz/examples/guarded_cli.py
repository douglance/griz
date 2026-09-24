#!/usr/bin/env python3
"""Apply caller matches, operations, or Codex patches with a guarded workflow.

Checks declared edit/file counts, file fingerprints, and planned syntax before
applying, then runs the supplied check once. A passed outcome means the edit
applied and the check exited zero. A failed check attempts guarded undo and
retains its output and the undo verdict. A check that cannot start retains the
operation ID for recovery.

Use --help for arguments; inspect this implementation when debugging the helper.
"""

import argparse
import stat
from contextlib import redirect_stdout
from copy import deepcopy
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile


class CommandFailure(Exception):
    """A failed stage, retaining the command's original output."""

    def __init__(self, report):
        super().__init__(json.dumps(report, separators=(",", ":")))
        self.report = report


def invoke(argv, root, stage):
    try:
        result = subprocess.run(argv, cwd=root, capture_output=True)
    except OSError as error:
        raise CommandFailure({
            "stage": stage, "outcome": "error", "command": argv,
            "reason": str(error),
        }) from error
    result.stdout_bytes = len(result.stdout)
    result.stderr_bytes = len(result.stderr)
    result.stdout = result.stdout.decode("utf-8", errors="backslashreplace")
    result.stderr = result.stderr.decode("utf-8", errors="backslashreplace")
    return result


def griz_command(options, stage, arguments, id_prefix=None, *, require_outcome=True):
    argv = [options.griz, stage, *arguments, "--format", "json"]
    result = invoke(argv, options.root, stage)
    report = {
        "stage": stage, "outcome": "error", "command": argv,
        "exit_code": result.returncode,
        "stdout": result.stdout, "stderr": result.stderr,
    }

    def refuse(reason):
        raise CommandFailure({**report, "reason": reason})

    if result.returncode != 0:
        refuse("griz exited unsuccessfully; inspect stdout and stderr")
    try:
        body = json.loads(result.stdout)
    except ValueError:
        refuse("griz did not return a JSON object")
    if not isinstance(body, dict):
        refuse("griz did not return a JSON object")
    if require_outcome and body.get("outcome") != "passed":
        refuse("griz did not report a passed outcome")
    if id_prefix is not None:
        identifier = body.get("id")
        if not isinstance(identifier, str) or not re.fullmatch(
                id_prefix + r"_[0-9a-f]{32}", identifier):
            refuse("griz did not return a valid " + id_prefix + " identifier")
    return body


def mutation_options(options, stage):
    return [
        "--purpose", "Guarded CLI edit: " + stage,
        "--idempotency-key", options.key + "-" + stage,
        "--verbosity", "warn",
    ]


def query_arguments(options):
    selected = [(name, getattr(options, name, None))
                for name in ("literal", "regex", "pattern")
                if getattr(options, name, None) is not None]
    if len(selected) != 1 or not selected[0][1]:
        raise CommandFailure({
            "stage": "find", "outcome": "error",
            "reason": "give exactly one nonempty literal, regex, or pattern",
        })
    name, value = selected[0]
    arguments = ["--" + name, value]
    if getattr(options, "language", None):
        arguments.extend(["--language", options.language])
    return arguments


def load_transform(path):
    if path is None:
        return None
    try:
        text = sys.stdin.read() if path == "-" else Path(path).read_text(encoding="utf-8")
        namespace = {"__name__": "griz_transform", "__file__": path}
        with redirect_stdout(sys.stderr):
            exec(compile(text, "<stdin>" if path == "-" else path, "exec"), namespace)
        transform = namespace.get("replace")
        if not callable(transform):
            raise ValueError("transform must define replace(match)")
        return transform
    except Exception as error:
        raise CommandFailure({
            "stage": "transform", "outcome": "error", "reason": str(error),
        }) from error


def transformed_operation(options, match, transform):
    span = match["range"]
    if (not isinstance(span, dict)
            or type(span.get("start")) is not int or type(span.get("end")) is not int
            or span["start"] < 0 or span["end"] < span["start"]
            or span["end"] - span["start"] != len(match["text"].encode("utf-8"))):
        raise ValueError("match byte range is invalid")
    operation = {
        "op": "replace", "path": match["path"], "range": dict(span),
        "find": {"text": match["text"]}, "expect_hash": match["file_hash"],
    }
    try:
        with redirect_stdout(sys.stderr):
            replacement = transform(deepcopy(match)) if transform else options.replace
        if not isinstance(replacement, str):
            raise TypeError("replace(match) must return a string")
    except Exception as error:
        raise CommandFailure({
            "stage": "transform", "outcome": "error",
            "path": match["path"], "reason": str(error),
        }) from error
    return {**operation, "replace": replacement}


def plan_edit(options, transform=None):
    if any(getattr(options, mode, None) is not None for mode in ("patch", "ops")):
        return plan_supplied_input(options), options.expected_files
    paths = [arg for path in options.path for arg in ("--paths", path)]
    if getattr(options, "literal", None) is not None and transform is None:
        return plan_literal(options, paths)
    found = griz_command(options, "find", [
        "--root", options.root, *paths, *query_arguments(options),
        "--limit", str(options.expected_matches),
        "--expect-matches", str(options.expected_matches),
    ])
    matches = found.get("matches")
    if not isinstance(matches, list) or len(matches) != options.expected_matches:
        raise CommandFailure({
            "stage": "find", "outcome": "error",
            "reason": "find did not return the complete expected page", "response": found,
        })
    try:
        ops = file_operations(options, matches, transform)
        file_count = len({op["path"] for op in ops})
    except (KeyError, TypeError, ValueError) as error:
        raise CommandFailure({
            "stage": "find", "outcome": "error",
            "reason": "find returned malformed matches", "response": found,
        }) from error
    return plan_from_ops(options, ops, file_count), file_count



def plan_literal(options, paths):
    found = griz_command(options, "find", [
        "--root", options.root, *paths, *query_arguments(options), "--files-only",
        "--limit", str(options.expected_matches),
        "--expect-matches", str(options.expected_matches),
    ])
    try:
        ops = summary_operations(options, found)
    except (KeyError, TypeError, ValueError) as error:
        raise CommandFailure({
            "stage": "find", "outcome": "error",
            "reason": "find returned malformed file summaries", "response": found,
        }) from error
    return plan_from_ops(options, ops, len(ops)), len(ops)


def summary_operations(options, found):
    files = found.get("file_matches")
    if (not isinstance(files, list) or type(found.get("files")) is not int
            or len(files) != found["files"] or found.get("next") is not None
            or type(found.get("total")) is not int
            or found["total"] != options.expected_matches):
        raise ValueError("find did not return the complete expected file page")
    seen, ops, total = set(), [], 0
    for file in files:
        path, fingerprint, count = file["path"], file["file_hash"], file["count"]
        if not isinstance(path, str) or not path or path in seen:
            raise ValueError("file path is empty, invalid, or repeated")
        if not isinstance(fingerprint, str) or not re.fullmatch(r"[0-9a-f]{64}", fingerprint):
            raise ValueError("file fingerprint is missing or invalid")
        if type(count) is not int or count < 1:
            raise ValueError("file match count is invalid")
        seen.add(path)
        total += count
        ops.append({
            "op": "replace", "path": path, "find": {"text": options.literal},
            "replace": options.replace, "occurrence": "all", "expect_hash": fingerprint,
        })
    if total != options.expected_matches:
        raise ValueError("file counts differ from the expected total")
    return ops


def file_operations(options, matches, transform=None):
    fingerprints = {}
    for match in matches:
        path, fingerprint = match["path"], match["file_hash"]
        if not isinstance(path, str) or not path:
            raise ValueError("match path is empty or invalid")
        if not isinstance(fingerprint, str) or not re.fullmatch(r"[0-9a-f]{64}", fingerprint):
            raise ValueError("match fingerprint is missing or invalid")
        if not isinstance(match["text"], str):
            raise ValueError("match text is invalid")
        if getattr(options, "literal", None) is not None and match["text"] != options.literal:
            raise ValueError("match text differs from the requested literal")
        if path in fingerprints and fingerprints[path] != fingerprint:
            raise ValueError("one file has inconsistent fingerprints")
        fingerprints[path] = fingerprint
    return [transformed_operation(options, match, transform) for match in matches]


def plan_from_ops(options, ops, file_count):
    return plan_input(options, [("--ops", json.dumps(ops))], options.expected_matches, file_count)


def plan_supplied_input(options):
    inputs = []
    for mode in ("patch", "ops"):
        path = getattr(options, mode, None)
        if path is None:
            continue
        try:
            data = sys.stdin.buffer.read() if path == "-" else Path(path).read_bytes()
            text = data.decode("utf-8")
        except (OSError, UnicodeError) as error:
            raise CommandFailure({
                "stage": "plan", "outcome": "error", "reason": mode + " input: " + str(error),
            }) from error
        inputs.append(("--" + mode, text))
    return plan_input(options, inputs, options.expected_edits, options.expected_files)


def plan_input(options, inputs, edit_count, file_count):
    try:
        with tempfile.TemporaryDirectory(prefix="griz-input-") as directory:
            arguments = []
            for index, (mode, text) in enumerate(inputs):
                payload = Path(directory) / ("input-" + str(index) + ".txt")
                payload.write_text(text, encoding="utf-8")
                arguments.extend([mode, "@" + str(payload)])
            return griz_command(options, "plan", [
                "--root", options.root, *arguments,
                "--expect-edits", str(edit_count),
                "--expect-files", str(file_count), "--expect-syntax", "clean",
                *mutation_options(options, "plan"),
            ], "plan")
    except OSError as error:
        raise CommandFailure({
            "stage": "plan", "outcome": "error",
            "reason": "plan input: " + str(error),
        }) from error


def check_report(options, check):
    report = {
        "command": options.check, "exit_code": check.returncode,
        "stdout_bytes": check.stdout_bytes,
        "stderr_bytes": check.stderr_bytes,
    }
    if check.returncode != 0 or options.show_check_output:
        report.update(stdout=check.stdout, stderr=check.stderr)
    return report



def current_file_hash(path):
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return None
    if not stat.S_ISREG(metadata.st_mode):
        raise OSError("replayed path is no longer a regular file")
    import hashlib
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_replayed_apply(options, applied):
    if not applied.get("replayed"):
        return
    operation = griz_command(options, "get", [applied["id"]], require_outcome=False)
    for file in operation["files"]:
        path = Path(file["path"])
        try:
            current = current_file_hash(path)
        except OSError as error:
            raise CommandFailure({
                "stage": "apply", "outcome": "error", "path": str(path),
                "reason": "cannot verify replayed operation: " + str(error),
            }) from error
        if current != file["after_hash"]:
            raise CommandFailure({
                "stage": "apply", "outcome": "error", "code": "REPLAY_STALE",
                "path": str(path),
                "reason": "replayed operation no longer matches current files; "
                          "inspect changes and use a new key to plan another edit",
            })


def edit_and_check(options, transform=None):
    plan, file_count = plan_edit(options, transform)
    applied = griz_command(options, "apply", [
        plan["id"], "--expect-files", str(file_count),
        *mutation_options(options, "apply"),
    ], "op")
    report = {"stage": "check", "operation": applied["id"]}
    try:
        verify_replayed_apply(options, applied)
        check = invoke(options.check, options.root, "check")
    except CommandFailure as error:
        return {**report, **error.report}
    report.update({
        "outcome": "passed" if check.returncode == 0 else "failed",
        "check": check_report(options, check),
    })
    if check.returncode == 0:
        return report
    try:
        report["undo"] = griz_command(options, "undo", [
            applied["id"], "--root", options.root,
            *mutation_options(options, "undo"),
        ], "op")
    except CommandFailure as error:
        report["undo"] = error.report
    return report


def run(options, transform=None):
    try:
        if transform is None:
            transform = load_transform(getattr(options, "transform", None))
        return edit_and_check(options, transform)
    except CommandFailure as error:
        return error.report


def edit(arguments, *, transform=None):
    """Run one guarded step; raise CommandFailure with its receipt on failure."""
    report = run(parse_options(arguments, transform), transform)
    if report["outcome"] != "passed":
        raise CommandFailure(report)
    return report


def validate_options(parser, options, transform=None):
    if transform is not None and (
        not callable(transform)
        or any(value is not None for value in (
            options.replace, options.transform, options.patch, options.ops))
    ):
        parser.error("callback must be callable and cannot accompany replace, transform, patch, or ops")
    if not options.check:
        parser.error("give a check command")
    if options.patch is not None or options.ops is not None:
        if options.patch == "-" and options.ops == "-":
            parser.error("only one of patch and ops may read stdin")
        match_fields = ("path", "replace", "transform", "language", "expected_matches",
                        "literal", "regex", "pattern")
        if (any(value == "" for value in (options.patch, options.ops))
                or any(getattr(options, name) is not None for name in match_fields)):
            parser.error("patch or ops input cannot accompany match or replacement options")
        if any(value is None or value < 1 for value in (options.expected_files, options.expected_edits)):
            parser.error("patches and ops require positive expected file and edit counts")
        return
    if all(getattr(options, name) is None for name in ("literal", "regex", "pattern")):
        parser.error("give a literal, regex, pattern, patch, or ops input")
    if options.expected_files is not None or options.expected_edits is not None:
        parser.error("expected file and edit counts require patch or ops input; use expected matches")
    if not options.path or options.expected_matches is None or options.expected_matches < 1:
        parser.error("give paths and a positive match count")
    if options.replace is None and options.transform is None and transform is None:
        parser.error("give replace or transform")
    if any(value == "" for value in (options.literal, options.regex, options.pattern, options.transform)):
        parser.error("give a nonempty query and transform path")


def parse_options(arguments=None, transform=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--path", action="append",
                        help="One file or directory; repeat for more paths.")
    query = parser.add_mutually_exclusive_group()
    query.add_argument("--literal", help="Exact text to match.")
    query.add_argument("--regex", help="Regex to match; captures are available to a transform.")
    query.add_argument("--pattern", help="Structural pattern, such as legacy($ARG).")
    parser.add_argument("--patch", metavar="FILE", help="Codex patch file; - reads UTF-8 stdin.")
    parser.add_argument("--ops", metavar="FILE", help="JSON operation array file; - reads UTF-8 stdin.")
    parser.add_argument("--language", help="Restrict structural matching to files in this language.")
    replacement = parser.add_mutually_exclusive_group()
    replacement.add_argument("--replace", help="Literal replacement text.")
    replacement.add_argument("--transform", metavar="FILE",
                             help="Caller Python defining replace(match) -> str; - reads stdin. "
                                  "Return text only. Match has text, vars, captures, path, and byte range.")
    parser.add_argument("--expected-matches", type=int)
    parser.add_argument("--expected-files", type=int, help="Required positive file count for patches or ops.")
    parser.add_argument("--expected-edits", type=int, help="Required positive edit count for patches or ops.")
    parser.add_argument("--key", required=True,
                        help="Stable identity for this attempt; new edits need new keys.")
    parser.add_argument("--griz", default="griz", help="griz executable to invoke.")
    parser.add_argument("--show-check-output", action="store_true",
                        help="Include stdout and stderr even when the check passes.")
    parser.add_argument("--check", nargs=argparse.REMAINDER, required=True,
                        help="Check executable and exact arguments; put this option last.")
    options = parser.parse_args(arguments)
    validate_options(parser, options, transform)
    options.root = str(Path(options.root).resolve())
    return options


if __name__ == "__main__":
    result = run(parse_options())
    print(json.dumps(result, indent=2))
    sys.exit(0 if result["outcome"] == "passed" else 1)
