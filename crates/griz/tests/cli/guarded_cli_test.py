"""Run the shipped CLI example against an isolated real griz binary."""

import importlib.util
import io
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


    def caller_args(self, query, key, check):
        return [
            "--griz", BINARY, "--root", str(self.root), "--path", self.file.name,
            "--literal", query, "--expected-matches", "1", "--key", key,
            "--check", *check,
        ]


    def test_documented_caller_recipe_runs(self):
        self.file = self.root / "recipe.ts"
        self.file.write_text("foo(value)\n")
        guide = Path(__file__).resolve().parents[4] / "skills" / "griz" / "SKILL.md"
        recipe = guide.read_text().split("~~~python\n", 1)[1].split("~~~", 1)[0]
        for name, value in {
            "HELPER": str(EXAMPLE), "ROOT": str(self.root),
            "PATH": self.file.name, "N": "1", "KEY": "recipe", "BIN": BINARY,
        }.items():
            recipe = recipe.replace(json.dumps(name), repr(value))
        recipe = recipe.replace('"cargo", "check"', repr(sys.executable) + ', "-c", "pass"')
        result = REAL_RUN([sys.executable, "-c", recipe], env=os.environ,
                          capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "bar(value)\n")


    def caller_failure_receipt(self, mode):
        self.file.write_text("user note\noldName\n")
        check = "raise SystemExit(17)"
        if mode == "undo_refused":
            check = ("from pathlib import Path;p=Path(" + repr(self.file.name)
                     + ");p.write_text(p.read_text()+'concurrent\\n');raise SystemExit(17)")
        args = ["--replace", "new_name",
                *self.caller_args("oldName", mode, [sys.executable, "-c", check])]
        program = ("import runpy\nhelper = runpy.run_path(" + repr(str(EXAMPLE)) + ")\n"
                   + "arguments = " + repr(args) + "\nhelper['edit'](arguments)\n")
        result = REAL_RUN([sys.executable, "-B", "-c", program], env=os.environ,
                          input="", capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(result.stdout, "")
        payload = result.stderr.rsplit("CommandFailure: ", 1)[-1].strip()
        self.assertTrue(payload.startswith("{"), result.stderr)
        report = json.loads(payload)
        self.assertEqual(report["stage"], "check")
        self.assertEqual(report["check"]["exit_code"], 17)
        log = REAL_RUN([BINARY, "log", "--json"], cwd=self.root, env=os.environ,
                       capture_output=True, text=True, check=True)
        apply = next(op for op in json.loads(log.stdout)["operations"] if op["kind"] == "apply")
        self.assertEqual(report["operation"], apply["id"])
        return report

    def test_caller_unhandled_failure_reports_successful_undo(self):
        report = self.caller_failure_receipt("restored")
        self.assertEqual(report["undo"]["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "user note\noldName\n")

    def test_caller_unhandled_failure_reports_refused_undo(self):
        report = self.caller_failure_receipt("undo_refused")
        self.assertEqual(report["undo"]["outcome"], "error")
        self.assertEqual(report["undo"]["stage"], "undo")
        self.assertEqual(self.file.read_text(), "user note\nnew_name\nconcurrent\n")

    def test_caller_composes_callback_and_constant_edits(self):
        check = [sys.executable, "-c", "pass"]
        first = example.edit(self.caller_args("oldName", "first", check),
                             transform=lambda match: match["text"].upper())
        second = example.edit([
            "--replace", "final_name",
            *self.caller_args("OLDNAME", "second", check),
        ])
        self.assertEqual(first["outcome"], "passed")
        self.assertEqual(second["outcome"], "passed")
        self.assertNotEqual(first["operation"], second["operation"])
        self.assertEqual(self.file.read_text(), "final_name\n")

    def test_caller_failure_stops_sequence_after_guarded_undo(self):
        self.file.write_text("user note\noldName\n")
        check = [sys.executable, "-c", "pass"]
        example.edit(self.caller_args("oldName", "first", check),
                     transform=lambda match: "accepted")
        with self.assertRaises(example.CommandFailure) as caught:
            example.edit(
                self.caller_args("accepted", "rejected",
                                 [sys.executable, "-c", "raise SystemExit(17)"]),
                transform=lambda match: "rejected",
            )
            self.marker.write_text("later step ran")
        report = caught.exception.report
        self.assertEqual(report["stage"], "check")
        self.assertEqual(report["check"]["exit_code"], 17)
        self.assertEqual(report["undo"]["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "user note\naccepted\n")
        self.assertFalse(self.marker.exists())
        resumed = example.edit(self.caller_args("accepted", "resumed", check),
                               transform=lambda match: "finished")
        self.assertEqual(resumed["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "user note\nfinished\n")

    def test_caller_raises_before_later_steps_on_find_failure(self):
        with self.assertRaises(example.CommandFailure) as caught:
            example.edit(
                ["--replace", "new", *self.caller_args(
                    "missing", "missing", [sys.executable, "-c", "pass"])],
            )
            self.marker.write_text("later step ran")
        self.assertEqual(caught.exception.report["stage"], "find")
        self.assertEqual(self.file.read_text(), "oldName\n")
        self.assertFalse(self.marker.exists())

    def test_caller_rejects_ambiguous_or_invalid_callback_before_commands(self):
        base = self.caller_args("oldName", "invalid", [sys.executable, "-c", "pass"])
        cases = [
            (["--replace", "new", *base], lambda match: "new"),
            (["--transform", "-", *base], lambda match: "new"),
            (base, "not callable"),
            (["--griz", BINARY, "--root", str(self.root), "--patch", "-",
              "--expected-files", "1", "--expected-edits", "1", "--key", "patch",
              "--check", sys.executable, "-c", "pass"], lambda match: "new"),
        ]
        for args, transform in cases:
            with self.subTest(args=args):
                with (
                    patch.object(example.subprocess, "run") as invoked,
                    patch("sys.stdin", new=io.TextIOWrapper(io.BytesIO(b""))),
                ):
                    with patch("sys.stderr", new=io.StringIO()), self.assertRaises(SystemExit):
                        example.edit(args, transform=transform)
                    invoked.assert_not_called()
        self.assertEqual(self.file.read_text(), "oldName\n")

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
                self.assertEqual(len(operations), 1)
                self.assertEqual(operations[0]["occurrence"], "all")
                self.assertNotIn("range", operations[0])
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


    def test_grouped_edits_keep_independent_file_hashes(self):
        self.file.write_text("oldName\n" * 3)
        second = self.root / "second.txt"
        second.write_text("é\noldName\noldName\n")
        self.options.path.append(second.name)
        self.options.expected_matches = 5
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                payload = Path(argv[argv.index("--ops") + 1][1:])
                operations = json.loads(payload.read_text())
                self.assertEqual(len(operations), 2)
                self.assertEqual(len({op["expect_hash"] for op in operations}), 2)
                self.assertTrue(all(op["occurrence"] == "all" for op in operations))
                self.assertEqual(argv[argv.index("--expect-edits") + 1], "5")
        result = self.workflow(intercept)
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.file.read_text(), "new_name\n" * 3)
        self.assertEqual(second.read_text(), "é\nnew_name\nnew_name\n")

    def test_grouped_plan_refuses_changes_after_find(self):
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                self.file.write_text("oldName\noutside\n")
        result = self.workflow(intercept)
        self.assertEqual(result["stage"], "plan", result)
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(self.calls, ["find", "plan"])
        self.assertEqual(self.file.read_text(), "oldName\noutside\n")
        self.assertFalse(self.marker.exists())

    def test_grouping_refuses_inconsistent_fingerprints(self):
        self.file.write_text("oldName\n" * 2)
        self.options.expected_matches = 2
        def intercept(stage, argv, kwargs):
            if stage == "find":
                output = REAL_RUN(argv, **kwargs)
                body = json.loads(output.stdout)
                body["matches"][1]["file_hash"] = "f" * 64
                output.stdout = json.dumps(body).encode()
                return output
        result = self.workflow(intercept)
        self.assertEqual(result["stage"], "find", result)
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(self.calls, ["find"])
        self.assertEqual(self.file.read_text(), "oldName\n" * 2)
        self.assertFalse(self.marker.exists())

    def test_grouping_refuses_unexpected_match_text(self):
        def intercept(stage, argv, kwargs):
            if stage == "find":
                output = REAL_RUN(argv, **kwargs)
                body = json.loads(output.stdout)
                body["matches"][0]["text"] = "different"
                output.stdout = json.dumps(body).encode()
                return output
        result = self.workflow(intercept)
        self.assert_stopped(result, "find", ["find"])

    def test_grouping_requires_a_valid_fingerprint(self):
        for fingerprint in (None, "", "not-a-hash", [], "A" * 64):
            with self.subTest(fingerprint=fingerprint):
                self.calls.clear()
                def intercept(stage, argv, kwargs):
                    if stage == "find":
                        output = REAL_RUN(argv, **kwargs)
                        body = json.loads(output.stdout)
                        body["matches"][0]["file_hash"] = fingerprint
                        output.stdout = json.dumps(body).encode()
                        return output
                result = self.workflow(intercept)
                self.assert_stopped(result, "find", ["find"])


    def test_binary_check_failure_still_undoes(self):
        self.options.check = [sys.executable, "-c",
                              "import os; os.write(1, b'\\xffout\\r\\n'); "
                              "os.write(2, b'\\xfeerr\\n'); raise SystemExit(7)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "failed", result)
        self.assertEqual(result["check"]["exit_code"], 7)
        self.assertEqual(result["check"]["stdout"], "\\xffout\r\n")
        self.assertEqual(result["check"]["stderr"], "\\xfeerr\n")
        self.assertEqual(result["check"]["stdout_bytes"], 6)
        self.assertEqual(result["check"]["stderr_bytes"], 5)
        self.assertEqual(result["undo"]["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "oldName\n")
        self.assertTrue(result["operation"].startswith("op_"))
        json.dumps(result)

    def test_binary_successful_check_keeps_compact_receipt(self):
        self.options.check = [sys.executable, "-c", "import os; os.write(1, b'\\xff')"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(result["check"]["stdout_bytes"], 1)
        self.assertNotIn("stdout", result["check"])
        self.assertEqual(self.file.read_text(), "new_name\n")

    def test_binary_successful_check_can_show_escaped_output(self):
        self.options.show_check_output = True
        self.options.check = [sys.executable, "-c", "import os; os.write(2, b'\\xfe')"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(result["check"]["stderr"], "\\xfe")
        self.assertEqual(result["check"]["stderr_bytes"], 1)
        json.dumps(result)

    def test_binary_griz_output_is_a_serializable_failure(self):
        def intercept(stage, argv, kwargs):
            if stage == "find":
                return subprocess.CompletedProcess(argv, 0, b"\xff", b"\xfe")
        result = self.workflow(intercept)
        self.assert_stopped(result, "find", ["find"])
        self.assertEqual(result["stdout"], "\\xff")
        self.assertEqual(result["stderr"], "\\xfe")
        json.dumps(result)

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
                output.stderr = b"original transport failure"
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
                output.stdout = json.dumps(body).encode()
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
                        return subprocess.CompletedProcess(argv, 0, value.encode(), b"retained")
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
                return subprocess.CompletedProcess(argv, 0, json.dumps(body).encode(), b"")
        result = self.workflow(intercept)
        self.assert_stopped(result, "find", ["find"])
        self.assertEqual(result["response"], body)

    def transform_cli(self, selector, source, count=1, check=None):
        return REAL_RUN([
            sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
            "--root", str(self.root), "--path", self.file.name,
            *selector, "--transform", "-", "--expected-matches", str(count),
            "--key", "transform", "--check", *(check or self.options.check),
        ], input=source, capture_output=True, text=True)

    def test_structural_transform_preserves_decoys_and_unicode(self):
        self.file = self.root / "client file.ts"
        before = ('// é legacy(0)\nconst a = legacy("é");\n'
                  'const b = obj.legacy(1);\nconst note = "legacy(2)";\n'
                  'const c = legacy(1 + 2);\n')
        self.file.write_text(before)
        result = self.transform_cli(
            ["--pattern", "legacy($ARG)"],
            "def replace(match):\n    return 'modern(' + match['vars']['ARG'] + ')'\n",
            count=2,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
        self.assertEqual(self.file.read_text(),
                         '// é legacy(0)\nconst a = modern("é");\n'
                         'const b = obj.legacy(1);\nconst note = "legacy(2)";\n'
                         'const c = modern(1 + 2);\n')
        self.assertTrue(self.marker.exists())

    def test_json_transform_preserves_unrelated_values(self):
        self.file = self.root / "services with spaces.json"
        self.file.write_text('{"timeout": 30, "retries": 2, "note": "é"}\n')
        source = ("import json\n"
                  "def replace(match):"
                  "\n    value = json.loads(match['text'])\n"
                  "    value['timeout'] = 60\n"
                  "    value['retries'] += 1\n"
                  "    return json.dumps(value, ensure_ascii=False, indent=2) + '\\n'\n")
        result = self.transform_cli(["--regex", r"(?s)\A.*\z"], source)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.file.read_text(),
                         '{\n  "timeout": 60,\n  "retries": 3,\n  "note": "é"\n}\n')

    def test_late_transform_failure_writes_nothing(self):
        self.file.write_text("oldName oldName\n")
        source = ("def replace(match):\n"
                  "    if match['range']['start'] > 0:\n"
                  "        raise ValueError('second match rejected')\n"
                  "    return 'new_name'\n")
        result = self.transform_cli(["--literal", "oldName"], source, count=2)
        self.assertNotEqual(result.returncode, 0)
        report = json.loads(result.stdout)
        self.assertEqual(report["stage"], "transform", report)
        self.assertIn("second match rejected", report["reason"])
        self.assertEqual(self.file.read_text(), "oldName oldName\n")
        self.assertFalse(self.marker.exists())

    def test_transform_errors_are_structured_and_write_nothing(self):
        for source in ("def broken(", "replace = 3\n",
                       "def replace(match):\n    return None\n",
                       "def replace(match):\n    return 5\n"):
            with self.subTest(source=source):
                result = self.transform_cli(["--literal", "oldName"], source)
                self.assertNotEqual(result.returncode, 0)
                report = json.loads(result.stdout)
                self.assertEqual(report["stage"], "transform", report)
                self.assertEqual(report["outcome"], "error", report)
                self.assertEqual(self.file.read_text(), "oldName\n")
                self.assertFalse(self.marker.exists())

    def test_transform_keeps_the_found_fingerprint(self):
        source = ("from pathlib import Path\n"
                  "def replace(match):\n"
                  f"    Path({str(self.file)!r}).write_text('oldName\\noutside edit\\n')\n"
                  "    return 'new_name'\n")
        result = self.transform_cli(["--literal", "oldName"], source)
        self.assertNotEqual(result.returncode, 0)
        report = json.loads(result.stdout)
        self.assertEqual(report["stage"], "plan", report)
        self.assertEqual(self.file.read_text(), "oldName\noutside edit\n")
        self.assertFalse(self.marker.exists())

    def test_transform_check_failure_restores_preexisting_content(self):
        self.file.write_text("# user's note\noldName\n")
        result = self.transform_cli(
            ["--regex", "(old)Name"],
            "def replace(match):\n    return match['captures'][0] + '_new'\n",
            check=[sys.executable, "-c", "raise SystemExit(7)"],
        )
        self.assertNotEqual(result.returncode, 0)
        report = json.loads(result.stdout)
        self.assertEqual(report["check"]["exit_code"], 7)
        self.assertEqual(report["undo"]["outcome"], "passed", report)
        self.assertEqual(self.file.read_text(), "# user's note\noldName\n")

    def test_transform_file_keeps_stdout_as_one_receipt(self):
        transform = self.root / "transform with spaces.py"
        transform.write_text("print('loaded')\ndef replace(match):\n"
                             "    print('transforming')\n    return 'new_name'\n")
        result = REAL_RUN([
            sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
            "--root", str(self.root), "--path", self.file.name,
            "--literal", "oldName", "--transform", str(transform),
            "--expected-matches", "1", "--key", "file-transform", "--check",
            *self.options.check,
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
        self.assertIn("loaded", result.stderr)
        self.assertIn("transforming", result.stderr)
        self.assertEqual(self.file.read_text(), "new_name\n")

    def test_regex_constant_replacement_and_language_selection(self):
        self.file = self.root / "sample file.ts"
        for query, before, expected in (
            (["--regex", r"oldN\w+"], "oldName\n", "new_name\n"),
            (["--pattern", "legacy($ARG)", "--language", "typescript"],
             "const a = legacy(1);\n", "const a = new_name;\n"),
        ):
            with self.subTest(query=query):
                self.file.write_text(before)
                result = REAL_RUN([
                    sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
                    "--root", str(self.root), "--path", self.file.name,
                    *query, "--replace", "new_name", "--expected-matches", "1",
                    "--key", "selector-" + query[0], "--check", *self.options.check,
                ], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(self.file.read_text(), expected)

    def test_transform_cannot_retarget_its_match(self):
        source = ("def replace(match):\n"
                  "    match['path'] = 'wrong.txt'\n"
                  "    match['range']['start'] = 99\n"
                  "    match['file_hash'] = None\n"
                  "    match['text'] = 'wrong'\n"
                  "    return 'new_name'\n")
        result = self.transform_cli(["--literal", "oldName"], source)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.file.read_text(), "new_name\n")
        self.assertFalse((self.root / "wrong.txt").exists())

    def test_transform_keeps_unchanged_matches_valid(self):
        self.file.write_text("oldName unchanged\n")
        source = ("def replace(match):\n"
                  "    return 'new_name' if match['text'] == 'oldName' else match['text']\n")
        result = self.transform_cli(["--regex", r"oldName|unchanged"], source, count=2)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.file.read_text(), "new_name unchanged\n")

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



    def ops_options(self, text=None, files=2, edits=2):
        payload = self.root / "input ops.json"
        payload.write_text(text if text is not None else json.dumps([
            {"op": "replace", "path": self.file.name, "find": "oldName", "replace": "new_name"},
            {"op": "create", "path": "created file.txt", "text": "created\n"},
        ]))
        self.options.ops = str(payload)
        self.options.expected_files = files
        self.options.expected_edits = edits
        return payload

    def test_ops_updates_and_creates_without_leaking_payload(self):
        self.ops_options()
        staged = []
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                self.assertLess(sum(len(arg.encode()) for arg in argv), 4096)
                argument = argv[argv.index("--ops") + 1]
                self.assertTrue(argument.startswith("@"))
                staged.append(Path(argument[1:]))
        result = self.workflow(intercept)
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.calls, ["plan", "apply", "check"])
        self.assertEqual(self.file.read_text(), "new_name\n")
        self.assertEqual((self.root / "created file.txt").read_text(), "created\n")
        self.assertTrue(self.marker.exists())
        self.assertEqual(len(staged), 1)
        self.assertFalse(staged[0].exists())
        replay = self.workflow()
        self.assertEqual(replay["operation"], result["operation"])


    def test_documented_ops_payload_runs(self):
        self.file = self.root / "old.txt"
        self.file.write_text("before\n")
        guide = Path(__file__).resolve().parents[4] / "skills" / "griz" / "SKILL.md"
        payload = guide.read_text().split("~~~json\n", 1)[1].split("~~~", 1)[0]
        self.ops_options(payload)
        result = self.workflow()
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.file.read_text(), "after\n")
        self.assertEqual((self.root / "new.txt").read_text(), "hello\n")

    def test_ops_cli_file_and_stdin(self):
        payload = self.ops_options()
        caller = Path(self.temp.name)
        relative = caller / "caller ops.json"
        relative.write_bytes(payload.read_bytes())
        for source in (relative.name, "-"):
            with self.subTest(source=source):
                command = [
                    sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
                    "--root", str(self.root), "--ops", source,
                    "--expected-files", "2", "--expected-edits", "2", "--key", "ops-cli",
                    "--check", *self.options.check,
                ]
                result = REAL_RUN(command, cwd=caller, input=payload.read_bytes(),
                                  capture_output=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
                self.assertEqual(self.file.read_text(), "new_name\n")
                self.assertEqual((self.root / "created file.txt").read_text(), "created\n")

    def test_ops_invalid_input_never_applies(self):
        cases = ["not json", "{}", '[{"op":"create","path":"new","text":"x","typo":true}]',
                 '[{"op":"replace","path":"sample file.txt","find":"missing","replace":"x"}]']
        for index, content in enumerate(cases):
            with self.subTest(content=content):
                self.calls.clear()
                self.ops_options(content)
                self.options.key = f"invalid-ops-{index}"
                result = self.workflow()
                self.assert_stopped(result, "plan", ["plan"])
                self.assertFalse((self.root / "created file.txt").exists())

    def test_ops_count_and_syntax_guards_never_apply(self):
        for files, edits in ((1, 2), (2, 1)):
            with self.subTest(files=files, edits=edits):
                self.calls.clear()
                self.ops_options(files=files, edits=edits)
                self.options.key = f"ops-counts-{files}-{edits}"
                self.assert_stopped(self.workflow(), "plan", ["plan"])
                self.assertFalse((self.root / "created file.txt").exists())
        self.calls.clear()
        self.ops_options(json.dumps([{"op": "create", "path": "bad.py", "text": "def broken(\n"}]),
                         files=1, edits=1)
        self.options.key = "ops-syntax"
        self.assert_stopped(self.workflow(), "plan", ["plan"])
        self.assertFalse((self.root / "bad.py").exists())

    def test_ops_stale_apply_stops_before_check(self):
        self.ops_options()
        def intercept(stage, argv, kwargs):
            if stage == "apply":
                self.file.write_text("outside\noldName\n")
        result = self.workflow(intercept)
        self.assertEqual(result["stage"], "apply", result)
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(self.calls, ["plan", "apply"])
        self.assertEqual(self.file.read_text(), "outside\noldName\n")
        self.assertFalse((self.root / "created file.txt").exists())
        self.assertFalse(self.marker.exists())

    def test_ops_failed_check_restores_modified_and_created_files(self):
        self.file.write_text("# user's note\noldName\n")
        self.ops_options()
        self.options.check = [sys.executable, "-c", "raise SystemExit(17)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "failed", result)
        self.assertEqual(result["check"]["exit_code"], 17)
        self.assertEqual(result["undo"]["outcome"], "passed", result)
        self.assertEqual(self.calls, ["plan", "apply", "check", "undo"])
        self.assertEqual(self.file.read_text(), "# user's note\noldName\n")
        self.assertFalse((self.root / "created file.txt").exists())

    def test_ops_input_errors_and_parser_conflicts_stop_before_commands(self):
        payload = self.ops_options()
        for content in (None, b"\xff"):
            with self.subTest(content=content):
                self.calls.clear()
                if content is None:
                    payload.unlink()
                else:
                    payload.write_bytes(content)
                self.assert_stopped(self.workflow(), "plan", [])
        base = ["--root", str(self.root), "--ops", "-", "--key", "invalid"]
        counts = ["--expected-files", "2", "--expected-edits", "2"]
        cases = [counts + [flag, value] for flag, value in [
            ("--path", self.file.name), ("--replace", "x"), ("--transform", "-"),
            ("--language", "python"), ("--expected-matches", "1"),
            ("--literal", "oldName"), ("--patch", "-"),
        ]]
        cases += [["--expected-files", "0", "--expected-edits", "2"],
                  ["--expected-files", "2", "--expected-edits", "0"],
                  ["--expected-files", "2"], ["--expected-edits", "2"]]
        for arguments in cases:
            with self.subTest(arguments=arguments), patch.object(example.subprocess, "run") as invoked:
                with patch("sys.stderr", new=io.StringIO()), self.assertRaises(SystemExit):
                    example.edit(base + arguments + ["--check", *self.options.check])
                invoked.assert_not_called()
        with patch.object(example.subprocess, "run") as invoked:
            with patch("sys.stderr", new=io.StringIO()), self.assertRaises(SystemExit):
                example.edit(base + counts + ["--check", *self.options.check],
                             transform=lambda match: "x")
            invoked.assert_not_called()

    def patch_options(self, text=None, files=1, edits=1):
        payload = self.root / "input patch.txt"
        payload.write_text(text or (
            "*** Begin Patch\n*** Update File: sample file.txt\n"
            "@@\n-oldName\n+new_name\n*** End Patch\n"
        ))
        self.options.patch = str(payload)
        self.options.expected_files = files
        self.options.expected_edits = edits
        return payload

    def test_patch_file_updates_and_creates_files(self):
        self.patch_options(
            "*** Begin Patch\n*** Update File: sample file.txt\n"
            "@@\n-oldName\n+new_name\n*** Add File: created file.txt\n"
            "+created\n*** End Patch\n", files=2, edits=2)
        staged = []
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                argument = argv[argv.index("--patch") + 1]
                self.assertTrue(argument.startswith("@"))
                staged.append(Path(argument[1:]))
        result = self.workflow(intercept)
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.calls, ["plan", "apply", "check"])
        self.assertEqual(self.file.read_text(), "new_name\n")
        self.assertEqual((self.root / "created file.txt").read_text(), "created\n")
        self.assertTrue(self.marker.exists())
        self.assertEqual(len(staged), 1)
        self.assertFalse(staged[0].exists())


    def test_large_patch_stays_file_backed_and_cleans_up(self):
        replacement = "new_name" * 65536
        self.patch_options(
            "*** Begin Patch\n*** Update File: sample file.txt\n"
            "@@\n-oldName\n+" + replacement + "\n*** End Patch\n")
        staged = []
        def intercept(stage, argv, kwargs):
            if stage == "plan":
                self.assertLess(sum(len(arg.encode()) for arg in argv), 4096)
                staged.append(Path(argv[argv.index("--patch") + 1][1:]))
        result = self.workflow(intercept)
        self.assertEqual(result["outcome"], "passed", result)
        self.assertEqual(self.file.read_text(), replacement + "\n")
        self.assertEqual(len(staged), 1)
        self.assertFalse(staged[0].exists())

    def test_patch_replay_keeps_operation_identity(self):
        self.patch_options()
        first = self.workflow()
        self.assertEqual(first["outcome"], "passed", first)
        second = self.workflow()
        self.assertEqual(second["outcome"], "passed", second)
        self.assertEqual(first["operation"], second["operation"])
        self.assertEqual(self.file.read_text(), "new_name\n")


    def test_patch_file_path_uses_calling_directory(self):
        payload = self.patch_options()
        caller = Path(self.temp.name)
        relative = caller / "caller patch.txt"
        relative.write_bytes(payload.read_bytes())
        command = [
            sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
            "--root", str(self.root), "--patch", relative.name, "--expected-files", "1",
            "--expected-edits", "1", "--key", "relative-patch",
            "--check", *self.options.check,
        ]
        result = REAL_RUN(command, cwd=caller, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "new_name\n")
        self.assertTrue(self.marker.exists())

    def test_patch_cli_stdin(self):
        payload = self.patch_options()
        command = [
            sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
            "--root", str(self.root), "--patch", "-", "--expected-files", "1",
            "--expected-edits", "1", "--key", "stdin-patch",
            "--check", *self.options.check,
        ]
        result = REAL_RUN(command, input=payload.read_bytes(), capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["outcome"], "passed")
        self.assertEqual(self.file.read_text(), "new_name\n")
        self.assertTrue(self.marker.exists())

    def test_patch_count_mismatch_never_applies(self):
        for files, edits in ((2, 1), (1, 2)):
            with self.subTest(files=files, edits=edits):
                self.calls.clear()
                self.patch_options(files=files, edits=edits)
                self.options.key = f"counts-{files}-{edits}"
                result = self.workflow()
                self.assert_stopped(result, "plan", ["plan"])
                self.assertIn("expected 2", result["stdout"])

    def test_patch_syntax_failure_never_applies(self):
        source = self.root / "app.py"
        source.write_text("value = 1\n")
        self.patch_options(
            "*** Begin Patch\n*** Update File: app.py\n"
            "@@\n-value = 1\n+def broken(\n*** End Patch\n")
        result = self.workflow()
        self.assert_stopped(result, "plan", ["plan"])
        self.assertEqual(source.read_text(), "value = 1\n")

    def test_patch_stale_apply_never_runs_check(self):
        self.patch_options()
        def intercept(stage, argv, kwargs):
            if stage == "apply":
                self.file.write_text("oldName\noutside\n")
        result = self.workflow(intercept)
        self.assertEqual(result["stage"], "apply", result)
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(self.calls, ["plan", "apply"])
        self.assertEqual(self.file.read_text(), "oldName\noutside\n")
        self.assertFalse(self.marker.exists())

    def test_patch_failed_check_preserves_preexisting_note(self):
        self.file.write_text("# user's note\noldName\n")
        self.patch_options()
        self.options.check = [sys.executable, "-c", "print('rejected'); raise SystemExit(17)"]
        result = self.workflow()
        self.assertEqual(result["outcome"], "failed", result)
        self.assertEqual(result["check"]["exit_code"], 17)
        self.assertEqual(result["undo"]["outcome"], "passed", result)
        self.assertEqual(self.calls, ["plan", "apply", "check", "undo"])
        self.assertEqual(self.file.read_text(), "# user's note\noldName\n")

    def test_patch_missing_or_non_utf8_input_never_applies(self):
        payload = self.patch_options()
        for content in (None, b"\xff"):
            with self.subTest(content=content):
                self.calls.clear()
                if content is None:
                    payload.unlink()
                else:
                    payload.write_bytes(content)
                result = self.workflow()
                self.assert_stopped(result, "plan", [])


    def test_match_parser_still_requires_scope_count_and_replacement(self):
        base = [sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
                "--root", str(self.root), "--literal", "oldName", "--key", "invalid-match"]
        cases = [
            ["--replace", "x", "--expected-matches", "1"],
            ["--path", self.file.name, "--replace", "x"],
            ["--path", self.file.name, "--expected-matches", "1"],
            ["--path", self.file.name, "--replace", "x", "--expected-matches", "0"],
            ["--path", self.file.name, "--replace", "x", "--expected-matches", "1",
             "--expected-files", "1", "--expected-edits", "1"],
        ]
        for args in cases:
            with self.subTest(args=args):
                result = REAL_RUN(base + args + ["--check", *self.options.check],
                                  capture_output=True, text=True)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertEqual(self.file.read_text(), "oldName\n")
                self.assertFalse(self.marker.exists())

    def test_patch_parser_rejects_mixed_modes_and_invalid_counts(self):
        payload = self.patch_options()
        base = [sys.executable, "-B", str(EXAMPLE), "--griz", BINARY,
                "--root", str(self.root), "--patch", str(payload), "--key", "invalid"]
        counts = ["--expected-files", "1", "--expected-edits", "1"]
        cases = [
            counts + ["--path", self.file.name], counts + ["--replace", "x"],
            counts + ["--transform", "-"], counts + ["--language", "python"],
            counts + ["--expected-matches", "1"], counts + ["--literal", "oldName"],
            ["--expected-files", "0", "--expected-edits", "1"],
            ["--expected-files", "1", "--expected-edits", "0"],
            ["--expected-files", "1"], ["--expected-edits", "1"],
        ]
        for args in cases:
            with self.subTest(args=args):
                result = REAL_RUN(base + args + ["--check", *self.options.check],
                                  capture_output=True, text=True)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertEqual(self.file.read_text(), "oldName\n")
                self.assertFalse(self.marker.exists())

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
