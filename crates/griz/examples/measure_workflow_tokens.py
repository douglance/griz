#!/usr/bin/env python3
"""Measure edit request/response text, excluding model reasoning and tool schemas.

Run with: uv run --with tiktoken==0.12.0 python measure_workflow_tokens.py
          --griz /path/to/griz --output /path/to/results
Fixtures are disposable. The bash baseline lacks griz's atomicity and journal.
"""

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

import tiktoken

OLD, NEW = "old_name(", "new_name("
BASH = """from pathlib import Path
for path in Path('.').glob('file-*.rs'):
    text = path.read_text()
    path.write_text(text.replace('old_name(', 'new_name('))
"""


def fixture(name, index):
    return (f"pub fn {name}() -> i32 {{ {index} }}\n"
            f"pub fn run() -> i32 {{\n"
            f"    let a = {name}();\n"
            f"    let b = {name}();\n"
            f"    a + b\n}}\n")


def checked(argv, root, env, trace, request=None):
    result = subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True)
    trace.append({
        "request": request if request is not None else shlex.join(argv),
        "stdout": result.stdout, "stderr": result.stderr,
    })
    if result.returncode:
        raise RuntimeError(f"{argv[0]} exited {result.returncode}: {result.stdout}{result.stderr}")
    return result


def run_case(method, count, binary):
    trace = []
    with tempfile.TemporaryDirectory() as temporary:
        parent = Path(temporary)
        root = parent / "work"
        root.mkdir()
        expected = {}
        patches = []
        for index in range(count):
            path = f"file-{index:03}.rs"
            before, after = fixture("old_name", index), fixture("new_name", index)
            assert before.count(OLD) == 3 and after.count(NEW) == 3
            (root / path).write_text(before)
            expected[path] = hashlib.sha256(after.encode()).hexdigest()
            patches.extend(difflib.unified_diff(
                before.splitlines(True), after.splitlines(True),
                fromfile="a/" + path, tofile="b/" + path,
            ))
        checker = ("import hashlib\nfrom pathlib import Path\n"
                   f"expected = {expected!r}\n"
                   "assert {p.name for p in Path('.').glob('file-*.rs')} == set(expected)\n"
                   "for path, digest in expected.items():\n"
                   "    assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest\n")
        (root / "check.py").write_text(checker)
        env = dict(os.environ, GRIZ_HOME=str(parent / "state"),
                   XDG_DATA_HOME=str(parent / "xdg"))
        for key in ("GRIZ_VERBOSITY", "GRIZ_FAILPOINT"):
            env.pop(key, None)
        if method == "patch":
            checked(["rg", "-n", "-F", OLD, "--glob", "*.rs", "."], root, env, trace)
            patch = "".join(patches)
            (root / "change.patch").write_text(patch)
            checked(["git", "apply", "change.patch"], root, env, trace,
                    request="git apply change.patch\n" + patch)
            checked([sys.executable, "check.py"], root, env, trace)
        elif method == "bash":
            checked([sys.executable, "-c", BASH], root, env, trace)
            checked([sys.executable, "check.py"], root, env, trace)
        else:
            script = str(Path(__file__).with_name("guarded_cli.py"))
            result = checked([
                sys.executable, script, "--griz", binary, "--root", ".",
                "--path", ".", "--literal", OLD, "--replace", NEW,
                "--expected-matches", str(count * 3), "--key", "benchmark",
                "--check", sys.executable, "check.py",
            ], root, env, trace)
            assert json.loads(result.stdout)["outcome"] == "passed"
        for path, digest in expected.items():
            assert hashlib.sha256((root / path).read_bytes()).hexdigest() == digest
    return trace


def measure(binary, output):
    encodings = {name: tiktoken.get_encoding(name)
                 for name in ("cl100k_base", "o200k_base")}
    rows, transcripts = [], []
    for count in (1, 8, 32):
        for method in ("patch", "bash", "griz"):
            trace = run_case(method, count, binary)
            transcripts.append({"files": count, "method": method, "trace": trace})
            tokens = {}
            for name, encoding in encodings.items():
                request = sum(len(encoding.encode(x["request"], disallowed_special=())) for x in trace)
                response = sum(len(encoding.encode(x["stdout"] + x["stderr"],
                                                  disallowed_special=())) for x in trace)
                tokens[name] = {"request": request, "response": response,
                                "total": request + response}
            rows.append({"files": count, "replacements": count * 3,
                         "method": method, "tokens": tokens})
    output.mkdir(parents=True, exist_ok=True)
    (output / "transcripts.json").write_text(json.dumps(transcripts, indent=2))
    report = {
        "rows": rows, "tokenizer": "tiktoken " + tiktoken.__version__,
        "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
        "scope": "Executed request/response text only. Excludes fixture/check setup, "
                 "model reasoning, tool schemas, transport envelopes, caching, and billing. "
                 "Patch request includes its full diff. Bash has no snapshot guard or journal.",
    }
    (output / "measurement.json").write_text(json.dumps(report, indent=2))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--griz", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(measure(str(Path(args.griz).resolve()), args.output), indent=2))
