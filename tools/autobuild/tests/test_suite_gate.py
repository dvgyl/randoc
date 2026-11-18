import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gate
import measure


class GateSuite(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.evidence = Path(folder.name)
        registry = measure.load_metrics()
        self.assertIn("gate", measure.active_suites(registry))
        self.assertNotIn("gate", gate.gate_suites(registry))
        self.runner = measure.RUNNERS["gate"]
        self.steps = [{"name": step.name, "status": "pass"} for step in gate.STEPS]

    def parse(self, result="pass", quick=False):
        report = {"steps": self.steps, "result": result, "quick": quick}
        (self.evidence / "gate-report.json").write_text(json.dumps(report), encoding="utf-8")
        return self.runner.parse(self.evidence, "")

    def test_counts_each_step_of_a_passing_gate(self):
        self.assertEqual(self.parse().rows, {"gate.steps": (len(self.steps), len(self.steps))})

    def test_failed_steps_and_steps_that_did_not_run_count_against_the_total(self):
        self.assertIn(1, self.runner.exits)
        self.steps[-2]["status"] = "fail"
        self.steps[-1]["status"] = "not-run"
        counts = self.parse(result="fail")
        self.assertEqual(counts.rows, {"gate.steps": (len(self.steps) - 2, len(self.steps))})
        self.assertEqual(counts.failing, [step["name"] for step in self.steps[-2:]])

    def test_a_changed_program_copy_fails_the_gate_even_if_all_steps_passed(self):
        self.steps.append({"name": "programs", "status": "fail"})
        self.assertEqual(self.parse(result="fail").failing, ["programs"])

    def test_a_missing_step_or_a_quick_gate_cannot_give_a_row(self):
        with self.assertRaises(measure.ToolError):
            self.parse(quick=True)
        self.steps.pop()
        with self.assertRaises(measure.ToolError):
            self.parse()
