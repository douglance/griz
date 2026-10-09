#!/usr/bin/env python3
"""Measure complete find, plan, apply, and undo calls with verified durable writes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import select
import sys
import statistics
import subprocess
import tempfile
import time

CASES = [
    ("single_small", 1, 4096),
    ("batch_32", 32, 4096),
    ("batch_256", 256, 4096),
    ("single_large", 1, 7340480),
]
MARKER = "write-benchmark-"


def content(index, size, revision, state):
    prefix = f"{MARKER}{index:04d}\n"
    tail = f"revision {revision:08d}{state}\n"
    return prefix + "x" * (size - len(prefix) - len(tail)) + tail


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


class Mcp:
    def __init__(self, binary, root, env):
        self.child = subprocess.Popen(
            [binary, "--mcp"], cwd=root, env=env, text=True, bufsize=1,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.seq = 0
        initialized = self.call("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "griz-write-benchmark", "version": "1"},
        })
        assert "result" in initialized, initialized
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

    def send(self, message):
        self.child.stdin.write(json.dumps(message) + "\n")
        self.child.stdin.flush()

    def call(self, method, params):
        self.seq += 1
        self.send({"jsonrpc": "2.0", "id": self.seq, "method": method, "params": params})
        if not select.select([self.child.stdout], [], [], 30)[0]:
            raise TimeoutError("MCP response exceeded 30 seconds")
        reply = json.loads(self.child.stdout.readline())
        assert reply.get("id") == self.seq, reply
        return reply

    def close(self):
        self.child.stdin.close()
        self.child.wait(timeout=10)


class Api:
    def __init__(self, binary, root, env, transport):
        self.binary, self.root, self.env = binary, root, env
        self.session = None
        if transport == "mcp":
            self.session = Mcp(binary, root, env)

    def call(self, method, arguments):
        if self.session is not None:
            reply = self.session.call("tools/call", {"name": method, "arguments": arguments})
            assert not reply["result"].get("isError"), reply
            result = reply["result"]["structuredContent"]
        else:
            args = [method]
            options = dict(arguments)
            if method in ["apply", "undo"]:
                args.append(options.pop("plan" if method == "apply" else "operation"))
            payload = None
            for key, value in options.items():
                flag = "--" + key.replace("_", "-")
                if key == "ops":
                    args.extend([flag, "@/dev/stdin"])
                    payload = json.dumps(value)
                elif isinstance(value, bool):
                    if value:
                        args.append(flag)
                elif isinstance(value, list):
                    for item in value:
                        args.extend([flag, str(item)])
                else:
                    args.extend([flag, str(value)])
            proc = subprocess.run(
                [self.binary, *args, "--format", "json"], cwd=self.root, env=self.env,
                input=payload, capture_output=True, text=True,
            )
            assert proc.returncode == 0, (method, proc.stdout, proc.stderr)
            result = json.loads(proc.stdout)
        assert result.get("outcome", "passed") == "passed", (method, result)
        return result

    def mutation(self, method, fields, key):
        return self.call(method, {
            **fields, "purpose": "Measure verified file write latency", "idempotency_key": key,
        })

    def find(self, case, count):
        return self.call("find", {
            "root": str(self.root), "glob": [case + "/*.txt"], "literal": MARKER,
            "files_only": True, "limit": count,
        })

    def close(self):
        if self.session is not None:
            self.session.close()


def verify_files(root, case, texts):
    for index, text in enumerate(texts):
        assert (root / case / f"{index:04d}.txt").read_bytes() == text.encode()


def verify_operation(home, identifier, root, case, before, after):
    uri = (home / "griz.sqlite3").resolve().as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True) as connection:
        row = connection.execute(
            "SELECT state, body FROM operations WHERE id = ?", (identifier,),
        ).fetchone()
    assert row is not None and row[0] == "applied", row
    operation = json.loads(row[1])
    assert operation["state"] == "applied" and not operation["conflicts"]
    assert len(operation["files"]) == len(before)
    expected = {
        str(root / case / f"{index:04d}.txt"): (digest(old), digest(new))
        for index, (old, new) in enumerate(zip(before, after))
    }
    paths = [file["path"] for file in operation["files"]]
    assert len(set(paths)) == len(paths) and set(paths) == set(expected)
    for file in operation["files"]:
        assert (file["before_hash"], file["after_hash"]) == expected[file["path"]]
    return operation


def timed(call):
    start = time.perf_counter_ns()
    value = call()
    return value, time.perf_counter_ns() - start


def run_case(api, home, case, count, size, samples):
    before = [content(index, size, 0, "A") for index in range(count)]
    assert api.find(case, count)["total"] == 0
    initial = api.mutation("plan", {
        "root": str(api.root),
        "ops": [{"op": "create", "path": f"{case}/{index:04d}.txt", "text": text}
                for index, text in enumerate(before)],
    }, case + "-create-plan")
    api.mutation("apply", {"plan": initial["id"]}, case + "-create-apply")
    verify_files(api.root, case, before)
    timings = {name: [] for name in ["find", "plan", "apply", "undo", "pipeline"]}
    for trial in range(samples + 1):
        found, find_ns = timed(lambda: api.find(case, count))
        assert (found["total"], found["files"], found["next"]) == (count, count, None)
        assert len(found["file_matches"]) == count
        known = {file["path"]: file for file in found["file_matches"]}
        after = [content(index, size, trial + 1, "B") for index in range(count)]
        ops = []
        for index, (old, new) in enumerate(zip(before, after)):
            path = str(api.root / case / f"{index:04d}.txt")
            assert known[path]["file_hash"] == digest(old) and known[path]["count"] == 1
            tail = f"revision {trial + 1:08d}B\n"
            ops.append({
                "op": "replace", "path": path,
                "range": {"start": size - len(tail), "end": size},
                "replace": tail, "expect_hash": known[path]["file_hash"],
            })
        plan, plan_ns = timed(lambda: api.mutation("plan", {
            "root": str(api.root), "ops": ops, "expect_files": count,
        }, f"{case}-{trial}-plan"))
        applied, apply_ns = timed(lambda: api.mutation("apply", {
            "plan": plan["id"], "expect_files": count,
        }, f"{case}-{trial}-apply"))
        verify_files(api.root, case, after)
        verify_operation(home, applied["id"], api.root, case, before, after)
        undone, undo_ns = timed(lambda: api.mutation("undo", {
            "operation": applied["id"], "root": str(api.root),
        }, f"{case}-{trial}-undo"))
        verify_files(api.root, case, before)
        verify_operation(home, undone["id"], api.root, case, after, before)
        for name, value in [
            ("find", find_ns), ("plan", plan_ns), ("apply", apply_ns), ("undo", undo_ns),
            ("pipeline", find_ns + plan_ns + apply_ns),
        ]:
            timings[name].append(value)
    row = {
        "files": count, "bytes_per_file": size, "samples": samples,
        "median_ns": {name: statistics.median(values[1:]) for name, values in timings.items()},
        "first_ns": {name: values[0] for name, values in timings.items()},
        "contents_and_journal_verified": True,
    }
    print(json.dumps({"completed_case": case, **row}), file=sys.stderr, flush=True)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--transport", choices=["cli", "mcp", "both"], default="both")
    parser.add_argument("--samples", type=int, default=11)
    parser.add_argument("--cases", nargs="+", choices=[case[0] for case in CASES])
    args = parser.parse_args()
    if args.samples < 3:
        parser.error("--samples must be at least 3")
    rows = {}
    for transport in (["cli", "mcp"] if args.transport == "both" else [args.transport]):
        with tempfile.TemporaryDirectory(prefix="griz-write-benchmark-") as directory:
            work = Path(directory).resolve()
            root = work / "repo"
            root.mkdir()
            home = work / "state"
            env = {**os.environ, "GRIZ_HOME": str(home), "XDG_DATA_HOME": str(work / "data"),
                   "GRIZ_READER_IDLE_MS": "200"}
            api = Api(str(args.binary.resolve()), root, env, transport)
            try:
                rows[transport] = {
                    case: run_case(api, home, case, count, size, args.samples)
                    for case, count, size in CASES if args.cases is None or case in args.cases
                }
            finally:
                api.close()
    print(json.dumps({"binary": str(args.binary), "measurements": rows}, indent=2))


if __name__ == "__main__":
    main()
