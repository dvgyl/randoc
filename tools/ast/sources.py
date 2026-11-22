"""Collect document trees from the visible JSON comparisons."""

import hashlib
import importlib.util
import inspect
import json
import os
import sys
import tempfile
import tomllib
from contextlib import closing
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "tools" / name) for name in ("readers", "pivot", "corpus", "casegen")]

import casegen
import readers
from capture import WALL_SECONDS
from generate import API_VERSION, json_bytes
from inputs import corpuslib, document_inputs, harness_inputs, pivot_inputs, reader_formats
from normalize import notebook_rule
from observe import NoResult
from reference import cache_key, open_cache, read_reference
from run import CACHE_NAME, OracleCache, arguments, verify_documents
from writers import attempt


def load_tool(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def version_cases():
    major, minor, patch, revision = API_VERSION
    versions = (
        ("missing", None),
        ("older-major", [major - 1, minor, patch, revision]),
        ("older-minor", [major, minor - 1, patch, revision]),
        ("newer-major", [major + 1, minor, patch, revision]),
        ("newer-minor", [major, minor + 1, patch, revision]),
        ("string", ".".join(map(str, API_VERSION))),
        ("null", None),
        ("non-integer-component", [major, minor, patch, str(revision)]),
        ("short", API_VERSION[:2]),
        ("long", [*API_VERSION, 0]),
    )
    for name, version in versions:
        tree = {"meta": {}, "blocks": [{"t": "HorizontalRule"}]}
        if name != "missing":
            tree["pandoc-api-version"] = version
        yield f"version/{name}", json_bytes(tree)


def harness_trees():
    cases_root = ROOT / "conformance/cases"
    for path in sorted(cases_root.rglob("*.toml")):
        if path.name.startswith("._"):
            continue
        case_file = tomllib.loads(path.read_text(encoding="utf-8"))
        relative = path.relative_to(cases_root)
        group = relative.with_suffix("")
        case_ids = {f"{case['id']}#read" for case in case_file.get("case", [])
                    if "cmd" not in case and case.get("from", case_file.get("from"))}
        if not case_ids:
            continue
        saved = json.loads((ROOT / "conformance/expected" / relative.with_suffix(".json")).read_bytes())
        if saved["oracle"] != "pandoc 3.13":
            raise ValueError(f"{group}: the saved result has a different oracle")
        for case_id in sorted(case_ids):
            result = saved["units"].get(case_id)
            if result is None:
                yield f"harness/{group}/{case_id}", "no recorded oracle result"
            elif result["exit"] == 0:
                output = result["stdout"]
                yield f"harness/{group}/{case_id}", (output.encode() if isinstance(output, str)
                                                       else bytes.fromhex(output["hex"]))


def source_json(capture, oracle, command, data=b"", **options):
    try:
        result = capture.run(oracle, command, data, oracle=True, **options)
    except NoResult as error:
        return str(error)
    return result.stdout if result.exit == 0 else None


def corpus_trees(args, oracle_sha, capture):
    documents = corpuslib.load_manifest()
    changed = verify_documents(documents, args.corpus)
    if changed:
        raise ValueError(f"missing or changed corpus documents: {', '.join(changed)}")
    with closing(OracleCache(args.corpus / CACHE_NAME, oracle_sha)) as cache:
        for doc in documents:
            command = arguments(doc, "json")
            result = cache.get(cache.key(doc.sha256, command))
            if result is None:
                output = source_json(capture, args.oracle, command, cwd=args.corpus,
                                     wall_seconds=max(WALL_SECONDS, doc.timeout))
            elif result.exit == 0:
                output = result.stdout
            else:
                output = None
            if output is not None:
                yield f"corpus/{doc.name}", output


def reader_trees(args, writers, trees, writer_version, pool):
    digest = hashlib.sha256(b"randoc reader results" + writer_version)
    digest.update((ROOT / "tools/readers/results.py").read_bytes())
    digest.update(inspect.getsource(readers.json_equal).encode("utf-8"))
    version = digest.digest()
    cache = (Path(os.environ["RANDOC_CACHE"]) if os.environ.get("RANDOC_CACHE") else
             Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "cache") / "fmt"
    formats = reader_formats(ROOT)
    own = harness_inputs(ROOT, formats)
    corpus = corpuslib.load_manifest()
    workbooks = corpuslib.load_manifest(ROOT / "tools/readers/workbooks.toml")
    rule = notebook_rule()
    with closing(open_cache(cache / "writers.sqlite")) as writer_cache, closing(
            open_cache(cache / "readers.sqlite")) as reader_cache:
        for fmt in formats:
            command = ["-f", fmt, "-t", "json"]
            units = list(own[fmt])
            if fmt in writers:
                units.extend(pivot_inputs(fmt, trees, writer_cache, writer_version,
                                          args.oracle, pool, rule, None))
            units.extend(document_inputs((d for d in corpus if d.format == fmt), args.corpus))
            units.extend(document_inputs((d for d in workbooks if d.format == fmt),
                                         cache / "workbooks", fetch=True))
            keys = [cache_key(version, command, unit.data) for unit in units]

            def capture(unit):
                return attempt(args.oracle, command, unit, oracle=True)

            readers.recorded_references(reader_cache, dict(zip(keys, units)), capture, pool, False)
            for key, unit in zip(keys, units):
                ref = read_reference(reader_cache, key)
                if ref.first is not None and ref.first.exit == 0:
                    yield unit.id, ref.first.stdout
                if ref.status != "stable":
                    yield unit.id, f"{ref.status}: {ref.reason}"


def suite_trees(args, pool, capture):
    tool = load_tool("ast_pandoc_suite", "tools/pandoc-testsuite/run.py")
    location = str(args.pandoc_tests)
    suite = tool.testsuite
    excluded = tool.load_exclusions(tool.EXCLUSIONS)
    cases = [case for case in suite.discover(location).cases
             if isinstance(case, suite.AstCase) and case.id not in excluded]
    with tempfile.TemporaryDirectory(prefix="randoc-ast-suite-") as bin_dir:
        runner = tool.Runner(str(args.oracle), location, WALL_SECONDS, bin_dir, ghcrts=tool.ORACLE_GHCRTS)

        def extension_list(command, data):
            result = capture.run(runner.pandoc, command, data, oracle=True, cwd=runner.suite, env=runner.env)
            return suite.Outcome(result.exit, result.stdout, result.stderr)

        def read(case):
            commands = {"expected": ("-f", case.expected_format, "-t", "json", case.expected_path)}
            fmt = case.reader
            if case.extensions is not None:
                try:
                    fmt = suite.format_with_exact_extensions(extension_list, fmt, case.extensions)
                except (NoResult, suite.HelperError) as error:
                    fmt = None
                    yield f"pandoc/{case.id}/input", str(error)
            if fmt is not None:
                commands["input"] = ("-f", fmt, "-t", "json", *case.extra_args, case.input_path)
            for side, command in commands.items():
                identifier = f"pandoc/{case.id}/{side}"
                output = source_json(capture, runner.pandoc, command, cwd=runner.suite, env=runner.env)
                if output is not None:
                    yield identifier, output

        yield from (entry for entries in pool.map(lambda case: list(read(case)), cases)
                    for entry in entries)


def commonmark_trees(args, pool, capture):
    tool = load_tool("ast_commonmark", "tools/commonmark-spec/run.py")
    path = Path(os.environ["RANDOC_ENV"]) / "commonmark-spec" / tool.SPEC_FILE
    data = path.read_bytes()
    tool.check_hash(data)
    examples = tool.extract_examples(data.decode("utf-8"))
    env = tool.environment(tool.ORACLE_GHCRTS)
    def read(example):
        number, source = example
        output = source_json(capture, args.oracle, tool.ARGS, source.encode("utf-8"), env=env)
        return f"commonmark/{number}", output

    for identifier, output in pool.map(read, examples):
        if output is not None:
            yield identifier, output


def generated_trees(args, pool, capture):
    extensions = json.loads(casegen.DEFAULT_EXTENSIONS.read_bytes())

    def oracle_json(reader, content):
        output = source_json(capture, args.oracle, ["-f", reader, "-t", "json"], content)
        return output if isinstance(output, bytes) else None

    def read(seed):
        files = casegen.generate(seed, 2, extensions, oracle_json=oracle_json)
        for name in sorted(files):
            if not name.endswith(".case.json"):
                continue
            case = json.loads(files[name])
            command = case["args"]
            formats = [value for option, value in zip(command, command[1:]) if option == "-t"]
            if not formats or formats[-1].split("+")[0].split("-")[0] != "json":
                continue
            identifier = f"casegen/{seed}/{case['id']}"
            with tempfile.TemporaryDirectory(prefix="randoc-ast-generated-") as work:
                (Path(work) / case["input"]).write_bytes(files[case["input"]])
                output = source_json(capture, args.oracle, command, cwd=work)
            if output is not None:
                yield identifier, output

    yield from (entry for entries in pool.map(lambda seed: list(read(seed)), range(1, 1001))
                for entry in entries)
