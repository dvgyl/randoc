import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import measure  # noqa: E402

SUITE = '''
def make_runner(m):
    def parse(evidence, log):
        units = [m.Unit("a", m.PASS, ("probe.units",)), m.Unit("b", m.FAIL, ("probe.units",))]
        return m.Counts.of(units, ("probe.units",))
    return m.Runner(parse, "--filter", "tools/probe")
'''

ENTRY = '''
[[metric]]
key = "{key}"
suite = "probe"
status = "active"
command = "{{python}} tools/probe/run.py"
pass = "units that pass"
total = "units"
'''


class PluginTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.saved = (measure.SUITES_DIR, measure.METRICS_DIR, dict(measure.RUNNERS))
        measure.SUITES_DIR, measure.METRICS_DIR = root / "suites", root / "metrics.d"
        measure.SUITES_DIR.mkdir()
        measure.METRICS_DIR.mkdir()
        (measure.SUITES_DIR / "probe.py").write_text(SUITE)

    def tearDown(self):
        measure.SUITES_DIR, measure.METRICS_DIR, runners = self.saved
        measure.RUNNERS.clear()
        measure.RUNNERS.update(runners)
        self.tmp.cleanup()

    def test_a_suite_file_and_a_metrics_file_add_an_active_metric(self):
        (measure.METRICS_DIR / "probe.toml").write_text(ENTRY.format(key="probe.units"))
        metrics = measure.load_metrics()
        self.assertEqual(metrics["probe.units"].status, "active")
        counts = measure.RUNNERS["probe"].parse(Path("."), "")
        self.assertEqual(counts.rows["probe.units"], (1, 2))

    def test_a_plugin_entry_replaces_a_to_build_entry(self):
        (measure.METRICS_DIR / "probe.toml").write_text(ENTRY.format(key="lua.api"))
        self.assertEqual(measure.load_metrics()["lua.api"].status, "active")

    def test_a_plugin_entry_cannot_replace_an_active_entry(self):
        (measure.METRICS_DIR / "probe.toml").write_text(ENTRY.format(key="harness.units"))
        with self.assertRaises(measure.ToolError):
            measure.load_metrics()


if __name__ == "__main__":
    unittest.main()
