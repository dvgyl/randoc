#!/usr/bin/env python3
"""Measure sequential and parallel conversion of the Randoc document corpus."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib

import run

sys.path.insert(0, str(run.ROOT / "tools" / "corpus"))
import corpuslib

TARGETS = ("json", "html", "markdown")


def warm_up(binary, task, scratch):
    """Run one conversion and record its output digest."""
    name, source, target, path, timeout = task
    command = [binary, "-f", source, "-t", target]
    with tempfile.TemporaryDirectory(dir=scratch) as folder:
        return run.probe(command, path, Path(folder), timeout)


def measure(binary, task, warm, repeats):
    """Record time and memory for one document and output format."""
    name, source, target, path, timeout = task
    command = [binary, "-f", source, "-t", target]
    result = {"document": name, "source": source, "target": target, "warmup": warm, "samples": []}
    result["status"] = warm["status"]
    if result["status"] != "ok":
        return result
    for _ in range(repeats):
        try:
            code, sample = run.run_once(command, path, timeout=timeout)
        except subprocess.TimeoutExpired:
            result.update(status="error", error=f"Time limit of {timeout} seconds")
            break
        if code != 0:
            result.update(status="error", exit_code=code)
            break
        result["samples"].append(asdict(sample))
    if result["samples"]:
        result["summary"] = run.summarize(result["samples"])
        for field in ("cpu", "wall"):
            times = sorted(sample[field] for sample in result["samples"])
            result["summary"][field]["p95"] = times[math.ceil(len(times) * 0.95) - 1]
    return result


def run_mode(binary, tasks, jobs, repeats, scratch):
    """Measure a program with at most jobs conversions at the same time."""
    with ThreadPoolExecutor(max_workers=jobs) as workers:
        warm = list(workers.map(lambda task: warm_up(binary, task, scratch), tasks))
        start = time.perf_counter()
        results = list(workers.map(lambda task, probe: measure(binary, task, probe, repeats), tasks, warm))
        elapsed = time.perf_counter() - start
    samples = [sample for result in results for sample in result["samples"]]
    peaks = [sample["maxrss"] for sample in samples]
    conversions = len(samples)
    return {
        "results": results,
        "summary": {
            "successful_conversions": conversions,
            "total_cpu_s": sum(sample["cpu"] for sample in samples),
            "total_child_wall_s": sum(sample["wall"] for sample in samples),
            "elapsed_wall_s": elapsed,
            "conversions_per_second": conversions / elapsed,
            "documents_per_second": conversions / (len(TARGETS) * elapsed),
            "median_peak_bytes": statistics.median(peaks) if peaks else None,
            "max_peak_bytes": max(peaks, default=None),
        },
    }


def compare_outputs(program, reference):
    """Compare successful conversions using the output from each warm-up."""
    return [
        a["status"] == b["status"] == "ok"
        and a["warmup"]["stdout_sha256"] == b["warmup"]["stdout_sha256"]
        for a, b in zip(program["results"], reference["results"], strict=True)
    ]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--program", required=True, help="Randoc executable")
    parser.add_argument("--oracle", required=True, help="Pandoc 3.13 executable")
    parser.add_argument("--manifest", type=Path, default=corpuslib.MANIFEST)
    parser.add_argument("--corpus", type=Path, default=os.environ.get("RANDOC_CORPUS"))
    parser.add_argument("--jobs", type=int, help="parallel jobs, by default the number of performance cores")
    parser.add_argument("--runs", type=int, default=5, help="timed runs for each conversion")
    parser.add_argument("--timeout", type=float, default=60.0, help="time limit of one conversion in seconds")
    parser.add_argument("--out", type=Path, required=True, help="JSON file for the results")
    args = parser.parse_args(argv)
    if args.corpus is None:
        parser.error("Set RANDOC_CORPUS or supply --corpus")
    if args.runs < 1 or args.timeout <= 0 or (args.jobs is not None and args.jobs < 1):
        parser.error("--jobs, --runs, and --timeout must be positive")
    programs = {"program": run.resolve_program(args.program), "reference": run.resolve_program(args.oracle)}
    if None in programs.values():
        parser.error("--program and --oracle must name executable files")
    try:
        documents = corpuslib.load_manifest(args.manifest)
        if not documents:
            parser.error("The corpus manifest has no documents")
        for document in documents:
            path = args.corpus / document.file_name
            if path.stat().st_size != document.size or run.file_sha256(path) != document.sha256:
                parser.error(f"Corpus input differs from the manifest: {document.file_name}")
    except (OSError, tomllib.TOMLDecodeError, corpuslib.ManifestError) as error:
        parser.error(str(error))
    tasks = [
        (doc.name, doc.format, target, args.corpus / doc.file_name, doc.timeout or args.timeout)
        for doc in documents for target in TARGETS
    ]
    env = run.environment()
    available = len(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else env["cpu_count"]
    jobs = args.jobs or env.get("perflevel0_cores") or available or 1
    record = {
        "schema": 1,
        "kind": "corpus-throughput",
        "started": run._now(),
        "environment": env,
        "programs": {role: run.program_info(path) for role, path in programs.items()},
        "manifest_sha256": run.file_sha256(args.manifest),
        "documents": len(documents),
        "conversions_per_run": len(tasks),
        "runs": args.runs,
        "modes": {},
    }
    failed = False
    with tempfile.TemporaryDirectory(prefix="randoc-corpus-bench-") as scratch:
        for mode, workers in (("sequential", 1), ("parallel", jobs)):
            print(f"{mode.capitalize()} measurement with {workers} job(s)", file=sys.stderr, flush=True)
            results = {role: run_mode(path, tasks, workers, args.runs, scratch) for role, path in programs.items()}
            matches = compare_outputs(results["program"], results["reference"])
            record["modes"][mode] = dict(results, jobs=workers, equal_outputs=sum(matches), output_equal=matches)
            for role, result in results.items():
                summary = result["summary"]
                failed |= summary["successful_conversions"] != len(tasks) * args.runs
                print(f"{mode} {role}: {summary['documents_per_second']:.2f} documents/s")
            print(f"{mode}: {sum(matches)} of {len(matches)} equal outputs")
    record["finished"] = run._now()
    args.out.write_text(json.dumps(record, indent=1) + "\n", encoding="utf-8")
    return 2 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
