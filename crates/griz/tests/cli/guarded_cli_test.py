"""Run the shipped CLI example against an isolated real griz binary."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import shlex
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

BINARY = str(Path(sys.argv.pop(1)).resolve())
EXAMPLE = Path(__file__).resolve().parents[2] / "examples" / "guarded_cli.py"
spec = importlib.util.spec_from_file_location("guarded_cli", EXAMPLE)
example = importlib.util.module_from_spec(spec)
spec.loader.exec_module(example)
REAL_RUN = subprocess.run


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "work with spaces"
        self.root.mkdir()
        (self.root / ".git").mkdir()
        self.file = self.root / "sample file.txt"
        self.file.write_text("oldName\n")
        self.marker = self.root / "checked"
        self.env = patch.dict(os.environ, {
            "GRIZ_HOME": str(Path(self.temp.name) / "state"),
            "XDG_DATA_HOME": str(Path(self.temp.name) / "xdg"),
        })
        self.env.start()
        self.addCleanup(self.env.stop)
        for name in ("GRIZ_VERBOSITY", "GRIZ_FAILPOINT"):
            os.environ.pop(name, None)
        self.options = SimpleNamespace(
            griz=BINARY, root=str(self.root), path=[self.file.name],
            literal="oldName", replace="new_name", expected_matches=1, key="test",
            show_check_output=False,
            check=[sys.executable, "-c",
                   "from pathlib import Path; Path('checked').write_text('ran')"],
        )
        self.calls = []

    def workflow(self, intercept=None):
        def invoke(argv, **kwargs):
            stage = argv[1] if argv[0] == BINARY else "check"
            self.calls.append(stage)
            if intercept:
                result = intercept(stage, argv, kwargs)
                if result is not None:
                    return result
            return REAL_RUN(argv, **kwargs)
        with patch.object(example.subprocess, "run", side_effect=invoke):
            return example.run(self.options)

    def assert_stopped(self, result, stage, calls):
        self.assertEqual(result["stage"], stage, result)
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(self.calls, calls)
        self.assertEqual(self.file.read_text(), "oldName\n")
        self.assertFalse(self.marker.exists())

    def test_success_and_repeated_paths(self):
        second = self.root / "another file.txt"
        second.write_text("oldName\n")
        self.options.path.append(second.name)
        self.options.expected_matches = 2
        result = self.workflow()
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.calls, ["find", "plan", "apply", "check"])
        self.assertEqual(self.file.read_text(), "new_name\n")
        self.assertEqual(second.read_text(), "new_name\n")
        self.assertTrue(self.marker.exists())



    def test_large_plan_uses_cleaned_up_input_file(self):
        self.file.write_text("oldName\n" * 128)
        self.options.expected_matches = 128
        staged = []
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                argument = argv[argv.index("--ops") + 1]
                self.assertTrue(argument.startswith("@"), "operations still passed inline")
                payload = Path(argument[1:])
                staged.append(payload)
                operations = json.loads(payload.read_text())
                self.assertEqual(len(operations), 128)
                self.assertTrue(all(op["expect_hash"] for op in operations))
                self.assertLess(sum(len(arg.encode()) for arg in argv), 4096)
        result = self.workflow(intercept)
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.file.read_text(), "new_name\n" * 128)
        self.assertEqual(len(staged), 1)
        self.assertFalse(staged[0].exists())
        self.assertFalse(staged[0].parent.exists())

    def test_rejected_plan_cleans_up_input_file(self):
        staged = []
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                argument = argv[argv.index("--ops") + 1]
                self.assertTrue(argument.startswith("@"), "operations still passed inline")
                staged.append(Path(argument[1:]))
                argv[argv.index("--expect-edits") + 1] = "2"
        result = self.workflow(intercept)
        self.assert_stopped(result, "plan", ["find", "plan"])
        self.assertFalse(staged[0].exists())
        self.assertFalse(staged[0].parent.exists())
        self.assertIn("expected 2 edits", result["stdout"])

    def test_plan_input_write_failure_stops_before_apply(self):
        with patch.object(example.Path, "write_text", side_effect=PermissionError("no space for input")):
            result = self.workflow()
        self.assert_stopped(result, "plan", ["find"])
        self.assertIn("no space for input", result["reason"])

    def test_successful_check_output_is_compact(self):
        self.options.check = [sys.executable, "-c",
                              "import sys; print('é' * 10000); print('warning', file=sys.stderr)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "passed", result)
        self.assertNotIn("stdout", result["check"])
        self.assertNotIn("stderr", result["check"])
        self.assertEqual(result["check"]["stdout_bytes"], 20001)
        self.assertEqual(result["check"]["stderr_bytes"], 8)
        self.assertLess(len(json.dumps(result)), 1000)
        self.assertEqual(self.file.read_text(), "new_name\n")

    def test_successful_check_output_can_be_requested(self):
        self.options.show_check_output = True
        self.options.check = [sys.executable, "-c",
                              "import sys; print('checked'); print('warning', file=sys.stderr)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(result["check"]["stdout"], "checked\n")
        self.assertEqual(result["check"]["stderr"], "warning\n")

    def test_cli_can_show_successful_check_output(self):
        result = REAL_RUN([
            sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
            "--root", str(self.root), "--path", self.file.name,
            "--literal", "oldName", "--replace", "new_name",
            "--expected-matches", "1", "--key", "output",
            "--show-check-output", "--check", sys.executable, "-c", "print('checked')",
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertEqual(json.loads(result.stdout)["check"]["stdout"], "checked\n")

    def test_exit_guard_stops_even_with_passed_json(self):
        def intercept(stage, argv, kwargs):
            if stage == "find":
                output = REAL_RUN(argv, **kwargs)
                output.returncode = 7
                output.stderr = "original transport failure"
                return output
        result = self.workflow(intercept)
        self.assert_stopped(result, "find", ["find"])
        self.assertEqual(result["exit_code"], 7)
        self.assertEqual(result["stderr"], "original transport failure")
        self.assertEqual(json.loads(result["stdout"])["outcome"], "passed")

    def test_verdict_guard_stops_even_with_zero_exit_and_plan_id(self):
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                output = REAL_RUN(argv, **kwargs)
                body = json.loads(output.stdout)
                body["outcome"] = "failed"
                output.stdout = json.dumps(body)
                return output
        result = self.workflow(intercept)
        self.assert_stopped(result, "plan", ["find", "plan"])
        self.assertTrue(json.loads(result["stdout"])["id"].startswith("plan_"))

    def test_malformed_responses_never_reach_apply(self):
        for value in ("not JSON", "[]", '{"outcome":"passed","id":"outcome: passed"}',
                      '{"outcome":"passed"}'):
            with self.subTest(value=value):
                self.calls.clear()
                def intercept(stage, argv, kwargs):
                    if stage == "plan":
                        return subprocess.CompletedProcess(argv, 0, value, "retained")
                result = self.workflow(intercept)
                self.assert_stopped(result, "plan", ["find", "plan"])
                self.assertEqual(result["stdout"], value)
                self.assertEqual(result["stderr"], "retained")

    def test_find_expectation_failure(self):
        self.options.expected_matches = 2
        result = self.workflow()
        self.assert_stopped(result, "find", ["find"])
        self.assertIn("expected 2 matches", result["stdout"])

    def test_plan_expectation_failure_keeps_inspection_id(self):
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                argv[argv.index("--expect-edits") + 1] = "2"
        result = self.workflow(intercept)
        self.assert_stopped(result, "plan", ["find", "plan"])
        self.assertIn("expected 2 edits", result["stdout"])
        self.assertTrue(json.loads(result["stdout"])["id"].startswith("plan_"))

    def test_stale_apply_does_not_run_check(self):
        def intercept(stage, argv, kwargs):
            if stage == "apply":
                self.file.write_text("outside edit\n")
        result = self.workflow(intercept)
        self.assertEqual(result["stage"], "apply", result)
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(self.calls, ["find", "plan", "apply"])
        self.assertEqual(self.file.read_text(), "outside edit\n")
        self.assertFalse(self.marker.exists())

    def test_failed_check_restores_and_preserves_output(self):
        self.options.check = [sys.executable, "-c",
                              "import sys; print('bad check'); print('diagnostic', file=sys.stderr); sys.exit(3)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "failed", result)
        self.assertEqual(result["check"]["stdout"], "bad check\n")
        self.assertEqual(result["check"]["stderr"], "diagnostic\n")
        self.assertEqual(result["check"]["exit_code"], 3)
        self.assertEqual(result["undo"]["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "oldName\n")
        self.assertEqual(self.calls, ["find", "plan", "apply", "check", "undo"])

    def test_failed_check_reports_refused_undo(self):
        self.options.check = [sys.executable, "-c",
                              "from pathlib import Path; import sys; "
                              "Path('sample file.txt').write_text('outside edit'); "
                              "print('bad check'); sys.exit(1)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "failed", result)
        self.assertEqual(result["check"]["stdout"], "bad check\n")
        self.assertEqual(result["undo"]["outcome"], "error")
        self.assertIn("stdout", result["undo"])
        self.assertEqual(self.file.read_text(), "outside edit")

    def test_missing_input_preserves_error_instead_of_key_error(self):
        with self.assertRaises(example.CommandFailure) as caught:
            example.griz_command(self.options, "plan", [
                "--root", str(self.root), "--ops", "@missing.json",
                *example.mutation_options(self.options, "missing"),
            ], "plan")
        result = caught.exception.report
        self.assertIn("missing.json", result["stdout"])
        self.assertIn("VALIDATION_ERROR", result["stdout"])
        self.assertEqual(self.file.read_text(), "oldName\n")

    def test_cli_entrypoint(self):
        result = REAL_RUN([
            sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
            "--root", str(self.root), "--path", self.file.name,
            "--literal", "oldName", "--replace", "new_name",
            "--expected-matches", "1", "--key", "entry", "--check",
            *self.options.check,
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
        self.assertTrue(self.marker.exists())

    def test_invalid_check_keeps_operation_for_recovery(self):
        self.options.check = [str(self.root / "missing-check")]
        result = self.workflow()
        self.assertEqual(result["outcome"], "error")
        self.assertTrue(result["operation"].startswith("op_"))
        self.assertEqual(self.file.read_text(), "new_name\n")



    def test_malformed_matches_keep_response(self):
        body = {"outcome": "passed", "matches": [{}]}
        def intercept(stage, argv, kwargs):
            if stage == "find":
                return subprocess.CompletedProcess(argv, 0, json.dumps(body), "")
        result = self.workflow(intercept)
        self.assert_stopped(result, "find", ["find"])
        self.assertEqual(result["response"], body)

    def mcp(self, name, arguments):
        messages = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "griz-usage-test", "version": "1"},
            }},
            {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
             "params": {"name": name, "arguments": arguments}},
        ]
        result = REAL_RUN([BINARY, "--mcp"], cwd=self.root, text=True,
                          input="".join(json.dumps(m) + "\n" for m in messages),
                          capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        responses = {r["id"]: r for r in map(json.loads, result.stdout.splitlines())
                     if "id" in r}
        self.assertNotIn("error", responses[2], responses[2])
        body = responses[2]["result"]
        self.assertFalse(body.get("isError"), body)
        return json.loads(body["content"][0]["text"])

    def test_cli_and_mcp_paths_and_ids_agree(self):
        second = self.root / "another file.txt"
        second.write_text("oldName\n")
        arguments = ["--root", str(self.root), "--paths", self.file.name,
                     "--paths", second.name, "--literal", "oldName",
                     "--expect-matches", "2"]
        cli = example.griz_command(self.options, "find", arguments)
        mcp = self.mcp("find", {
            "root": str(self.root), "paths": [self.file.name, second.name],
            "literal": "oldName", "expect_matches": 2,
        })
        self.assertEqual(cli, mcp)
        result = self.workflow()
        operation = result["operation"]
        cli_record = REAL_RUN([BINARY, "get", operation, "--format", "json"],
                              capture_output=True, text=True)
        self.assertEqual(cli_record.returncode, 0, cli_record.stderr)
        self.assertEqual(json.loads(cli_record.stdout), self.mcp("get", {"id": operation}))

    def test_rejected_spellings_stay_rejected(self):
        cases = [
            ["apply", "--plan", "plan_invalid", "--purpose", "probe",
             "--idempotency-key", "probe"],
            ["get", "plan_invalid", "--purpose", "probe"],
            ["get", "plan_invalid", "--verbosity", "debug"],
        ]
        for args in cases:
            result = REAL_RUN([BINARY, *args, "--format", "json"],
                              cwd=self.root, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(json.loads(result.stdout)["code"], "VALIDATION_ERROR")
        with self.assertRaises(example.CommandFailure):
            example.griz_command(self.options, "find", [
                "--root", str(self.root), "--paths", json.dumps([self.file.name]),
                "--literal", "oldName", "--expect-matches", "1",
            ])
        self.assertEqual(self.file.read_text(), "oldName\n")

    def test_help_exposes_canonical_examples(self):
        for name in ("read", "find", "plan", "select", "diff",
                     "apply", "undo", "absorb", "log", "get"):
            result = REAL_RUN([BINARY, name, "--help"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("--format json", result.stdout)
            self.assertIn("Examples:", result.stdout)
        result = REAL_RUN([BINARY, "get", "--help"], capture_output=True, text=True)
        self.assertIn("already returns the complete record", result.stdout)



    def test_printed_examples_are_executable(self):
        for name in ("read", "find", "plan", "select", "diff",
                     "apply", "undo", "absorb", "log", "get"):
            with self.subTest(command=name):
                self.execute_help_example(name)

    def execute_help_example(self, name):
        original = "fn oldName() {}\n"
        files = ["src one.rs", "src two.rs"]
        for file in files:
            (self.root / file).write_text(original)
        ops = [{"op": "replace", "path": file, "find": "oldName",
                "replace": "new_name"} for file in files]
        (self.root / "edits.json").write_text(json.dumps(ops[:1]))
        identifiers = {}
        if name in ("select", "diff", "apply", "undo", "absorb", "get"):
            selected = ops if name in ("undo", "absorb") else ops[:1]
            plan = example.griz_command(self.options, "plan", [
                "--root", str(self.root), "--ops", json.dumps(selected),
                *example.mutation_options(self.options, "prepare-" + name),
            ], "plan")
            identifiers = {"PLAN": plan["id"], "ID": plan["id"]}
            if name in ("undo", "absorb"):
                operation = example.griz_command(self.options, "apply", [
                    plan["id"], *example.mutation_options(self.options, "write-" + name),
                ], "op")
                identifiers["OP"] = operation["id"]
        help_result = REAL_RUN([BINARY, name, "--help"], capture_output=True, text=True)
        commands = [line.strip().split("  # ", 1)[0]
                    for line in help_result.stdout.splitlines()
                    if line.startswith("  griz " + name + " ")]
        self.assertEqual(len(commands), 1, help_result.stdout)
        argv = shlex.split(commands[0])
        argv[0] = BINARY
        substitutions = {"/path/to/repo": str(self.root), **identifiers}
        argv = [substitutions.get(arg, arg) for arg in argv]
        result = REAL_RUN(argv, cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        if name in ("plan", "select", "apply", "undo", "absorb"):
            self.assertEqual(json.loads(result.stdout)["outcome"], "passed")


if __name__ == "__main__":
    unittest.main()
