#!/usr/bin/env python3
"""Alternate baseline and candidate calls, verifying every write and undo."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import tempfile
import time
import measure_write_latency as bench


def setup(binary, base, case, count, transport):
    base = base.resolve()
    root = base / "repo"
    root.mkdir(parents=True)
    home = base / "state"
    env = {**os.environ, "GRIZ_HOME": str(home), "XDG_DATA_HOME": str(base / "data"),
           "GRIZ_READER_IDLE_MS": "200"}
    api = bench.Api(str(binary), root, env, transport)
    before = [bench.content(i, 4096, 0, "A") for i in range(count)]
    assert api.find(case, count)["total"] == 0
    plan = api.mutation("plan", {"root": str(root), "ops": [
        {"op": "create", "path": f"{case}/{i:04d}.txt", "text": text}
        for i, text in enumerate(before)
    ]}, case + "-initial-plan")
    api.mutation("apply", {"plan": plan["id"]}, case + "-initial-apply")
    bench.verify_files(root, case, before)
    return api, home, before


def trial(api, home, before, case, index):
    count = len(before)
    found, find_ns = bench.timed(lambda: api.find(case, count))
    assert (found["total"], found["files"], found["next"]) == (count, count, None)
    known = {item["path"]: item for item in found["file_matches"]}
    after = [bench.content(i, 4096, index + 1, "B") for i in range(count)]
    ops = []
    for i, (old, new) in enumerate(zip(before, after)):
        path = str(api.root / case / f"{i:04d}.txt")
        assert known[path]["file_hash"] == bench.digest(old)
        tail = f"revision {index + 1:08d}B\n"
        ops.append({"op": "replace", "path": path,
                    "range": {"start": 4096 - len(tail), "end": 4096},
                    "replace": tail, "expect_hash": known[path]["file_hash"]})
    plan, plan_ns = bench.timed(lambda: api.mutation("plan", {
        "root": str(api.root), "ops": ops, "expect_files": count,
    }, f"{case}-{index}-plan"))
    applied, apply_ns = bench.timed(lambda: api.mutation("apply", {
        "plan": plan["id"], "expect_files": count,
    }, f"{case}-{index}-apply"))
    bench.verify_files(api.root, case, after)
    bench.verify_operation(home, applied["id"], api.root, case, before, after)
    undone, undo_ns = bench.timed(lambda: api.mutation("undo", {
        "operation": applied["id"], "root": str(api.root),
    }, f"{case}-{index}-undo"))
    bench.verify_files(api.root, case, before)
    bench.verify_operation(home, undone["id"], api.root, case, after, before)
    return {"find": find_ns, "plan": plan_ns, "apply": apply_ns, "undo": undo_ns,
            "pipeline": find_ns + plan_ns + apply_ns}


def measure(binaries, count, samples, transport):
    case = f"batch_{count}"
    with tempfile.TemporaryDirectory(prefix="griz-paired-writes-") as directory:
        fixtures = [setup(binary, Path(directory) / str(i), case, count, transport)
                    for i, binary in enumerate(binaries)]
        rows = [[], []]
        try:
            for index in range(samples + 1):
                for side in ([0, 1] if index % 2 == 0 else [1, 0]):
                    row = trial(*fixtures[side], case, index)
                    if index:
                        rows[side].append(row)
                if index and index % 5 == 0:
                    print(json.dumps({"files": count, "pairs_complete": index}),
                          file=__import__("sys").stderr, flush=True)
        finally:
            for api, _, _ in fixtures:
                api.close()
    stages = rows[0][0].keys()
    medians = [{stage: statistics.median(row[stage] for row in side)
                for stage in stages} for side in rows]
    return {"files": count, "bytes_per_file": 4096, "paired_samples": samples,
            "median_ns": {"baseline": medians[0], "candidate": medians[1]},
            "speedup": {stage: medians[0][stage] / medians[1][stage] for stage in stages},
            "paired_speedup_median": {
                stage: statistics.median(a[stage] / b[stage] for a, b in zip(*rows))
                for stage in stages
            }, "contents_and_journal_verified": True}


def artifact(binary):
    paths = [binary]
    engine = binary.with_name("griz-engine")
    if engine.exists():
        paths.append(engine)
    return {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=31)
    parser.add_argument("--transport", choices=["cli", "mcp"], default="mcp")
    parser.add_argument("--files", type=int, nargs="+", choices=[32, 256], default=[32, 256])
    args = parser.parse_args()
    if args.samples < 3:
        parser.error("--samples must be at least 3")
    binaries = [args.baseline.resolve(), args.candidate.resolve()]
    hashes = [artifact(binary) for binary in binaries]
    rows = [measure(binaries, count, args.samples, args.transport) for count in args.files]
    assert hashes == [artifact(binary) for binary in binaries], "artifact changed while measuring"
    print(json.dumps({"transport": args.transport, "artifacts": hashes, "measurements": rows}))


if __name__ == "__main__":
    main()
