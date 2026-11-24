#!/usr/bin/env python3
"""Write the Markdown tables for the Randoc 0.1.0 benchmark results."""

import argparse
import json
import math
from pathlib import Path

import run


def memory(value):
    """Format peak resident memory in MiB."""
    return "—" if value is None else f"{value / 2**20:.1f}"


def benchmark_row(bench, results, comparison, looks):
    """Format one row, with a ratio only when the output matches."""
    name = bench["name"]
    candidate = results["program"][name]
    reference = results["reference"][name]
    equal = comparison.get("same_output", False)
    size = bench.get("input_bytes")
    if size is None:
        size = len(bench["stdin"]["text"].encode("utf-8")) if "text" in bench["stdin"] else "—"

    def times(result):
        return [sample["cpu"] for sample in result["samples"]]

    def cpu(result):
        median = result.get("summary", {}).get("cpu", {}).get("median")
        return "—" if median is None else f"{median * 1000:.2f}"

    def peak(result):
        rss = result.get("summary", {}).get("maxrss", {})
        return f"{memory(rss.get('median'))} / {memory(rss.get('max'))}"

    ratio = "—"
    if equal:
        low, estimate, high = run.paired_bounds(times(reference), times(candidate), 0.025 / looks)
        upper = f"{high:.3f}" if math.isfinite(high) else "∞"
        ratio = f"{estimate:.3f} ({low:.3f}–{upper})"
    return f"| {name} | {size} | {cpu(candidate)} | {cpu(reference)} | {ratio} | {peak(candidate)} | {peak(reference)} | {'Yes' if equal else 'No'} |"


def benchmark_tables(data):
    """Format the conversion benchmarks and the separate nesting benchmark."""
    results = data["results"]
    if "reference" not in results:
        raise ValueError("A benchmark result must include both programs")
    comparisons = {row["name"]: row for row in data["comparison"]}
    settings = data["settings"]
    looks = len(run.look_sizes(settings["runs"], settings.get("max_runs", settings["runs"])))
    header = [
        "| Benchmark | Input bytes | Randoc median CPU (ms) | Pandoc median CPU (ms) | Randoc / Pandoc (95% CI) | Randoc peak MiB (median / max) | Pandoc peak MiB (median / max) | Equal output |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
    ]
    normal, nested = [], []
    for bench in data["benchmarks"]:
        rows = nested if bench["name"] == "adversarial-nested-quotes" else normal
        rows.append(benchmark_row(bench, results, comparisons[bench["name"]], looks))
    lines = header + normal if normal else []
    if nested:
        lines += ["", "#### Memory with 10,000 nested block quotes", "", *header, *nested]
    return lines


def corpus_table(data):
    """Format the throughput and output equality by job count."""
    lines = [
        "| Mode | Jobs | Program | CPU (s) | Elapsed wall (s) | Documents/s | Max peak MiB | Equal outputs |",
        "| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for name, mode in data["modes"].items():
        for role, label in (("program", "Randoc"), ("reference", "Pandoc")):
            s = mode[role]["summary"]
            equal = f"{mode['equal_outputs']} / {data['conversions_per_run']}"
            lines.append(
                f"| {name} | {mode['jobs']} | {label} | {s['total_cpu_s']:.2f} | "
                f"{s['elapsed_wall_s']:.2f} | {s['documents_per_second']:.2f} | "
                f"{memory(s['max_peak_bytes'])} | {equal} |"
            )
    return lines


def report(paths):
    """Return a report for one or more run.py and corpus_throughput.py files."""
    lines = ["## Release 0.1.0", ""]
    for path in paths:
        data = json.loads(path.read_text(encoding="utf-8"))
        env = data["environment"]
        machine = env.get("cpu_model") or env["machine"]
        commit = data["programs"]["program"].get("worktree_commit") or "unknown"
        date = data["started"][:10]
        if data["programs"]["program"].get("worktree_dirty"):
            commit += " (worktree had changes)"
        lines += [f"### {path.stem}", "", f"{machine}, {env['os']}. Measured on {date}. Randoc commit {commit}.", ""]
        if data.get("kind") == "corpus-throughput":
            lines += corpus_table(data)
        else:
            lines += benchmark_tables(data)
        lines.append("")
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path, nargs="+")
    args = parser.parse_args(argv)
    try:
        print(report(args.results))
    except (OSError, KeyError, TypeError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
