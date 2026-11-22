#!/usr/bin/env python3
"""Makes deterministic conformance cases for Randoc from a seed.

Usage: casegen.py --seed N --count K --out PATH [options]

For each case the tool writes the input file and a JSON file with the exact
arguments. See README.md for the case format and the families.
"""

import argparse
import dataclasses
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bank  # noqa: E402  (bank.py is next to this file, also under `python3 -I`)

MASK64 = (1 << 64) - 1
GENERATOR = "casegen/1"
DEFAULT_EXTENSIONS = Path(__file__).resolve().parents[2] / "spec" / "inventory" / "extensions.json"
FAMILIES = ("ext-matrix", "json-roundtrip", "options-cli")
FAMILY_WEIGHTS = {"ext-matrix": 70, "json-roundtrip": 10, "options-cli": 20}
ROUNDTRIP_WRITERS_PER_DOCUMENT = 3

# The environment of each oracle run, as the conformance harness uses it.
ORACLE_ENV = {
    "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    "HOME": "/var/empty",
    "LANG": "en_US.UTF-8",
    "LC_ALL": "en_US.UTF-8",
    "TZ": "UTC",
    "SOURCE_DATE_EPOCH": "1700000000",
    "GHCRTS": "-M3g",
}

# Input format -> (bank name, file extension).
READERS = {
    "markdown": ("markdown", "md"),
    "commonmark": ("markdown", "md"),
    "commonmark_x": ("markdown", "md"),
    "gfm": ("markdown", "md"),
    "html": ("html", "html"),
    "latex": ("latex", "tex"),
    "rst": ("rst", "rst"),
    "org": ("org", "org"),
}

# Text writers. Binary writers (docx, odt, epub, pptx, pdf, chunkedhtml) are left out.
COMMON_WRITERS = (
    "html", "latex", "markdown", "gfm", "commonmark_x", "rst", "org", "plain",
    "native", "json", "typst", "docbook5", "jats", "asciidoc", "mediawiki",
)
OTHER_WRITERS = (
    "html4", "html5", "context", "beamer", "markdown_strict", "markdown_mmd",
    "markdown_phpextra", "commonmark", "djot", "asciidoctor", "asciidoc_legacy",
    "dokuwiki", "zimwiki", "xwiki", "textile", "jira", "muse", "haddock", "man",
    "ms", "texinfo", "docbook4", "jats_archiving", "jats_publishing",
    "jats_articleauthoring", "tei", "icml", "opendocument", "rtf", "revealjs",
    "slidy", "markua", "bbcode", "ansi", "vimdoc", "xml",
)
WRITERS = COMMON_WRITERS + OTHER_WRITERS

# The options of the ext-matrix and json-roundtrip families. Each entry makes
# the arguments of one option from the random source.
SAFE_OPTIONS = (
    lambda r: ["--wrap=" + r.choice(("auto", "none", "preserve"))],
    lambda r: [f"--columns={r.between(8, 140)}"],
    lambda r: ["--toc"] + ([f"--toc-depth={r.between(1, 6)}"] if r.chance(50) else []),
    lambda r: ["-s"],
    lambda r: ["--number-sections"] + ([f"--number-offset={r.between(1, 4)}"] if r.chance(25) else []),
    lambda r: [f"--shift-heading-level-by={r.between(-1, 2)}"],
    lambda r: ["--top-level-division=" + r.choice(("default", "section", "chapter", "part"))],
    lambda r: ["--markdown-headings=" + r.choice(("atx", "setext"))],
    lambda r: ["--reference-links"]
    + (["--reference-location=" + r.choice(("block", "section", "document"))] if r.chance(50) else []),
    lambda r: [f"--tab-stop={r.between(1, 8)}"],
    lambda r: ["--preserve-tabs"],
    lambda r: ["--ascii"],
    lambda r: [f"--id-prefix={r.ascii_word()}-"],
    lambda r: ["--section-divs"],
    lambda r: ["--eol=" + r.choice(("lf", "crlf"))],
    lambda r: ["--strip-comments"],
    lambda r: ["--html-q-tags"],
    lambda r: ["--list-tables"],
    lambda r: ["-M", f"title={r.ascii_word().capitalize()}"],
)
SAFE_OPTION_NAMES = (
    "--wrap", "--columns", "--toc", "--toc-depth", "-s", "--number-sections",
    "--number-offset", "--shift-heading-level-by", "--top-level-division",
    "--markdown-headings", "--reference-links", "--reference-location",
    "--tab-stop", "--preserve-tabs", "--ascii", "--id-prefix", "--section-divs",
    "--eol", "--strip-comments", "--html-q-tags", "--list-tables", "-M",
)

TOKEN = re.compile(r"@(w|W|v|V|n|d|s|p|id|id2|ref|url|lang|table:[a-z-]+)@")
UNDERLINE = re.compile(r"^@u:(.)@$")
LEFTOVER = re.compile(r"@(?:w|W|v|V|n|d|s|p|id|id2|ref|url|lang|table:[a-z-]+|u:.)@")
DOCUMENT_CLOSERS = (("\\begin{document}", "\\end{document}"), ("<body>", "</body>\n</html>"))
ASCII_WORDS = tuple(word for word in bank.WORDS if word.isascii())


class Rng:
    """SplitMix64. The sequence does not depend on the Python version."""

    def __init__(self, seed):
        self.state = seed & MASK64

    def next64(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK64
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
        return z ^ (z >> 31)

    def below(self, n):
        """Returns an integer in [0, n) without modulo bias."""
        limit = MASK64 - (MASK64 + 1) % n
        while True:
            value = self.next64()
            if value <= limit:
                return value % n

    def between(self, low, high):
        return low + self.below(high - low + 1)

    def chance(self, percent):
        return self.below(100) < percent

    def choice(self, items):
        return items[self.below(len(items))]

    def sample(self, items, k):
        pool = list(items)
        for i in range(min(k, len(pool))):
            j = i + self.below(len(pool) - i)
            pool[i], pool[j] = pool[j], pool[i]
        return pool[:k]

    def weighted(self, weights):
        """Returns a key of `weights` (a dict of key -> integer weight)."""
        pick = self.below(sum(weights.values()))
        for key, weight in weights.items():
            if pick < weight:
                return key
            pick -= weight
        raise AssertionError("unreachable")

    def word(self):
        return self.choice(bank.WORDS)

    def ascii_word(self):
        return self.choice(ASCII_WORDS)

    def ident(self):
        return f"{self.ascii_word()}{self.between(1, 99)}"


def group_rng(seed, group):
    """The random source of one group of cases: it depends only on the seed and the group number."""
    digest = hashlib.sha256(f"{GENERATOR}:{seed}:{group}".encode()).digest()
    return Rng(int.from_bytes(digest[:8], "big"))


def sentence(r):
    words = [r.word() for _ in range(r.between(5, 12))]
    words[0] = words[0].capitalize()
    if len(words) > 6 and r.chance(30):
        words[3] += ","
    return " ".join(words) + r.choice((".", ".", ".", "?", "!"))


def table(r, style):
    columns, rows = r.between(2, 4), r.between(2, 4)
    header = [r.ascii_word().capitalize() for _ in range(columns)]
    body = [[r.choice((r.ascii_word(), str(r.between(1, 999)), f"{r.ascii_word()} {r.ascii_word()}"))
             for _ in range(columns)] for _ in range(rows)]
    widths = [max(len(cell) for cell in column) for column in zip(header, *body)]

    def padded(cells):
        return [cell.ljust(width) for cell, width in zip(cells, widths)]

    if style == "pipe":
        marks = [r.choice(("---", ":--", "--:", ":-:")) for _ in range(columns)]
        lines = ["| " + " | ".join(header) + " |", "|" + "|".join(marks) + "|"]
        lines += ["| " + " | ".join(row) + " |" for row in body]
    elif style == "org":
        lines = ["| " + " | ".join(padded(header)) + " |",
                 "|" + "+".join("-" * (w + 2) for w in widths) + "|"]
        lines += ["| " + " | ".join(padded(row)) + " |" for row in body]
    elif style == "grid":
        rule = "+" + "+".join("-" * (w + 2) for w in widths) + "+"
        lines = [rule, "| " + " | ".join(padded(header)) + " |",
                 "+" + "+".join("=" * (w + 2) for w in widths) + "+"]
        for row in body:
            lines += ["| " + " | ".join(padded(row)) + " |", rule]
    elif style == "simple":
        lines = ["  ".join(padded(header)).rstrip(), "  ".join("-" * w for w in widths)]
        lines += ["  ".join(padded(row)).rstrip() for row in body]
    elif style == "rst-simple":
        rule = "  ".join("=" * w for w in widths)
        lines = [rule, "  ".join(padded(header)).rstrip(), rule]
        lines += ["  ".join(padded(row)).rstrip() for row in body] + [rule]
    else:
        raise ValueError(f"unknown table style {style}")
    return "\n".join(lines)


def fill(r, template, ref):
    """Replaces the tokens of `template` (see bank.py) with random text."""
    fixed = {"id": r.ident(), "id2": r.ident(), "v": r.ascii_word(), "ref": ref}
    fixed["V"] = fixed["v"].capitalize()

    def token(match):
        name = match.group(1)
        if name in fixed:
            return fixed[name]
        if name == "w":
            return r.word()
        if name == "W":
            return r.word().capitalize()
        if name == "n":
            return str(r.between(1, 99))
        if name == "d":
            return str(r.between(1, 9))
        if name == "s":
            return sentence(r)
        if name == "p":
            return " ".join(sentence(r) for _ in range(r.between(2, 4)))
        if name == "url":
            return f"https://example.org/{r.ascii_word()}/{r.ascii_word()}.html"
        if name == "lang":
            return r.choice(bank.LANGS)
        return table(r, name.split(":", 1)[1])

    lines = TOKEN.sub(token, template).split("\n")
    for index, line in enumerate(lines):
        match = UNDERLINE.match(line)
        if match:
            above = lines[index - 1] if index > 0 else ""
            title = above if above.strip() and not UNDERLINE.match(above) else lines[index + 1]
            lines[index] = match.group(1) * max(len(title), 3)
    return "\n".join(lines)


def document(r, reader):
    """A synthesized document for the input format `reader`."""
    snippets, headers = bank.BANKS[READERS[reader][0]]
    ref = r.ident()
    parts, closers = [], []
    if r.chance(35):
        header = fill(r, r.choice(headers), ref)
        parts.append(header)
        closers = [closer for opener, closer in DOCUMENT_CLOSERS if opener in header]
    parts += [fill(r, template, ref) for template in r.sample(snippets, r.between(3, 7))]
    text = "\n\n".join(parts + closers) + "\n"
    return text.replace("\n", "\r\n") if r.chance(5) else text


def toggles(r, extensions, fmt, count):
    """`count` extension toggles that flip the default state of extensions that `fmt` supports."""
    table_of_format = extensions["formats"][fmt]
    default = set(table_of_format["default"])
    chosen = r.sample(sorted(table_of_format["supported"]), count)
    return "".join(("-" if name in default else "+") + name for name in chosen)


def options(r, low, high):
    makers = r.sample(range(len(SAFE_OPTIONS)), r.between(low, high))
    return [arg for index in sorted(makers) for arg in SAFE_OPTIONS[index](r)]


@dataclasses.dataclass
class Case:
    family: str
    args: list
    content: bytes
    extension: str
    derived: bool = False  # the input is the oracle's JSON of the case before it


def ext_matrix(r, extensions):
    reader = r.choice(tuple(READERS))
    text = document(r, reader)
    writer = r.choice(COMMON_WRITERS) if r.chance(60) else r.choice(WRITERS)
    reader_spec = reader + toggles(r, extensions, reader, r.between(0, 3))
    writer_spec = writer + (toggles(r, extensions, writer, r.between(1, 2)) if r.chance(30) else "")
    args = ["-f", reader_spec, "-t", writer_spec, *options(r, 0, 4)]
    return [Case("ext-matrix", [*args, "FILE"], text.encode(), READERS[reader][1])]


def json_roundtrip(r, oracle_json):
    reader = r.choice(tuple(READERS))
    content = document(r, reader).encode()
    writers = r.sample(WRITERS, ROUNDTRIP_WRITERS_PER_DOCUMENT)
    writer_args = [["-f", "json", "-t", writer, *options(r, 0, 2), "FILE"] for writer in writers]
    cases = [Case("json-roundtrip", ["-f", reader, "-t", "json", "FILE"], content, READERS[reader][1])]
    tree = oracle_json(reader, content)
    if tree is not None:
        cases += [Case("json-roundtrip", args, tree, "json", derived=True) for args in writer_args]
    return cases


def options_cli(r, extensions):
    """A case for errors, warnings and exit codes. FILE marks the place of the input file."""
    fmt = r.choice(("markdown", "gfm", "commonmark_x", "html", "rst", "org", "latex"))
    writer = r.choice(COMMON_WRITERS)
    content = document(r, "markdown").encode()
    unsupported = sorted(set(extensions["extensions"]) - set(extensions["formats"][fmt]["supported"]))
    word, word2 = r.ascii_word(), r.ascii_word()
    bad_value = r.choice((word, f"{word}{r.between(1, 9)}", "", "-3", "0"))
    valued = ("wrap", "columns", "tab-stop", "shift-heading-level-by", "top-level-division", "eol",
              "markdown-headings", "track-changes", "toc-depth", "number-offset", "reference-location",
              "dpi", "ipynb-output", "email-obfuscation")
    templates = (
        lambda: ["-f", f"{word}{word2}", "-t", writer, "FILE"],
        lambda: ["-f", fmt, "-t", f"{word}_{word2}", "FILE"],
        lambda: ["-f", f"{fmt}{r.choice('+-')}{word}_{word2}", "-t", writer, "FILE"],
        lambda: ["-f", f"{fmt}+{r.choice(unsupported)}", "-t", writer, "FILE"],
        lambda: ["-f", "markdown", "-t", f"{writer}{r.choice('+-')}{word}_{word2}", "FILE"],
        lambda: ["-f", "markdown", "-t", writer, f"--{r.choice(valued)}={bad_value}", "FILE"],
        lambda: ["-f", "markdown", "-t", writer, f"--{word}-{word2}", "FILE"],
        lambda: ["-f", "markdown", "-t", writer, "FILE", f"--{r.choice(('columns', 'wrap', 'tab-stop', 'to'))}"],
        lambda: ["-f", "markdown", "-t", writer, f"{word}-missing.md"],
        lambda: ["-f", r.choice(("docx", "odt", "epub", "pptx", "xlsx", "json", "ipynb", "csljson")), "-t", writer,
                 "FILE"],
        lambda: ["-f", "markdown", "-t", "html", r.choice(("--self-contained", "--embed-resources")), "FILE"],
        lambda: ["-f", r.choice(("markdown_github", "markdown")), "-t", r.choice(("markdown_github", "gfm")), "FILE"],
        lambda: ["-f", fmt, "-f", "markdown", "-t", writer, "-t", r.choice(COMMON_WRITERS), "FILE"],
        lambda: ["-f", "markdown", "-t", writer, f"--wrap={r.choice(('none', 'auto'))}",
                 f"--wrap={r.choice(('preserve', 'auto'))}", f"--columns={r.between(1, 5)}", "-s", "--toc", "FILE"],
        lambda: ["-f", "markdown", "-t", writer,
                 f"--{r.choice(('template', 'metadata-file', 'defaults', 'lua-filter', 'reference-doc'))}={word}-missing",
                 "FILE"],
        lambda: [f"--list-extensions={r.choice(tuple(extensions['formats']))}"],
        lambda: [r.choice(("--list-input-formats", "--list-output-formats", "--list-highlight-styles"))],
        lambda: ["-f", "markdown", "-t", writer, "--file-scope", "--strip-comments", "FILE", "FILE"],
    )
    return [Case("options-cli", templates[r.below(len(templates))](), content, "md")]


def generate(seed, count, extensions, families=FAMILIES, oracle_json=None):
    """Returns a dict of file name -> bytes for `count` cases.

    Case `i` depends only on the seed, the families and the oracle, not on
    `count`: a shorter run gives a prefix of a longer run.
    """
    if "json-roundtrip" in families and oracle_json is None:
        raise ValueError("the json-roundtrip family needs the oracle (--oracle)")
    weights = {family: FAMILY_WEIGHTS[family] for family in FAMILIES if family in families}
    width = max(4, len(str(count)))
    files, index, group = {}, 0, 0
    while index < count:
        r = group_rng(seed, group)
        family = r.weighted(weights)
        if family == "ext-matrix":
            cases = ext_matrix(r, extensions)
        elif family == "json-roundtrip":
            cases = json_roundtrip(r, oracle_json)
        else:
            cases = options_cli(r, extensions)
        read_id = None
        for case in cases:
            if index == count:
                break
            index += 1
            case_id = f"{index:0{width}d}"
            input_name = f"{case_id}.{case.extension}"
            args = [input_name if arg == "FILE" else arg for arg in case.args]
            record = {"id": case_id, "family": case.family, "input": input_name, "args": args,
                      "generator": GENERATOR}
            if case.derived:
                record["source"] = read_id
            else:
                read_id = case_id
            files[input_name] = case.content
            files[f"{case_id}.case.json"] = (json.dumps(record, indent=1, ensure_ascii=False, sort_keys=True)
                                             + "\n").encode()
        group += 1
    return files


def oracle_runner(oracle, limit_run):
    """Returns a function that gives the oracle's JSON of a document, or None when the oracle fails."""

    def run(reader, content):
        command = [str(oracle), "-f", reader, "-t", "json"]
        if limit_run:
            command = [sys.executable, str(limit_run), "3000", "60", *command]
        try:
            result = subprocess.run(command, input=content, capture_output=True, env=ORACLE_ENV, timeout=120)
        except subprocess.TimeoutExpired:
            return None
        return result.stdout if result.returncode == 0 else None

    return run


def write_directory(files, out):
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        raise SystemExit(f"casegen: {out} is not empty")
    for name in sorted(files):
        (out / name).write_bytes(files[name])


def write_tar(files, out):
    """Writes one tar archive with fixed metadata, so equal cases give equal bytes."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        for name in sorted(files):
            info = tarfile.TarInfo(name)
            info.size, info.mtime, info.mode = len(files[name]), 0, 0o644
            archive.addfile(info, io.BytesIO(files[name]))
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(buffer.getvalue())


def read_cases(path):
    """Reads the cases of a directory or a tar archive: a list of (case record, input bytes)."""
    path = Path(path)
    if path.is_dir():
        files = {p.name: p.read_bytes() for p in path.iterdir() if p.is_file() and not p.name.startswith("._")}
    else:
        with tarfile.open(path) as archive:
            files = {m.name: archive.extractfile(m).read() for m in archive.getmembers() if m.isfile()}
    cases = []
    for name in sorted(files):
        if name.endswith(".case.json"):
            record = json.loads(files[name])
            cases.append((record, files[record["input"]]))
    return cases


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--seed", type=int, required=True, help="0 to 2**64-1; the visible seeds are 1 to 1000")
    parser.add_argument("--count", type=int, required=True, help="the number of cases")
    parser.add_argument("--out", type=Path, required=True,
                        help="an empty or new directory, or a file name that ends in .tar")
    parser.add_argument("--families", default=",".join(FAMILIES),
                        help=f"a comma-separated subset of {', '.join(FAMILIES)}")
    parser.add_argument("--extensions", type=Path, default=DEFAULT_EXTENSIONS,
                        help="the extension inventory (spec/inventory/extensions.json)")
    parser.add_argument("--oracle", type=Path, default=os.environ.get("RANDOC_ORACLE_PANDOC"),
                        help="Pandoc 3.13 for json-roundtrip (default: $RANDOC_ORACLE_PANDOC)")
    parser.add_argument("--limit-run", type=Path, default=None,
                        help="run each oracle call as `limit-run 3000 60 ORACLE ...`")
    args = parser.parse_args(argv)
    if not 0 <= args.seed <= MASK64:
        parser.error("--seed must be in 0 to 2**64-1")
    if args.count < 1:
        parser.error("--count must be 1 or more")
    families = tuple(name.strip() for name in args.families.split(",") if name.strip())
    unknown = sorted(set(families) - set(FAMILIES))
    if unknown or not families:
        parser.error(f"unknown family: {', '.join(unknown) or '(none)'}")
    oracle_json = None
    if "json-roundtrip" in families:
        if args.oracle is None or not args.oracle.is_file():
            parser.error("json-roundtrip needs --oracle or $RANDOC_ORACLE_PANDOC (or leave the family out)")
        oracle_json = oracle_runner(args.oracle, args.limit_run)
    extensions = json.loads(args.extensions.read_text(encoding="utf-8"))
    files = generate(args.seed, args.count, extensions, families, oracle_json)
    if args.out.suffix == ".tar":
        write_tar(files, args.out)
    else:
        write_directory(files, args.out)
    counts = {}
    for name, data in files.items():
        if name.endswith(".case.json"):
            family = json.loads(data)["family"]
            counts[family] = counts.get(family, 0) + 1
    summary = ", ".join(f"{family} {counts.get(family, 0)}" for family in FAMILIES if family in families)
    print(f"casegen: {args.count} cases ({summary}) in {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
