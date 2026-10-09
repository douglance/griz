#!/usr/bin/env python3
"""Compare complete MCP search calls against two griz builds."""
import argparse
import json
import os
from pathlib import Path
import select
import statistics
import subprocess
import tempfile
import time

EXPECTED_HASH = "095c9f2fc59713d124a55214013a8c80310ba0693a7bf46808809ba5890fddf1"


def cli(binary, argv, root, env, payload=None):
    result = subprocess.run(
        [binary, *argv, "--format", "json"], cwd=root, env=env,
        input=payload, capture_output=True, text=True, check=True,
    )
    return json.loads(result.stdout)


def fixture(binary, root, env):
    cli(binary, ["find", "--root", str(root), "--literal", "needle"], root, env)
    text = ("ordinary text with no match\n" * 4096 + "needle\n") * 64
    plan = cli(binary, [
        "plan", "--root", str(root), "--ops", "@/dev/stdin",
        "--purpose", "Create search benchmark fixture",
        "--idempotency-key", "benchmark-fixture-plan",
    ], root, env, json.dumps([{"op": "create", "path": "sparse.txt", "text": text}]))
    assert plan["outcome"] == "passed", plan
    applied = cli(binary, [
        "apply", plan["id"], "--purpose", "Create search benchmark fixture",
        "--idempotency-key", "benchmark-fixture-apply",
    ], root, env)
    assert applied["outcome"] == "passed", applied


class Mcp:
    def __init__(self, binary, root, env):
        self.child = subprocess.Popen(
            [binary, "--mcp"], cwd=root, env=env, text=True, bufsize=1,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.seq = 0
        initialized = self.call("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "griz-search-benchmark", "version": "1"},
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


def verify(page, root, mode):
    assert (page["total"], page["files"], page["next"]) == (64, 1, None), page
    assert len(page["matches"]) == 64
    for index, match in enumerate(page["matches"]):
        start = index * 114695 + 114688
        assert match["path"] == str(root / "sparse.txt")
        assert match["range"] == {"start": start, "end": start + 6}
        assert (match["line"], match["column"], match["text"]) == (
            (index + 1) * 4097, 1, "needle",
        )
        assert match["file_hash"] == EXPECTED_HASH
        assert match["captures"] == (["needle"] if mode == "regex" else [])


def measure(sessions, root, count):
    rows = {}
    for mode, expression in [("literal", {"literal": "needle"}), ("regex", {"regex": "(needle)"})]:
        samples = {name: [] for name in sessions}
        arguments = {"root": str(root), "paths": ["sparse.txt"], "limit": 64, **expression}
        for index in range(count + 1):
            outputs = {}
            for name in (["before", "after"] if index % 2 == 0 else ["after", "before"]):
                start = time.perf_counter_ns()
                reply = sessions[name].call("tools/call", {"name": "find", "arguments": arguments})
                samples[name].append(time.perf_counter_ns() - start)
                assert not reply["result"].get("isError"), reply
                outputs[name] = json.loads(reply["result"]["content"][0]["text"])
                verify(outputs[name], root, mode)
            assert outputs["before"] == outputs["after"]
        before = statistics.median(samples["before"][1:])
        after = statistics.median(samples["after"][1:])
        rows[mode] = {
            "before_median_ns": before, "after_median_ns": after,
            "speedup": before / after,
            "before_first_request_ns": samples["before"][0],
            "after_first_request_ns": samples["after"][0],
        }
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=21)
    args = parser.parse_args()
    if args.samples < 3:
        parser.error("--samples must be at least 3")
    with tempfile.TemporaryDirectory(prefix="griz-search-benchmark-") as directory:
        root = Path(directory).resolve()
        env = {**os.environ, "GRIZ_HOME": str(root / "state"),
               "XDG_DATA_HOME": str(root / "data"), "GRIZ_READER_IDLE_MS": "200"}
        before, after = str(args.before.resolve()), str(args.after.resolve())
        fixture(after, root, env)
        sessions = {}
        try:
            sessions["before"] = Mcp(before, root, env)
            sessions["after"] = Mcp(after, root, env)
            rows = measure(sessions, root, args.samples)
            print(json.dumps({"bytes": 7340480, "samples": args.samples, "mcp": rows}, indent=2))
        finally:
            for session in sessions.values():
                session.close()


if __name__ == "__main__":
    main()
