#!/usr/bin/env python3
"""Example: replace literal matches, check the edit, and report guarded rollback."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile


class CommandFailure(Exception):
    """A failed stage, retaining the command's original output."""

    def __init__(self, report):
        super().__init__(report["reason"])
        self.report = report


def invoke(argv, root, stage):
    try:
        result = subprocess.run(argv, cwd=root, capture_output=True, text=True)
    except OSError as error:
        raise CommandFailure({
            "stage": stage, "outcome": "error", "command": argv,
            "reason": str(error),
        }) from error
    return result


def griz_command(options, stage, arguments, id_prefix=None):
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
    if body.get("outcome") != "passed":
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


def plan_edit(options):
    paths = [arg for path in options.path for arg in ("--paths", path)]
    found = griz_command(options, "find", [
        "--root", options.root, *paths, "--literal", options.literal,
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
        ops = file_operations(options, matches)
        file_count = len(ops)
    except (KeyError, TypeError, ValueError) as error:
        raise CommandFailure({
            "stage": "find", "outcome": "error",
            "reason": "find returned malformed matches", "response": found,
        }) from error
    return plan_from_ops(options, ops, file_count), file_count


def file_operations(options, matches):
    fingerprints = {}
    for match in matches:
        path, fingerprint = match["path"], match["file_hash"]
        if not isinstance(path, str) or not path:
            raise ValueError("match path is empty or invalid")
        if not isinstance(fingerprint, str) or not re.fullmatch(r"[0-9a-f]{64}", fingerprint):
            raise ValueError("match fingerprint is missing or invalid")
        if match["text"] != options.literal:
            raise ValueError("match text differs from the requested literal")
        if path in fingerprints and fingerprints[path] != fingerprint:
            raise ValueError("one file has inconsistent fingerprints")
        fingerprints[path] = fingerprint
    return [{
        "op": "replace", "path": path, "find": {"text": options.literal},
        "replace": options.replace, "occurrence": "all", "expect_hash": fingerprint,
    } for path, fingerprint in fingerprints.items()]


def plan_from_ops(options, ops, file_count):
    try:
        with tempfile.TemporaryDirectory(prefix="griz-ops-") as directory:
            payload = Path(directory) / "ops.json"
            payload.write_text(json.dumps(ops), encoding="utf-8")
            return griz_command(options, "plan", [
                "--root", options.root, "--ops", "@" + str(payload),
                "--expect-edits", str(options.expected_matches),
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
        "stdout_bytes": len(check.stdout.encode()),
        "stderr_bytes": len(check.stderr.encode()),
    }
    if check.returncode != 0 or options.show_check_output:
        report.update(stdout=check.stdout, stderr=check.stderr)
    return report


def edit_and_check(options):
    plan, file_count = plan_edit(options)
    applied = griz_command(options, "apply", [
        plan["id"], "--expect-files", str(file_count),
        *mutation_options(options, "apply"),
    ], "op")
    report = {"stage": "check", "operation": applied["id"]}
    try:
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


def run(options):
    try:
        return edit_and_check(options)
    except CommandFailure as error:
        return error.report


def parse_options():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--path", action="append", required=True,
                        help="One file or directory; repeat for more paths.")
    parser.add_argument("--literal", required=True)
    parser.add_argument("--replace", required=True)
    parser.add_argument("--expected-matches", required=True, type=int)
    parser.add_argument("--key", required=True,
                        help="Stable identity for this attempt; new edits need new keys.")
    parser.add_argument("--griz", default="griz", help="griz executable to invoke.")
    parser.add_argument("--show-check-output", action="store_true",
                        help="Include stdout and stderr even when the check passes.")
    parser.add_argument("--check", nargs=argparse.REMAINDER, required=True,
                        help="Check executable and exact arguments; put this option last.")
    options = parser.parse_args()
    if options.expected_matches < 1 or not options.literal or not options.check:
        parser.error("give a nonempty literal, positive match count, and check command")
    options.root = str(Path(options.root).resolve())
    return options


if __name__ == "__main__":
    result = run(parse_options())
    print(json.dumps(result, indent=2))
    sys.exit(0 if result["outcome"] == "passed" else 1)
