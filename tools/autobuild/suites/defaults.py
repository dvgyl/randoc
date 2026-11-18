"""Measure defaults keys and features with the conformance harness."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import re
import subprocess
import sys
import time
import tomllib


METRICS = ("defaults.units",)
REPORT = "defaults-keys.json"
GENERATED = "defaults/probes"
FEATURES = (
    "nested-defaults", "dot", "userdata", "home", "datadir-with-yaml",
    "datadir-without-yaml", "ordered-defaults", "option-order",
)
VARIABLES = {"${.}": "dot", "${USERDATA}": "userdata", "${HOME}": "home"}
DOC = "# Quartz\n\nA line with *emphasis*, $x+y$, and a [link](#quartz).\n"
HIGHLIGHT_DOC = "```python\nprint(7)\n```\n"
HIGHLIGHT_KEYS = {"highlight-style", "syntax-definition", "syntax-definitions", "syntax-highlighting"}
MEDIA_DOC = "![Part layout](layout.svg)\n"
MEDIA_FILE = '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="4"/></svg>\n'
HEADING_DOC = "# Check\n\n## Cover\n\n### Seal\n\n#### Edge\n\nFit the cover.\n"
LINK_DOC = "# Parts\n\n[Part data](https://example.org/parts).\n\n# Test\n\n[Results](https://example.org/results).\n"
TABLE_DOC = "| Part | Count |\n|------|------:|\n| Seal | 2     |\n\n: Parts list\n"
CODE_DOC = "```\npart\tcount\nseal\t2\n```\n"
DOCUMENTS = {
    **dict.fromkeys(HIGHLIGHT_KEYS, HIGHLIGHT_DOC),
    **dict.fromkeys({"table-of-contents", "toc", "toc-depth", "number-offset"}, HEADING_DOC),
    **dict.fromkeys({"default-image-extension", "dpi"}, "![Part layout](layout){width=96px}\n"),
    **dict.fromkeys({"embed-resources", "extract-media", "figure-caption-position",
                     "lof", "list-of-figures"}, MEDIA_DOC),
    **dict.fromkeys({"list-of-tables", "list-tables", "lot", "table-caption-position"}, TABLE_DOC),
    **dict.fromkeys({"reference-links", "reference-location", "link-images"}, LINK_DOC),
    **dict.fromkeys({"preserve-tabs", "tab-stop", "indented-code-classes"}, CODE_DOC),
    "ascii": "The label reads Ångström.\n",
    "email-obfuscation": "Contact <mira@example.org>.\n",
    "html-q-tags": 'The label reads "service".\n',
    "strip-comments": "Open the cover.\n\n<!-- Internal note -->\n\nCheck the seal.\n",
}
STANDALONE = {
    "css", "defaults", "filters", "list-of-figures", "list-of-tables", "list-tables", "lof", "lot",
    "metadata", "metadata-file", "metadata-files", "number-offset", "table-of-contents",
    "toc", "toc-depth", "variables",
}
TARGETS = {
    **dict.fromkeys({"dpi", "lof", "lot", "list-of-figures", "list-of-tables", "list-tables"}, "latex"),
    **dict.fromkeys({"reference-links", "reference-location", "markdown-headings"}, "markdown"),
    "columns": "plain",
}
PAIR = ("alpha", "beta")


def make_runner(measure):
    def parse(evidence, _log):
        groups = measure.read_json(evidence / REPORT)
        units = [measure.Unit(key, measure.PASS if row["sensitive"] and not row["failures"] else measure.FAIL,
                              METRICS, row["no_result"]) for key, row in groups.items()]
        return measure.Counts.of(units, METRICS)

    return measure.Runner(parse, "--filter", "tools/autobuild")


def candidate_keys(manual, options):
    """Read the names in the defaults tables and the introductory YAML blocks."""
    heading = re.search(r"^# Defaults files\s*$", manual, re.M)
    if heading is None:
        raise ValueError("The manual has no Defaults files section.")
    section = re.split(r"^# ", manual[heading.end():], maxsplit=1, flags=re.M)[0]
    names = set(re.findall(r"^\|[^|]*\| ([a-z][a-z0-9-]*)\s*:", section, re.M))
    for block in re.findall(r"^``` yaml\n(.*?)^```", section, re.M | re.S):
        names.update(re.findall(r"^([a-z][a-z0-9-]*)\s*:", block, re.M))
    if not names:
        raise ValueError("The Defaults files section has no keys.")
    return names | {name for option in options for name in option["long"]}


def key_values():
    """Give each accepted key two distinct values."""
    values = {}
    for keys, pair in (
        ("ascii citeproc dump-args embed-resources epub-title-page fail-if-warnings file-scope "
         "html-q-tags ignore-args incremental link-images list-of-figures list-of-tables "
         "list-tables listings lof lot no-check-certificate number-sections preserve-tabs "
         "reference-links sandbox section-divs self-contained standalone strip-comments "
         "table-of-contents toc trace", (True, False)),
        ("columns dpi epub-chapter-level shift-heading-level-by slide-level split-level "
         "tab-stop toc-depth", (2, 3)),
        ("from reader", ("markdown", "commonmark")),
        ("to writer", ("html", "plain")),
        ("data-dir", (".", "inputs")),
        ("cite-method", ("natbib", "biblatex")),
        ("css", ("alpha.css", ["beta.css"])),
        ("default-image-extension", ("svg", "png")),
        ("defaults", ("alpha.cfg", ["beta.cfg"])),
        ("email-obfuscation", ("javascript", "references")),
        ("eol", ("lf", "crlf")),
        ("epub-fonts", (["alpha.font"], ["beta.font"])),
        ("figure-caption-position table-caption-position", ("above", "below")),
        ("filters", (["alpha.lua"], ["beta.lua"])),
        ("highlight-style syntax-highlighting", ("tango", "kate")),
        ("indented-code-classes", (["python"], ["ruby", "perl"])),
        ("input-files", (["alpha.md"], ["beta.md"])),
        ("input-file", ("alpha.md", "beta.md")),
        ("ipynb-output", ("IpynbOutputAll", "IpynbOutputNone")),
        ("markdown-headings", ("setext", "atx")),
        ("math-method", ("mathjax", {"method": "mathml"})),
        ("metadata variables", ({"title": "Quartz"}, {"title": "Flint"})),
        ("metadata-files", (["alpha.meta"], ["beta.meta"])),
        ("number-offset", ([1], [2, 3])),
        ("output-file", ("alpha.html", "beta.html")),
        ("pdf-engine", ("xelatex", "lualatex")),
        ("pdf-engine-opt", ("-silent", "-draftmode")),
        ("pdf-engine-opts", (["-silent"], ["-draftmode"])),
        ("reference-location", ("section", "document")),
        ("request-headers", ([["X-Sample", "alpha"]], [["X-Sample", "beta"]])),
        ("resource-path", (["."], ["inputs", "."])),
        ("syntax-definitions", (["alpha.xml"], ["beta.xml"])),
        ("top-level-division", ("chapter", "part")),
        ("track-changes", ("accept", "reject")),
        ("verbosity", ("INFO", "ERROR")),
        ("wrap", ("auto", "preserve")),
        ("chunk-template", ("alpha-%s.html", "beta-%s.html")),
    ):
        values.update(dict.fromkeys(keys.split(), pair))
    for key, kind in FILE_OPTIONS.items():
        values[key] = tuple(f"{name}.{kind}" for name in PAIR)
    values.update(dict.fromkeys(
        ("epub-subdirectory", "extract-media", "identifier-prefix", "log-file", "title-prefix"),
        PAIR,
    ))
    return values


FILE_OPTIONS = {
    "abbreviations": "txt",
    "bibliography": "bib",
    "citation-abbreviations": "json",
    "csl": "csl",
    "epub-cover-image": "svg",
    "epub-metadata": "epubmeta",
    "include-after-body": "html",
    "include-before-body": "html",
    "include-in-header": "html",
    "metadata-file": "meta",
    "reference-doc": "md",
    "syntax-definition": "xml",
    "template": "html",
}
FILE_TEXT = {
    "bib": "@book{sample, title={Quartz}, author={L. Hale}, year={2011}}\n",
    "cfg": "metadata: {title: %s}\n",
    "csl": '<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text">'
           '<info><title>Sample</title><id>https://example.invalid/quartz</id></info>'
           '<citation><layout><text variable="title"/></layout></citation></style>',
    "epubmeta": '<dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Quartz</dc:title>',
    "font": "",
    "html": "<p>%s</p>\n$body$\n",
    "json": "{}",
    "lua": 'function Pandoc(doc) doc.meta.title = "%s"; return doc end\n',
    "md": "%s\n",
    "meta": "title: %s\n",
    "svg": '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>',
    "txt": "Dr.\n",
    "xml": '<language name="%s" version="1" kateversion="2.4" section="Other" '
           'extensions="*.sample"><highlighting><contexts>'
           '<context attribute="Normal Text" lineEndContext="#stay" name="Normal"/>'
           '</contexts><itemDatas><itemData name="Normal Text" defStyleNum="dsNormal"/>'
           '</itemDatas></highlighting></language>',
}


def probe(key, value, positive=False):
    files = {"probe.cfg": json.dumps({key: value})}
    if positive and key in {"data-dir", "resource-path"}:
        files["inputs/marker.txt"] = "Quartz\n"
    if positive and key in {"embed-resources", "extract-media"}:
        files["layout.svg"] = MEDIA_FILE
    if positive and key in (FILE_OPTIONS.keys() | {
        "defaults", "epub-fonts", "filters", "input-files", "input-file",
        "metadata-files", "syntax-definitions",
    }):
        for path in value if isinstance(value, list) else [value]:
            text = FILE_TEXT[PurePosixPath(path).suffix[1:]]
            files[path] = text.replace("%s", PurePosixPath(path).stem)
    cmd = ["-d", "probe.cfg"]
    if key not in {"from", "reader"}:
        cmd += ["-f", "markdown"]
    if key not in {"to", "writer"}:
        cmd += ["-t", TARGETS.get(key, "html")]
    if key in STANDALONE:
        cmd.append("-s")
    if key == "number-offset":
        cmd.append("--number-sections")
    if key == "reference-location":
        cmd.append("--reference-links")
    if key == "toc-depth":
        cmd.append("--toc")
    return cmd, files


def feature_probe(feature, index):
    wrong_type = index == 3
    name = ("alpha", "beta", "flint")[index - 1]
    invalid = {"wrap": 7}
    match feature:
        case "nested-defaults":
            value = ["inside.cfg"] if index == 2 else "inside.cfg"
            return ["-d", "outer.cfg"], {
                "outer.cfg": json.dumps({"defaults": value}),
                "inside.cfg": json.dumps(invalid if wrong_type else {
                    "metadata": {"title": name},
                }),
            }
        case "dot" | "userdata" | "home":
            base, variable = {
                "dot": ("sub", "${.}"), "userdata": ("data", "${USERDATA}"),
                "home": (".home", "${HOME}"),
            }[feature]
            path = f"{name}.meta"
            values = [f"{variable}/{path}"]
            if wrong_type:
                values.append(7)
            return ["--data-dir=data", "-d", "sub/sample.cfg"], {
                "sub/sample.cfg": json.dumps({"metadata-files": values}),
                f"{base}/{path}": f"title: {name}\n",
            }
        case "datadir-with-yaml" | "datadir-without-yaml":
            arg = f"{name}.yaml" if feature == "datadir-with-yaml" else name
            return ["--data-dir=data", "-d", arg], {
                f"data/defaults/{name}.yaml": json.dumps(invalid if wrong_type else {
                    "metadata": {"title": name},
                }),
            }
        case "ordered-defaults":
            cmd = ["-d", "first.cfg", "-d", "second.cfg"]
            if index == 2:
                cmd = ["-d", "second.cfg", "-d", "first.cfg"]
            return cmd, {"first.cfg": json.dumps({"metadata": {"title": "Quartz"}}),
                         "second.cfg": json.dumps(invalid if wrong_type else {
                             "metadata": {"title": "Flint"},
                         })}
        case "option-order":
            before, after = (("Quartz", "Flint") if index == 1 else ("Flint", "Quartz"))
            return ["-M", f"title={before}", "-d", "sample.cfg", "-M", f"title={after}"], {
                "sample.cfg": json.dumps(invalid if wrong_type else {"metadata": {"title": name}}),
            }
    raise ValueError(f"Unknown defaults feature {feature}.")


def generated_cases(candidates):
    values = key_values()
    cases = {}
    for key in sorted(candidates):
        invalid = 7 if key in {"metadata", "variables", "math-method"} else {"quartz": 7}
        cases[f"{key}/wrong"] = probe(key, invalid)
        for index, value in enumerate(values.get(key, ()), 1):
            cases[f"{key}/valid-{index}"] = probe(key, value, True)
    for feature in FEATURES:
        for index, kind in enumerate(("valid-1", "valid-2", "wrong"), 1):
            cmd, files = feature_probe(feature, index)
            cases[f"feature/{feature}/{kind}"] = (cmd + ["-f", "markdown", "-t", "html", "-s"], files)
    return cases


def write_cases(root, cases):
    path = root / f"conformance/cases/{GENERATED}.toml"
    path.parent.mkdir(parents=True)
    with path.open("w", encoding="utf-8") as stream:
        stream.write("capture_files = {}\n")
        for ident, (cmd, files) in cases.items():
            fields = ", ".join(f"{json.dumps(name)} = {json.dumps(text)}"
                               for name, text in files.items())
            document = DOCUMENTS.get(ident.split("/")[0], DOC)
            stream.write(f"\n[[case]]\nid = {json.dumps(ident)}\ncmd = {json.dumps(cmd)}\n"
                         f"input = {json.dumps(document)}\nfiles = {{ {fields} }}\n")


def accepted_keys(expected, candidates):
    keys = set()
    for key in sorted(candidates):
        result = expected[f"{key}/wrong#cmd"]
        if f'Unknown option "{key}"' not in result["stderr"]:
            keys.add(key)
    for group in sorted(keys | {f"feature/{name}" for name in FEATURES}):
        # The oracle also accepts non-map metadata. Compare that result as recorded.
        wrong_exit = 0 if group == "metadata" else 64
        for kind, exit_code in (("valid-1", 0), ("valid-2", 0), ("wrong", wrong_exit)):
            ident = f"{group}/{kind}#cmd"
            result = expected.get(ident)
            if result is None or result["exit"] != exit_code:
                raise ValueError(f"Oracle result for {ident} must have exit code {exit_code}: {result}")
    return keys


def sensitive_pair(group, expected):
    first, second = ({key: value for key, value in expected[f"{group}/valid-{index}#cmd"].items()
                      if key != "key"} for index in (1, 2))
    return first != second


def defaults_args(args):
    """Yield the position and path of each defaults argument."""
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--":
            break
        if arg in {"-d", "--defaults"}:
            yield index, args[index + 1] if index + 1 < len(args) else ""
            index += 1
        elif arg.startswith("--defaults="):
            yield index, arg.removeprefix("--defaults=")
        elif arg.startswith("-d") and len(arg) > 2:
            yield index, arg[2:]
        index += 1


def defaults_fields(text):
    """Read top-level fields in the JSON and block YAML of harness fixtures."""
    class Pairs(list):
        pass

    if text.lstrip().startswith("{"):
        try:
            fields = json.loads(text, object_pairs_hook=Pairs)
        except json.JSONDecodeError:
            # Malformed JSON can still name the first key.
            fields = [(name, None) for name in re.findall(r'^\s*\{\s*"([\w-]+)"\s*:', text)]
        return fields
    fields = []
    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = re.fullmatch(r"\t*([a-z][a-z0-9-]*)\s*:\s*(.*)", line)
        if match:
            key, value = match.groups()
            if key == "defaults" and value.startswith("[") and value.endswith("]"):
                value = [name.strip() for name in value[1:-1].split(",")]
            elif key == "defaults" and not value:
                value = []
                for following in range(index + 1, len(lines)):
                    match = re.fullmatch(r"\s*-\s+(.*)", lines[following])
                    if not match:
                        break
                    value.append(match[1])
            fields.append((key, value))
    return fields


def legacy_groups(case, args):
    paths = list(defaults_args(args))
    if not paths:
        return set()
    files = case.get("files", {})
    roots = [arg.removeprefix("--data-dir=") for arg in args if arg.startswith("--data-dir=")]
    roots += [".home/.local/share/pandoc", ".home/.pandoc"]
    names = set()
    features = set()
    if len(paths) > 1:
        features.add("ordered-defaults")
    defaults_positions = {position for position, _ in paths}
    defaults_positions.update(position + 1 for position, _ in paths
                              if args[position] in {"-d", "--defaults"})
    end = args.index("--") if "--" in args else len(args)
    options = [position for position in range(end)
               if args[position].startswith("-") and args[position] != "-" and position not in defaults_positions]
    if options and any(options[0] < position < options[-1] for position, _ in paths):
        features.add("option-order")
    pending = [path for _, path in paths]
    visited = set()
    while pending:
        path = pending.pop()
        path = posixpath.normpath(path)
        locations = [path] + [posixpath.join(root, "defaults", path) for root in roots]
        possibilities = (locations if PurePosixPath(path).suffix else
                         [candidate for name in locations for candidate in (name, name + ".yaml")])
        selected = next((name for name in possibilities if name in files), None)
        if path == "/dev/stdin":
            selected, text = path, case.get("input", "")
        elif selected is None:
            continue
        else:
            text = files[selected]
        if selected in visited:
            continue
        visited.add(selected)
        if selected not in (path, path + ".yaml"):
            features.add("datadir-with-yaml" if path.endswith(".yaml") else "datadir-without-yaml")
        for variable, feature in VARIABLES.items():
            if variable in text:
                features.add(feature)
        for key, value in defaults_fields(text):
            names.add(key)
            if key == "defaults":
                features.add("nested-defaults")
                includes = value if isinstance(value, list) else [value]
                for include in includes:
                    if isinstance(include, str):
                        include = include.strip("'\x22")
                        for variable, folder in zip(VARIABLES, (posixpath.dirname(selected), roots[0], ".home")):
                            if include.startswith(variable + "/"):
                                include = posixpath.join(folder, include.removeprefix(variable + "/"))
                                break
                        pending.append(include)
    return names | {f"feature/{name}" for name in features}


def legacy_cases(root):
    for path in sorted((root / "conformance/cases").rglob("*.toml")):
        if path.name.startswith("._"):
            continue
        text = path.read_text(encoding="utf-8")
        if not re.search(r'["\x27](?:-d|--defaults)', text):
            continue
        data = tomllib.loads(text)
        prefix = path.relative_to(root / "conformance/cases").with_suffix("").as_posix()
        for case in data.get("case", []):
            args = case.get("cmd", case.get("args", data.get("args", [])))
            groups = legacy_groups(case, args)
            if groups:
                yield f"{prefix}/{case['id']}#", groups


def collect_statuses(path):
    report = json.loads(path.read_text(encoding="utf-8"))
    statuses = {unit["id"]: unit["status"] for unit in report["units"]}
    if len(statuses) != len(report["units"]) or len(statuses) != report["summary"]["units"]:
        raise ValueError(f"The unit count in {path} is incorrect.")
    return statuses


def group_results(groups, statuses, members):
    results = {group: {"units": {}, "failures": [], "no_result": False} for group in sorted(groups)}
    if set(statuses) != set(members):
        raise ValueError("The harness reports do not match the requested defaults units.")
    for ident, status in sorted(statuses.items()):
        if status == "skipped":
            continue
        if status not in {"pass", "fail", "no_result", "not_recorded"}:
            raise ValueError(f"Unknown harness status {status} for {ident}.")
        for group in sorted(members[ident] & groups):
            result = results[group]
            result["units"][ident] = status
            if status != "pass":
                result["failures"].append(ident)
                result["no_result"] |= status == "no_result"
    return results


def measure_defaults(root, args):
    evidence = args.evidence.resolve()
    generated = evidence / "defaults-inputs"
    oracle_path = Path(args.oracle).resolve()
    inventory = json.loads((root / "spec/inventory/oracle.json").read_text(encoding="utf-8"))
    with oracle_path.open("rb") as binary:
        if hashlib.file_digest(binary, "sha256").hexdigest() != inventory["sha256"]:
            raise ValueError(f"The SHA-256 of {oracle_path} does not match the oracle inventory.")
    manual = Path(os.environ["RANDOC_PANDOC_SRC"]) / "MANUAL.txt"
    options = json.loads((root / "spec/inventory/options.json").read_text(encoding="utf-8"))
    candidates = candidate_keys(manual.read_text(encoding="utf-8"), options)
    cases = generated_cases(candidates)
    write_cases(generated, cases)

    harness = [str(Path(args.harness).resolve())]
    oracle = ["--oracle", str(oracle_path), "--root", str(generated), "--jobs", "2"]
    subprocess.run(harness + ["record"] + oracle, check=True)
    time.sleep(1)
    subprocess.run(harness + ["verify-oracle", "--all"] + oracle, check=True)
    expected = generated / f"conformance/expected/{GENERATED}.json"
    oracle_units = json.loads(expected.read_text(encoding="utf-8"))["units"]
    keys = accepted_keys(oracle_units, candidates)
    groups = keys | {f"feature/{feature}" for feature in FEATURES}
    if args.filter is not None:
        groups = {group for group in groups if args.filter in group}

    statuses = {}
    members = {}
    run = harness + ["run", "--randoc", str(Path(args.randoc).resolve()), "--jobs", "3"]
    for folder, name, selection in (
        (generated, "probes", {f"{GENERATED}/{case}#cmd": {case.rsplit("/", 1)[0]} for case in cases}),
        (root, "legacy", dict(legacy_cases(root))),
    ):
        selected = {unit: tags for unit, tags in selection.items() if tags & groups}
        if not selected:
            continue
        report = evidence / f"defaults-{name}-report.json"
        # One filter per case file. The report can also contain unrelated cases in those files.
        filters = sorted({unit.rsplit("/", 1)[0] if name == "legacy" else
                          f"{GENERATED}/" for unit in selected})
        subprocess.run(run + ["--root", str(folder), "--report", str(report)] +
                       [part for text in filters for part in ("--filter", text)], check=True)
        found = collect_statuses(report)
        for ident, status in found.items():
            prefix = ident.partition("#")[0] + "#" if name == "legacy" else ident
            if prefix in selected:
                statuses[ident] = status
                members[ident] = selected[prefix]
        if not set(selected) <= {ident.partition("#")[0] + "#" if name == "legacy" else ident
                                 for ident in found}:
            raise ValueError(f"The harness report {report} is missing selected defaults cases.")

    report = group_results(groups, statuses, members)
    for group, result in report.items():
        result["sensitive"] = sensitive_pair(group, oracle_units)
    (evidence / REPORT).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--harness", required=True)
    parser.add_argument("--oracle", required=True)
    parser.add_argument("--randoc", required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--filter")
    try:
        measure_defaults(Path.cwd(), parser.parse_args())
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"defaults: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
