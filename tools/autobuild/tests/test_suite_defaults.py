import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("defaults_suite", ROOT / "suites/defaults.py")
defaults = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(defaults)


class DefaultsSuiteTests(unittest.TestCase):
    def test_candidates_are_long_options_and_top_level_manual_keys(self):
        manual = """# Other
``` yaml
skip: true
```
# Defaults files
``` yaml
sample-key: true
```
## General options
| --config | other-key  : |
|          |   nested-key: false |
# Templates
|          | outside: true |
"""
        options = [{"long": ["option", "alias"], "short": ["o"]}]
        self.assertEqual(defaults.candidate_keys(manual, options),
                         {"sample-key", "other-key", "option", "alias"})

    def test_a_known_key_needs_two_successful_values_and_a_type_error(self):
        candidates = {"wrap", "extra"}
        expected = {
            "extra/wrong#cmd": {"stderr": 'Unknown option "extra"'},
            "wrap/wrong#cmd": {"stderr": "Unexpected Object", "exit": 64},
        }
        for group in ("wrap", *(f"feature/{name}" for name in defaults.FEATURES)):
            for kind, code in (("valid-1", 0), ("valid-2", 0), ("wrong", 64)):
                expected.setdefault(f"{group}/{kind}#cmd", {"exit": code})
        self.assertEqual(defaults.accepted_keys(expected, candidates), {"wrap"})
        expected["wrap/valid-2#cmd"]["exit"] = 1
        with self.assertRaisesRegex(ValueError, "wrap/valid-2#cmd must have exit code 0"):
            defaults.accepted_keys(expected, candidates)

    def test_legacy_cases_include_nested_keys_and_lookup_features(self):
        args = ["--data-dir=local", "-dfirst", "--defaults=second.yaml", "--wrap=none"]
        files = {
            "first.yaml": 'defaults:\n  - ${.}/nested.cfg\n',
            "nested.cfg": '{"from":"commonmark","from":"markdown","metadata":{"toc":true}}',
            "local/defaults/second.yaml": 'to: html\ninclude-in-header: ${HOME}/text\n',
        }
        self.assertEqual(defaults.legacy_groups({"files": files}, args), {
            "defaults", "from", "metadata", "to", "include-in-header", "feature/dot",
            "feature/datadir-with-yaml", "feature/home", "feature/nested-defaults",
            "feature/option-order", "feature/ordered-defaults",
        })

    def test_legacy_cases_use_the_stdin_defaults_file(self):
        case = {"input": "to: plain\n"}
        self.assertEqual(defaults.legacy_groups(case, ["-d", "/dev/stdin"]), {"to"})

    def test_one_failing_case_fails_every_key_that_uses_it(self):
        statuses = {"probe": "pass", "legacy": "fail", "slow": "no_result", "unused": "skipped"}
        members = {"probe": {"wrap"}, "legacy": {"wrap", "to"}, "slow": {"to"}, "unused": {"wrap"}}
        results = defaults.group_results({"wrap", "to"}, statuses, members)
        self.assertEqual(results["wrap"], {
            "units": {"probe": "pass", "legacy": "fail"},
            "failures": ["legacy"],
            "no_result": False,
        })
        self.assertEqual(results["to"]["failures"], ["legacy", "slow"])
        self.assertTrue(results["to"]["no_result"])
        with self.assertRaisesRegex(ValueError, "do not match"):
            defaults.group_results({"wrap"}, {}, members)


if __name__ == "__main__":
    unittest.main()
