"""Build server requests from the conformance cases."""

import base64
import json
import re
import tomllib
from dataclasses import dataclass

JSON = "application/json"
TEXT = "text/plain"
ACCEPTS = (("json", JSON), ("text", TEXT))
BINARY_READERS = {"docx", "epub", "odt", "pptx", "xlsx"}
FLAGS = {
    "-s": "standalone",
    "--standalone": "standalone",
    "--ascii": "ascii",
    "--citeproc": "citeproc",
    "--embed-resources": "embed-resources",
    "--html-q-tags": "html-q-tags",
    "--incremental": "incremental",
    "--list-tables": "list-tables",
    "--number-sections": "number-sections",
    "--reference-links": "reference-links",
    "--section-divs": "section-divs",
    "--strip-comments": "strip-comments",
    "--table-of-contents": "table-of-contents",
    "--toc": "table-of-contents",
}
VALUES = {
    "--abbreviations", "--bibliography", "--csl", "--columns", "--default-image-extension",
    "--dpi", "--email-obfuscation", "--eol", "--epub-cover-image", "--epub-fonts",
    "--epub-metadata", "--epub-subdirectory", "--figure-caption-position", "--id-prefix",
    "--ipynb-output", "--markdown-headings", "--math-method", "--metadata", "--number-offset",
    "--reference-doc", "--reference-location",
    "--shift-heading-level-by", "--slide-level", "--syntax-highlighting",
    "--tab-stop", "--table-caption-position", "--title-prefix", "--toc-depth",
    "--top-level-division", "--wrap",
}
INTEGERS = {"columns", "dpi", "shift-heading-level-by", "slide-level", "tab-stop", "toc-depth"}
HIGHLIGHT_ALIASES = {"--no-highlight": "none", "--listings": "idiomatic"}
HTML_MATH_METHODS = {"gladtex", "katex", "mathjax", "mathml", "plain", "webtex"}
HTML_MATH_FLAGS = {f"--{method}" for method in HTML_MATH_METHODS if method != "plain"}


class UnsupportedOption(ValueError):
    """The server request cannot express a command option."""


@dataclass(frozen=True)
class Request:
    id: str
    path: str
    content: bytes = b""
    accept: str = JSON
    method: str = "POST"
    timeout: int | None = None


def encode(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()


def format_name(spec):
    return re.split(r"[+-]", spec, maxsplit=1)[0]


def options(args):
    """Convert the supported CLI options to fields in the request body."""
    fields = {}
    arguments = iter(args)
    for arg in arguments:
        key, equal, value = arg.partition("=")
        if key in FLAGS and not equal:
            fields[FLAGS[key]] = True
            continue
        if key in HIGHLIGHT_ALIASES and not equal:
            fields["syntax-highlighting"] = HIGHLIGHT_ALIASES[key]
            continue
        if key in {"--natbib", "--biblatex"} and not equal:
            fields["cite-method"] = key[2:]
            continue
        if key in HTML_MATH_FLAGS and not equal:
            fields["html-math-method"] = key[2:]
            continue
        if key not in VALUES:
            raise UnsupportedOption(key)
        if not equal:
            value = next(arguments, None)
            if value is None or value.startswith("--"):
                raise UnsupportedOption(key)
        name = key[2:]
        if name in INTEGERS:
            fields[name] = int(value)
        elif name == "id-prefix":
            fields["identifier-prefix"] = value
        elif name == "math-method":
            if value not in HTML_MATH_METHODS:
                raise UnsupportedOption(key)
            fields["html-math-method"] = value
        elif name == "markdown-headings":
            if value not in {"atx", "setext"}:
                raise UnsupportedOption(key)
            fields["setext-headers"] = value == "setext"
        elif name == "number-offset":
            fields["number-offset"] = [int(number) for number in value.split(",")]
            fields["number-sections"] = True
        elif name in {"bibliography", "epub-fonts"}:
            fields.setdefault(name, []).append(value)
        elif name == "metadata":
            label, separator, text = value.partition(":")
            if not separator:
                raise UnsupportedOption(key)
            fields.setdefault("metadata", {})[label] = text
        else:
            fields[name] = value
    return fields


def text_input(case, fixtures, source):
    if "input_file" in case:
        path = (fixtures / case["input_file"]).resolve()
        if "input" in case or not path.is_relative_to(fixtures) or path == fixtures:
            raise ValueError(f"invalid input_file for {case['id']}")
        data = path.read_bytes()
    else:
        data = case.get("input", "").encode()
    if format_name(source) in BINARY_READERS:
        return base64.b64encode(data).decode("ascii")
    return data.decode("utf-8")


def conformance_requests(root):
    fixtures = (root / "conformance/fixtures").resolve()
    cases = root / "conformance/cases"
    skipped = {}
    requests = []
    for path in sorted(cases.rglob("*.toml")):
        key = path.relative_to(cases).with_suffix("").as_posix()
        if any(part.startswith("._") for part in path.relative_to(cases).parts):
            continue
        document = tomllib.loads(path.read_text(encoding="utf-8"))
        for case in document.get("case", []):
            if "cmd" in case:
                continue
            source = case.get("from", document.get("from"))
            targets = case.get("to", document.get("to", []))
            if not targets:
                continue
            if not source:
                raise ValueError(f"missing input format in {key}/{case['id']}")
            for target in targets:
                unit = f"{key}/{case['id']}#e2e:{target}"
                if format_name(target) == "pdf":
                    skipped[unit] = "PDF output requires an external engine"
                    continue
                try:
                    fields = options(case.get("args", document.get("args", [])))
                except UnsupportedOption as error:
                    skipped[unit] = f"no request field for {error}"
                    continue
                fields.update({"from": source, "to": target,
                               "text": text_input(case, fixtures, source)})
                if case.get("files"):
                    fields["files"] = {name: base64.b64encode(data.encode()).decode("ascii")
                                       for name, data in case["files"].items()}
                body = encode(fields)
                for accept_name, accept in ACCEPTS:
                    requests.append(Request(f"e2e/{unit}/{accept_name}", "/", body, accept))
    return requests, skipped


def fixed_requests():
    batch = [{"text": f"## Berth {number}\n\nCheck the **line**.\n", "to": "html"}
             for number in range(1, 11)]
    slow = {"text": "```python\n" + "tray = [red, blue, green]  # count each tray\n" * 25000
                    + "```\n", "from": "markdown", "to": "html"}
    return [
        Request("batch", "/batch", encode(batch)),
        Request("version", "/version", method="GET"),
        Request("version/text", "/version", accept=TEXT, method="GET"),
        Request("error/invalid-json", "/", b'{"text":'),
        Request("error/unknown-format", "/", encode({"text": "Reef", "to": "reef-report"})),
        Request("error/unknown-field", "/", encode({"text": "Reef", "reef-report": True})),
        Request("error/wrong-type", "/", encode({"text": 19})),
        Request("option/timeout", "/", encode(slow), timeout=1),
    ]
