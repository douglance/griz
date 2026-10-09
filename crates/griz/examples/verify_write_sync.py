#!/usr/bin/env python3
"""Observe real Darwin synchronization ordering across multi-file applies."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import measure_write_latency as benchmark


def verify(binary, library, count):
    with tempfile.TemporaryDirectory(prefix="griz-sync-proof-") as directory:
        base = Path(directory).resolve()
        root = base / "repo"
        root.mkdir()
        env = {**os.environ, "GRIZ_HOME": str(base / "state"),
               "XDG_DATA_HOME": str(base / "data"), "GRIZ_READER_IDLE_MS": "200"}
        api = benchmark.Api(str(binary), root, env, "cli")
        texts = [benchmark.content(i, 4096, 0, "A") for i in range(count)]
        api.find("batch", count)
        plan = api.mutation("plan", {"root": str(root), "ops": [
            {"op": "create", "path": f"batch/{i:04d}.txt", "text": text}
            for i, text in enumerate(texts)
        ]}, "create-plan")
        api.mutation("apply", {"plan": plan["id"]}, "create-apply")
        plan = api.mutation("plan", {"root": str(root), "ops": [
            {"op": "replace", "path": f"batch/{i:04d}.txt",
             "range": {"start": 4094, "end": 4095},
             "replace": "B", "expect_hash": benchmark.digest(text)}
            for i, text in enumerate(texts)
        ]}, "replace-plan")
        trace = base / "trace.jsonl"
        api.env = {**env, "DYLD_INSERT_LIBRARIES": str(library),
                   "GRIZ_SYNC_TRACE": str(trace)}
        applied = api.mutation("apply", {"plan": plan["id"]}, "replace-apply")
        benchmark.verify_files(root, "batch", [text[:-2] + "B\n" for text in texts])
        expected_names = {f".{i:04d}.txt" for i in range(count)}
        entries = [json.loads(line) for line in trace.read_text().splitlines()]
        assert not any(e["kind"] == "unsupported_fcntl" for e in entries), entries
        source = [
            (i, entry) for i, entry in enumerate(entries)
            if Path(entry["path"]).parent == root / "batch"
        ]
        synced = [(i, e) for i, e in source if e["kind"] == "fsync" and e["result"] == 0]
        renamed = [(i, e) for i, e in source if e["kind"] == "rename" and e["result"] == 0]
        name = lambda entry: Path(entry["path"]).name.split(".griz-")[0]
        assert {name(e) for _, e in synced} == expected_names, synced
        assert {name(e) for _, e in renamed} == expected_names, renamed
        assert len(synced) == len(renamed) == count
        first_rename = min(i for i, _ in renamed)
        devices = {os.stat(root / "batch" / f"{i:04d}.txt").st_dev for i in range(count)}
        for device in devices:
            last_file_sync = max(i for i, e in synced if e["device"] == device)
            full = [i for i, e in source if e["kind"] == "full_sync"
                    and e["device"] == device and e["result"] == 0]
            assert len(full) == 1 and last_file_sync < full[0] < first_rename, {'device': device, 'file_syncs': len(synced), 'full_sync_indices': full, 'first_rename': first_rename}
        benchmark.verify_operation(base / "state", applied["id"], root, "batch",
                                   texts, [text[:-2] + "B\n" for text in texts])
        return {"files": count, "file_syncs": len(synced),
                "device_flushes": sum(e["kind"] == "full_sync" and e["result"] == 0 for _, e in source), "sync_before_rename_verified": True}



def verify_blob_groups(binary, library):
    with tempfile.TemporaryDirectory(prefix="griz-blob-buffer-proof-") as directory:
        base = Path(directory).resolve()
        root = base / "repo"
        root.mkdir()
        trace = base / "trace.jsonl"
        env = {**os.environ, "GRIZ_HOME": str(base / "state"),
               "XDG_DATA_HOME": str(base / "data"), "GRIZ_READER_IDLE_MS": "200",
               "DYLD_INSERT_LIBRARIES": str(library), "GRIZ_SYNC_TRACE": str(trace)}
        api = benchmark.Api(str(binary), root, env, "cli")
        api.find("batch", 6)
        texts = [benchmark.content(i, 16 * 1024 * 1024, 0, "A") for i in range(6)]
        hashes = [benchmark.digest(text) for text in texts]
        sizes = {base / "state" / "blobs" / h[:2] / h[2:]: len(text)
                 for h, text in zip(hashes, texts)}
        plan = api.mutation("plan", {"root": str(root), "ops": [
            {"op": "create", "path": f"batch/{i:04d}.txt", "text": text}
            for i, text in enumerate(texts)
        ]}, "bounded-plan")
        entries = [json.loads(line) for line in trace.read_text().splitlines()]
        assert not any(e["kind"] == "unsupported_fcntl" for e in entries)
        blob_entries = [(i, e) for i, e in enumerate(entries)
                        if Path(e["path"]).parent.parent == base / "state" / "blobs"]
        full = [i for i, e in blob_entries if e["kind"] == "full_sync" and e["result"] == 0]
        assert len(full) == 2, {"successful_full_syncs": len(full), "expected": 2}
        final_path = lambda e: Path(e["path"]).with_name(
            Path(e["path"]).name[1:].split(".griz-")[0])
        synced = [(i, e) for i, e in blob_entries if e["kind"] == "fsync" and e["result"] == 0]
        renamed = [(i, e) for i, e in blob_entries if e["kind"] == "rename" and e["result"] == 0]
        assert {final_path(e) for _, e in synced} == set(sizes)
        assert {final_path(e) for _, e in renamed} == set(sizes)
        assert len(synced) == len(renamed) == 6
        previous = -1
        group_bytes = []
        for flush in full:
            group = [(i, e) for i, e in synced if previous < i < flush]
            total = sum(sizes[final_path(e)] for _, e in group)
            assert 0 < total <= 64 * 1024 * 1024, total
            for _, entry in group:
                rename = next(i for i, e in renamed if e["path"] == entry["path"])
                assert flush < rename
            group_bytes.append(total)
            previous = flush
        assert sum(group_bytes) == 96 * 1024 * 1024
        uri = (base / "state" / "griz.sqlite3").as_uri() + "?mode=ro"
        with benchmark.sqlite3.connect(uri, uri=True) as database:
            row = database.execute("SELECT body FROM plans WHERE id = ?", (plan["id"],)).fetchone()
        assert row is not None and len(json.loads(row[0])["files"]) == 6
        for path, text in zip(sizes, texts):
            assert path.read_bytes() == text.encode()
        return {"planned_bytes": sum(group_bytes), "observed_group_bytes": group_bytes,
                "successful_device_flushes": len(full), "bounded_blob_groups_verified": True}


def verify_rename_errors(binary, library):
    with tempfile.TemporaryDirectory(prefix="griz-commit-error-proof-") as directory:
        base = Path(directory).resolve()
        root = base / "repo"
        root.mkdir()
        env = {**os.environ, "GRIZ_HOME": str(base / "state"),
               "XDG_DATA_HOME": str(base / "data"), "GRIZ_READER_IDLE_MS": "200"}
        api = benchmark.Api(str(binary), root, env, "cli")
        count = 64
        texts = [benchmark.content(i, 4096, 0, "A") for i in range(count)]
        after = [text[:-2] + "B\n" for text in texts]
        api.find("batch", count)
        plan = api.mutation("plan", {"root": str(root), "ops": [
            {"op": "create", "path": f"batch/{i:04d}.txt", "text": text}
            for i, text in enumerate(texts)
        ]}, "create-plan")
        api.mutation("apply", {"plan": plan["id"]}, "create-apply")
        plan = api.mutation("plan", {"root": str(root), "ops": [
            {"op": "replace", "path": f"batch/{i:04d}.txt",
             "range": {"start": 4094, "end": 4095}, "replace": "B",
             "expect_hash": benchmark.digest(text)}
            for i, text in enumerate(texts)
        ]}, "replace-plan")
        trace = base / "trace.jsonl"
        injected = {**env, "DYLD_INSERT_LIBRARIES": str(library),
                    "GRIZ_SYNC_TRACE": str(trace), "GRIZ_RENAME_FAIL_AFTER": "1"}
        output = subprocess.run([
            str(binary), "apply", plan["id"], "--purpose", "Probe source commit failure",
            "--idempotency-key", "commit-error", "--format", "json"
        ], cwd=root, env=injected, capture_output=True, text=True)
        assert output.returncode == 1 and json.loads(output.stdout)["code"] == "STORE_ERROR"
        benchmark.verify_files(root, "batch", [after[0], *texts[1:]])
        assert not list(root.rglob("*.griz-*.tmp")), "owned stages remain after worker failure"
        entries = [json.loads(line) for line in trace.read_text().splitlines()]
        assert not any(e["kind"] == "unsupported_fcntl" for e in entries)
        source = [e for e in entries if Path(e["path"]).parent == root / "batch"]
        success = [e for e in source if e["kind"] == "rename" and e["result"] == 0]
        errors = [e for e in source if e["kind"] == "rename_injected_error" and e["result"] == -1]
        assert len(success) == 1 and len(errors) >= 2, {"successful": len(success), "errors": len(errors)}
        uri = (base / "state" / "griz.sqlite3").as_uri() + "?mode=ro"
        with benchmark.sqlite3.connect(uri, uri=True) as database:
            row = database.execute(
                "SELECT state, body FROM operations ORDER BY rowid DESC LIMIT 1"
            ).fetchone()
        assert row is not None and row[0] == "applying", row
        record = json.loads(row[1])
        log = api.call("log", {})
        assert log["operations"][0]["id"] == record["id"]
        assert log["operations"][0]["state"] == "applied"
        benchmark.verify_files(root, "batch", after)
        benchmark.verify_operation(base / "state", record["id"], root, "batch", texts, after)
        assert not list(root.rglob("*.griz-*.tmp"))
        return {"files": count, "successful_first_commit": len(success),
                "injected_worker_errors": len(errors), "stage_cleanup_verified": True,
                "applying_before_recovery": True, "recovered_bytes_and_journal_verified": True}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--files", type=int, nargs="+", choices=[32, 256], default=[32, 256])
    parser.add_argument("--blob-groups", action="store_true")
    parser.add_argument("--rename-errors", action="store_true")
    args = parser.parse_args()
    rows = [verify(args.binary.resolve(), args.library.resolve(), count) for count in args.files]
    if args.blob_groups:
        rows.append(verify_blob_groups(args.binary.resolve(), args.library.resolve()))
    if args.rename_errors:
        rows.append(verify_rename_errors(args.binary.resolve(), args.library.resolve()))
    print(json.dumps(rows))


if __name__ == "__main__":
    main()
