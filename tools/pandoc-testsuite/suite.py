"""Discovery of the tests in the Pandoc test tree.

The functions here read the test definitions from the Pandoc source tree when
the tool runs. The repository holds no Pandoc test content.
"""

from __future__ import annotations

import difflib
import json
import os
import re
from dataclasses import dataclass, field, replace
from typing import Callable, Union


@dataclass(frozen=True)
class Outcome:
    """The result of one run of the program."""

    code: int
    stdout: bytes
    stderr: bytes
    timed_out: bool = False


# A function that runs the program. The first argument is an argument list
# (the program runs directly) or a string (a shell runs the command line).
Execute = Callable[[Union[tuple, str], bytes], Outcome]


@dataclass(frozen=True)
class CommandCase:
    """A code block of `test/command/*.md`: a shell command, its input and the expected text."""

    id: str
    group: str
    command: str
    stdin: str
    expected: str

    needs_reference = False

    def evaluate(self, execute: Execute, suite: str, reference: Execute | None = None) -> str | None:
        out = execute(self.command, self.stdin.encode("utf-8"))
        if out.timed_out:
            return "timeout"
        actual = command_output(out)
        return None if actual == self.expected else diff_text(self.expected, actual)


@dataclass(frozen=True)
class GoldenCase:
    """A run with an argument list whose standard output must equal a file of the suite."""

    id: str
    group: str
    args: tuple
    expected_path: str
    normalize: str = ""

    needs_reference = False

    def evaluate(self, execute: Execute, suite: str, reference: Execute | None = None) -> str | None:
        out = execute(self.args, b"")
        if out.timed_out:
            return "timeout"
        if out.code != 0:
            return f"exit code {out.code}\n{decode(out.stderr)}"
        norm = NORMALIZERS[self.normalize]
        actual = norm(decode(out.stdout).replace("\r", ""))
        with open(os.path.join(suite, self.expected_path), "rb") as handle:
            # Tests/Old.hs removes carriage returns from the output only.
            expected = norm(decode(handle.read()))
        return None if actual == expected else diff_text(expected, actual)


@dataclass(frozen=True)
class AstCase:
    """A reader test that compares two documents as Pandoc JSON.

    `reader` is the format of `input_path`. `extensions` is None for the
    default extensions of the reader, or the exact set of enabled extensions.
    The reference program (the oracle) reads `expected_path` in the format
    `expected_format`. The program under test reads only `input_path`, so a
    defect of that program cannot change both sides of the comparison.
    """

    id: str
    group: str
    reader: str
    input_path: str
    expected_format: str
    expected_path: str
    extensions: frozenset | None = None
    extra_args: tuple = ()

    needs_reference = True

    def evaluate(self, execute: Execute, suite: str, reference: Execute | None = None) -> str | None:
        if reference is None:
            return "not checked: this test needs a reference program (--oracle)"
        spec = self.reader
        if self.extensions is not None:
            try:
                spec = format_with_exact_extensions(reference, self.reader, self.extensions)
            except HelperError as error:
                return str(error)
        actual = execute(("-f", spec, "-t", "json", *self.extra_args, self.input_path), b"")
        expected = reference(("-f", self.expected_format, "-t", "json", self.expected_path), b"")
        for name, out in (("actual", actual), ("expected", expected)):
            if out.timed_out:
                return f"timeout ({name})"
            if out.code != 0:
                return f"exit code {out.code} ({name})\n{decode(out.stderr)}"
        try:
            left, right = parse_strict_json(actual.stdout), parse_strict_json(expected.stdout)
        except ValueError as error:
            return f"output is not JSON: {error}"
        for name, doc in (("actual", left), ("expected", right)):
            problem = document_problem(doc)
            if problem:
                return f"{name} output is not a Pandoc JSON document: {problem}"
        if not json_equal(left["pandoc-api-version"], right["pandoc-api-version"]):
            return f"pandoc-api-version {left['pandoc-api-version']} is not {right['pandoc-api-version']}"
        if json_equal(left, right):
            return None
        return diff_text(json.dumps(right, indent=1), json.dumps(left, indent=1))


def _reject_constant(name: str):
    raise ValueError(f"{name} is not JSON")


def _unique_keys(pairs: list) -> dict:
    keys = [key for key, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError("an object has a key two times")
    return dict(pairs)


def parse_strict_json(data: bytes):
    """Parses JSON and rejects NaN, Infinity and repeated keys."""
    return json.loads(data, parse_constant=_reject_constant, object_pairs_hook=_unique_keys)


def document_problem(doc) -> str:
    """A description of the first defect of the top level of a Pandoc JSON document, or ""."""
    if not isinstance(doc, dict):
        return "the top level is not an object"
    version = doc.get("pandoc-api-version")
    if not isinstance(version, list) or not version or not all(type(part) is int for part in version):
        return "pandoc-api-version is not a list of integers"
    if not isinstance(doc.get("blocks"), list) or not isinstance(doc.get("meta"), dict):
        return "blocks or meta is missing"
    return ""


def json_equal(left, right) -> bool:
    """Equality of decoded JSON values that keeps Booleans, integers, floats
    and strings apart. In Python, True == 1 and 1 == 1.0."""
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(json_equal(left[k], right[k]) for k in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(json_equal(a, b) for a, b in zip(left, right))
    return left == right


Case = Union[CommandCase, GoldenCase, AstCase]


@dataclass(frozen=True)
class Unsupported:
    """A group of tests that the tool cannot run."""

    group: str
    count: int
    reason: str


@dataclass
class Discovery:
    """All cases of the suite and the groups that the tool cannot run."""

    cases: list = field(default_factory=list)
    unsupported: list = field(default_factory=list)


def decode(data: bytes) -> str:
    return data.decode("utf-8", errors="surrogateescape")


def hs_lines(text: str) -> list:
    """Haskell `lines`: split at newlines; a final newline gives no empty line."""
    if not text:
        return []
    parts = text.split("\n")
    if parts[-1] == "":
        parts.pop()
    return parts


def hs_unlines(lines: list) -> str:
    return "".join(line + "\n" for line in lines)


def command_output(out: Outcome) -> str:
    """The text that the Pandoc command tests compare: standard error lines
    with a `2> ` prefix, then standard output, then `=> CODE` for a failure."""
    err = hs_unlines(["2> " + line for line in hs_lines(decode(out.stderr))])
    text = (err + decode(out.stdout)).replace("\r", "")
    if out.code != 0:
        text += f"=> {out.code}\n"
    return text


def diff_text(expected: str, actual: str, limit: int = 40) -> str:
    lines = difflib.unified_diff(
        expected.splitlines(), actual.splitlines(), "expected", "actual", lineterm="", n=1
    )
    out = list(lines)
    if len(out) > limit:
        out = out[:limit] + [f"... {len(out) - limit} more lines"]
    return "\n".join(out)


def fb2_normalize(text: str) -> str:
    """Old.hs `fb2WriterTest`: break the XML between tags and drop `<binary` lines."""
    return hs_unlines([line for line in hs_lines(split_tags(text)) if not hs_starts_with("<binary ", line)])


def split_tags(text: str) -> str:
    """`formatXML` of Old.hs. It walks the pairs of neighbour characters and
    puts a newline in each `><`, except in the last pair. A text of fewer
    than two characters gives an empty text."""
    pairs = list(zip(text, text[1:]))
    out = []
    for index, (char, following) in enumerate(pairs):
        if index == len(pairs) - 1:
            out.append(char + following)
        elif (char, following) == (">", "<"):
            out.append(">\n")
        else:
            out.append(char)
    return "".join(out)


def hs_starts_with(tag: str, line: str) -> bool:
    """The `startsWith` of Old.hs. It compares only the common length, so it
    is also true for an empty line and for a line that is a prefix of `tag`."""
    return all(a == b for a, b in zip(tag, line))


NORMALIZERS = {"": lambda text: text, "fb2": fb2_normalize}

_EXTENSION_CACHE: dict = {}


class HelperError(Exception):
    """A helper call of the reference program failed. The test fails."""


def format_with_exact_extensions(execute: Execute, reader: str, enabled: frozenset) -> str:
    """A format string that enables exactly `enabled` for `reader`.

    The Haskell tests set `readerExtensions` to a full set. The command line
    can only change the default set, so the tool asks the reference program
    for the defaults of the reader. Only a successful answer goes in the cache.
    """
    if reader not in _EXTENSION_CACHE:
        out = execute((f"--list-extensions={reader}",), b"")
        if out.timed_out:
            raise HelperError(f"timeout (--list-extensions={reader})")
        if out.code != 0:
            raise HelperError(f"exit code {out.code} (--list-extensions={reader})\n{decode(out.stderr)}")
        words = decode(out.stdout).split()
        if not words or not all(re.fullmatch(r"[+-][a-z0-9_]+", word) for word in words):
            raise HelperError(f"--list-extensions={reader} did not give a list of extensions")
        _EXTENSION_CACHE[reader] = {word[1:] for word in words if word.startswith("+")}
    defaults = _EXTENSION_CACHE[reader]
    off = "".join(f"-{name}" for name in sorted(defaults - enabled))
    on = "".join(f"+{name}" for name in sorted(enabled - defaults))
    return reader + off + on


# Command tests


FENCE = re.compile(r"^( {0,3})(`{3,}|~{3,})(.*)$")


def code_blocks(text: str) -> list:
    """The contents of the fenced code blocks of a Markdown file, in order.

    Like the Pandoc Markdown reader, the function skips HTML comments. It
    keeps tabs, because Tests/Command.hs calls the reader directly and not
    through the command line, which expands tabs.
    """
    blocks = []
    lines = text.split("\n")
    index = 0
    while index < len(lines):
        if lines[index].lstrip(" ").startswith("<!--") and len(lines[index]) - len(lines[index].lstrip(" ")) < 4:
            start = lines[index].index("<!--") + 4
            while index < len(lines) and "-->" not in lines[index][start:]:
                index += 1
                start = 0
            index += 1
            continue
        match = FENCE.match(lines[index])
        if not match or (match.group(2)[0] == "`" and "`" in match.group(3)):
            index += 1
            continue
        indent, fence = len(match.group(1)), match.group(2)
        closing = re.compile(r"^ {0,3}" + re.escape(fence[0]) + "{" + str(len(fence)) + r",}[ \t]*$")
        body = []
        end = index + 1
        while end < len(lines) and not closing.match(lines[end]):
            line = lines[end]
            strip = len(line) - len(line.lstrip(" "))
            body.append(line[min(indent, strip):])
            end += 1
        if end == len(lines):
            break
        blocks.append("\n".join(body))
        index = end + 1
    return blocks


def parse_command_block(code: str) -> tuple | None:
    """Splits a command test into (command, stdin, expected) as Tests/Command.hs does.

    Returns None when the block does not start with a `%` line.
    """
    codelines = hs_lines(code)
    split = 0
    while split < len(codelines) and codelines[split].endswith("\\"):
        split += 1
    continuations, rest = codelines[:split], codelines[split:]
    joined = " ".join([line[:-1] for line in continuations] + rest[:1])
    if not joined.startswith("%"):
        return None
    command = joined[1:].lstrip(" ")
    body = rest[1:]
    stop = body.index("^D") if "^D" in body else len(body)
    inplines, after = body[:stop], body[stop + 1:]
    normlines = []
    for line in after:
        if line == ".":
            break
        normlines.append(line)
    return command, hs_unlines(inplines), hs_unlines(normlines)


def discover_commands(suite: str, found: Discovery) -> None:
    directory = os.path.join(suite, "command")
    malformed = 0
    for name in sorted(os.listdir(directory)):
        if name.startswith("._") or not name.endswith(".md"):
            continue
        with open(os.path.join(directory, name), encoding="utf-8") as handle:
            blocks = code_blocks(handle.read())
        for number, block in enumerate(blocks, start=1):
            parsed = parse_command_block(block)
            if parsed is None:
                malformed += 1
                continue
            command, stdin, expected = parsed
            found.cases.append(CommandCase(f"command/{name}#{number}", "command", command, stdin, expected))
    if malformed:
        found.unsupported.append(
            Unsupported("command", malformed, "code block without a `%` command line (Pandoc fails it too)")
        )


# Golden file tests of Tests/Old.hs


class HaskellError(Exception):
    """An expression that the small evaluator does not understand."""


TOKEN = re.compile(
    r'\s+|--[^\n]*|"(?:[^"\\]|\\.)*"|\\|->|\+\+|<\.>|</>|[\[\](),$]|[A-Za-z_][A-Za-z0-9_\']*'
)


def tokenize(text: str) -> list:
    tokens = []
    pos = 0
    while pos < len(text):
        match = TOKEN.match(text, pos)
        if not match:
            raise HaskellError(f"unknown text at {text[pos:pos + 30]!r}")
        token = match.group(0)
        pos = match.end()
        if token.isspace() or token.startswith("--"):
            continue
        tokens.append(token)
    return tokens


def haskell_string(token: str) -> str:
    return json.loads(token)


@dataclass(frozen=True)
class Lambda:
    param: str
    body: object


@dataclass(frozen=True)
class Apply:
    func: object
    args: tuple


class Parser:
    """Parses the expression subset of the `tests` list in Tests/Old.hs."""

    BINARY = {"++": 5, "<.>": 7, "</>": 7}

    def __init__(self, tokens: list):
        self.tokens = tokens
        self.pos = 0

    def peek(self):
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self, expected=None):
        token = self.peek()
        if token is None or (expected is not None and token != expected):
            raise HaskellError(f"expected {expected!r}, found {token!r}")
        self.pos += 1
        return token

    def expression(self):
        left = self.operators(0)
        if self.peek() == "$":
            self.take()
            return Apply(left, (self.expression(),))
        return left

    def operators(self, level: int):
        left = self.application()
        while self.peek() in self.BINARY and self.BINARY[self.peek()] >= level:
            op = self.take()
            right = self.operators(self.BINARY[op] + (0 if op == "++" else 1))
            left = Apply(op, (left, right))
        return left

    def application(self):
        func = self.atom()
        args = []
        while self.peek() is not None and self.peek() not in (",", "]", ")", "$", *self.BINARY):
            args.append(self.atom())
        return Apply(func, tuple(args)) if args else func

    def atom(self):
        token = self.take()
        if token.startswith('"'):
            return haskell_string(token)
        if token == "[":
            items = []
            while self.peek() != "]":
                items.append(self.expression())
                if self.peek() == ",":
                    self.take()
            self.take("]")
            return ("list", tuple(items))
        if token == "(":
            inner = self.expression()
            self.take(")")
            return inner
        if token == "\\":
            param = self.take()
            self.take("->")
            return Lambda(param, self.expression())
        if re.match(r"[A-Za-z_]", token):
            return ("var", token)
        raise HaskellError(f"unexpected token {token!r}")


@dataclass(frozen=True)
class Golden:
    name: str
    args: tuple
    input: str
    norm: str
    normalize: str = ""


@dataclass(frozen=True)
class Group:
    name: str
    children: tuple


def _writer_opts(fmt):
    return ["-r", "native", "-w", fmt, "--columns=80", "--variable", "pandoc-version="]


def _writer_tests(fmt):
    opts = _writer_opts(fmt)
    return [
        Golden("basic", tuple(opts + ["-s"]), "testsuite.native", f"writer.{fmt}"),
        Golden("tables", tuple(opts), "tables.native", f"tables.{fmt}"),
    ]


def _ext_writer_tests(fmt):
    return _writer_tests(fmt) + [
        Golden(f"{name} table", tuple(_writer_opts(fmt)), f"tables/{name}.native", f"tables/{name}.{fmt}")
        for name in ["planets", "nordics", "students"]
    ]


def _lhs_writer_tests(fmt):
    def one(name, out):
        args = ("--wrap=preserve", "-r", "native", "--markdown-headings=setext", "-w", out)
        return Golden(name, args, "lhs-test.native", f"lhs-test.{out}")

    return [one("lhs to normal", fmt), one("lhs to lhs", fmt + "+lhs")]


def _lhs_reader_test(fmt):
    norm = "lhs-test-markdown.native" if fmt == "markdown+lhs" else "lhs-test.native"
    return Golden("lhs", ("-r", fmt, "-w", "native"), f"lhs-test.{fmt}", norm)


def _s5_writer_test(modifier, opts, fmt):
    args = ("-r", "native", "-w", fmt, *opts, "--variable", "pandoc-version=")
    return Golden(f"{fmt} writer ({modifier})", args, "s5.native", f"s5-{modifier}.html")


def _fb2_writer_test(title, opts, inp, norm):
    return Golden(title, ("-t", "fb2", *opts), inp, norm, "fb2")


# The helper functions of Tests/Old.hs, written from their Haskell definitions.
OLD_BUILTINS = {
    "test'": lambda name, opts, inp, norm: Golden(name, tuple(opts), inp, norm),
    "writerTests'": _writer_tests,
    "extWriterTests'": _ext_writer_tests,
    "lhsWriterTests'": _lhs_writer_tests,
    "lhsReaderTest'": _lhs_reader_test,
    "s5WriterTest'": _s5_writer_test,
    "fb2WriterTest'": _fb2_writer_test,
    "testGroup": lambda name, children: Group(name, tuple(children)),
    "mconcat": lambda lists: [item for sub in lists for item in sub],
    "map": lambda func, items: [func(item) for item in items],
}


def evaluate(node, env: dict):
    if isinstance(node, str):
        return node
    if isinstance(node, Lambda):
        return lambda value: evaluate(node.body, {**env, node.param: value})
    if isinstance(node, tuple) and node[0] == "list":
        return [evaluate(item, env) for item in node[1]]
    if isinstance(node, tuple) and node[0] == "var":
        name = node[1]
        if name in env:
            return env[name]
        if name in OLD_BUILTINS:
            return OLD_BUILTINS[name]
        raise HaskellError(f"unknown name {name}")
    if isinstance(node, Apply):
        if node.func in Parser.BINARY:
            left, right = (evaluate(arg, env) for arg in node.args)
            if node.func == "++":
                return left + right
            return left + ("." if node.func == "<.>" else "/") + right
        func = evaluate(node.func, env)
        args = [evaluate(arg, env) for arg in node.args]
        return _call(func, args)
    raise HaskellError(f"cannot evaluate {node!r}")


@dataclass(frozen=True)
class Partial:
    """A function applied to fewer arguments than it needs."""

    func: Callable
    args: tuple


def _call(func, args):
    if isinstance(func, Partial):
        return _call(func.func, list(func.args) + list(args))
    if not callable(func):
        raise HaskellError(f"{func!r} is not a function")
    arity = func.__code__.co_argcount
    if len(args) < arity:
        return Partial(func, tuple(args))
    result = func(*args[:arity])
    return _call(result, args[arity:]) if len(args) > arity else result


def old_tests_body(source: str) -> str:
    """The text of the list after `tests pandocPath =` up to its `where` clause."""
    match = re.search(r"^tests\s+\w+\s*=\s*$", source, re.M)
    if not match:
        raise HaskellError("no `tests` definition")
    rest = source[match.end():]
    end = re.search(r"^ where\b", rest, re.M)
    return rest[: end.start()] if end else rest


def flatten(tree, prefix: str, out: list) -> None:
    if isinstance(tree, list):
        for item in tree:
            flatten(item, prefix, out)
    elif isinstance(tree, Group):
        flatten(list(tree.children), f"{prefix}/{tree.name}", out)
    elif isinstance(tree, Golden):
        out.append((f"{prefix}/{tree.name}", tree))
    else:
        raise HaskellError(f"unexpected value {tree!r}")


def parse_old(source: str) -> list:
    """Returns (identifier, Golden) pairs for the golden tests of Tests/Old.hs."""
    tokens = tokenize(old_tests_body(source))
    parser = Parser(tokens)
    tree = evaluate(parser.expression(), {})
    if parser.peek() is not None:
        raise HaskellError(f"text after the list at token {parser.peek()!r}")
    pairs = []
    flatten(tree, "old", pairs)
    return unique_ids(pairs)


def unique_ids(pairs: list) -> list:
    seen: dict = {}
    result = []
    for ident, value in pairs:
        seen[ident] = seen.get(ident, 0) + 1
        result.append((ident if seen[ident] == 1 else f"{ident}#{seen[ident]}", value))
    return result


def discover_old(suite: str, found: Discovery) -> None:
    with open(os.path.join(suite, "Tests", "Old.hs"), encoding="utf-8") as handle:
        source = handle.read()
    for ident, golden in parse_old(source):
        args = ("--quiet", golden.input, *golden.args)
        found.cases.append(GoldenCase(ident, "old", args, golden.norm, golden.normalize))


# Reader tests for binary and file-based formats


HS_STRING = r'"(?:[^"\\]|\\.)*"'
TRACK_CHANGES = {"AcceptChanges": "accept", "RejectChanges": "reject", "AllChanges": "all"}


def parse_reader_options(text: str) -> tuple | None:
    """Maps a `def{...}` record of ReaderOptions to (extensions, extra args).

    Returns None for a field that has no command line equivalent.
    """
    extensions = frozenset()
    extra = []
    body = text.strip()[len("def"):].strip().lstrip("{").rstrip("}")
    for field_text in re.split(r",\s*(?=reader)", body):
        key, _, value = (part.strip() for part in field_text.partition("="))
        if key == "readerTrackChanges" and value in TRACK_CHANGES:
            extra.append(f"--track-changes={TRACK_CHANGES[value]}")
        elif key == "readerExtensions":
            names = re.findall(r"Ext_(\w+)", value)
            if not value.startswith("extensionsFromList") or not names:
                return None
            extensions = frozenset(names)
        elif key == "readerStandalone" and value == "True":
            extra.append("--standalone")
        else:
            return None
    return extensions, tuple(extra)


COMPARE = re.compile(
    r"\btestCompare(?:WithOpts\s*(?P<opts>def\s*\{[^}]*\}))?\s+(?P<name>"
    + HS_STRING
    + r")\s+(?P<input>"
    + HS_STRING
    + r")\s+(?P<expected>"
    + HS_STRING
    + r")"
)


def parse_compare_tests(source: str, reader: str, group: str) -> tuple:
    """`testCompare` and `testCompareWithOpts` entries of a reader test module.

    Returns the cases and the number of entries with options that the
    command line cannot express.
    """
    cases = []
    skipped = 0
    for match in COMPARE.finditer(source):
        extensions, extra = None, ()
        if match.group("opts"):
            parsed = parse_reader_options(match.group("opts"))
            if parsed is None:
                skipped += 1
                continue
            extensions, extra = parsed
        name = haskell_string(match.group("name"))
        cases.append(
            AstCase(
                f"{group}/{name}",
                group,
                reader,
                haskell_string(match.group("input")),
                "native",
                haskell_string(match.group("expected")),
                extensions,
                extra,
            )
        )
    pairs = unique_ids([(case.id, case) for case in cases])
    return [replace_id(case, ident) for ident, case in pairs], skipped


def replace_id(case, ident):
    return replace(case, id=ident)


def string_list(source: str, name: str) -> list:
    """The string items of a top-level list `name = [ ... ]`, without commented lines."""
    match = re.search(r"^" + re.escape(name) + r"\s*=\s*\[(.*?)\]", source, re.M | re.S)
    if not match:
        raise HaskellError(f"no list {name}")
    lines = [line for line in match.group(1).split("\n") if not line.strip().startswith("--")]
    return [haskell_string(item) for item in re.findall(HS_STRING, "\n".join(lines))]


# The ODT test lists of Tests/Readers/ODT.hs: (list, reader, input pattern, expected format, expected pattern).
ODT_LISTS = [
    ("namesOfTestsComparingToMarkdown", "odt", "odt/odt/{}.odt", "markdown", "odt/markdown/{}.md"),
    ("namesOfTestsComparingToNative", "odt", "odt/odt/{}.odt", "native", "odt/native/{}.native"),
    ("namesOfFODTTestsComparingToMarkdown", "fodt", "odt/fodt/{}.fodt", "markdown", "odt/markdown/{}.md"),
    ("namesOfFODTTestsComparingToNative", "fodt", "odt/fodt/{}.fodt", "native", "odt/native/{}.native"),
    ("namesOfFODTTestsComparingToOwnNative", "fodt", "odt/fodt/{}.fodt", "native", "odt/fodt-native/{}.native"),
]


def parse_odt(source: str) -> list:
    cases = []
    for list_name, reader, inp, expected_format, expected in ODT_LISTS:
        prefix = "readers/odt/fodt" if reader == "fodt" else "readers/odt"
        for name in string_list(source, list_name):
            cases.append(
                AstCase(
                    f"{prefix}/{name}",
                    "readers/odt",
                    reader,
                    inp.format(name),
                    expected_format,
                    expected.format(name),
                )
            )
    pairs = unique_ids([(case.id, case) for case in cases])
    return [replace_id(case, ident) for ident, case in pairs]


def parse_fb2(source: str) -> list:
    cases = []
    for name, path in re.findall(r"\bfb2Test\s+(" + HS_STRING + r")\s+(" + HS_STRING + ")", source):
        path = haskell_string(path)
        native = os.path.splitext(path)[0] + ".native"
        args = ("-f", "fb2", "-t", "native", "-s", path)
        cases.append(GoldenCase(f"readers/fb2/{haskell_string(name)}", "readers/fb2", args, native))
    return cases


def parse_rtf(source: str) -> list:
    match = re.search(r"map\s+rtfTest\s*\[(.*?)\]", source, re.S)
    if not match:
        raise HaskellError("no rtfTest list")
    cases = []
    for name in (haskell_string(item) for item in re.findall(HS_STRING, match.group(1))):
        args = ("-f", "rtf", "-t", "native", "-s", f"rtf/{name}.rtf")
        cases.append(GoldenCase(f"readers/rtf/{name}", "readers/rtf", args, f"rtf/{name}.native"))
    return cases


# A helper body passes a variable as the name (`testCase name ...`). That
# call is not a test, so a constructor before an identifier does not count.
TEST_MARKERS = re.compile(
    r"=\?>|\b(?:testCase|testProperty|goldenTest|testGolden|goldenVsString"
    r"|docxTest|pptxTests?|odtTest|imageSizeTest)\b(?![ \t]+[a-z_])"
)


SYMBOL = set("!#$%&*+./<=>?@\\^|-~:")


def active_code(source: str) -> str:
    """The source without Haskell comments. Strings and line breaks stay.

    Block comments `{- -}` nest. A line comment starts with two or more
    dashes that are not part of an operator.
    """
    out = []
    pos, depth, size = 0, 0, len(source)
    while pos < size:
        char = source[pos]
        if source.startswith("{-", pos):
            depth += 1
            pos += 2
        elif depth and source.startswith("-}", pos):
            depth -= 1
            pos += 2
        elif depth:
            out.append("\n" if char == "\n" else "")
            pos += 1
        elif char == '"':
            match = re.compile(HS_STRING).match(source, pos)
            end = match.end() if match else size
            out.append(source[pos:end])
            pos = end
        elif char == "'" and (pos == 0 or not (source[pos - 1].isalnum() or source[pos - 1] in "_'")):
            match = re.compile(r"'(?:[^'\\]|\\[^']*)'").match(source, pos)
            end = match.end() if match else pos + 1
            out.append(source[pos:end])
            pos = end
        elif source.startswith("--", pos):
            dashes = pos
            while dashes < size and source[dashes] == "-":
                dashes += 1
            if dashes < size and source[dashes] in SYMBOL:
                out.append(source[pos:dashes])
                pos = dashes
            else:
                while pos < size and source[pos] != "\n":
                    pos += 1
        else:
            out.append(char)
            pos += 1
    return "".join(out)


def count_tests(source: str) -> int:
    """An estimate of the number of tests in a Haskell module.

    Comments do not count. Each `=?>` pair is one test. A line without a
    pair and with a call of a test constructor is one test (`pptxTests`
    makes two). Type declarations (lines with `::`), imports and calls in
    helper bodies (a variable as the name) do not count.
    """
    total = 0
    for line in active_code(source).split("\n"):
        stripped = line.strip()
        if "::" in stripped or stripped.startswith("import"):
            continue
        markers = [match.group(0) for match in TEST_MARKERS.finditer(line)]
        pairs = markers.count("=?>")
        if pairs:
            total += pairs
        elif markers:
            # `pptxTests` makes two tests: without and with a reference document.
            total += 2 if markers[0] == "pptxTests" else 1
    return total


def count_entries(source: str, names: str) -> int:
    """The number of calls of the functions `names` (a regex alternation) as
    list entries or indented lines. A definition starts in column 0 and does
    not count."""
    pattern = r"(?:^[ \t]+|[\[,][ \t]*)(?:" + names + r")\b"
    return len(re.findall(pattern, active_code(source), re.M))


BINARY_READERS = {"Docx.hs": "docx", "Pptx.hs": "pptx", "Xlsx.hs": "xlsx"}

UNSUPPORTED_REASON = {
    "Readers": "the tests give Haskell values (Pandoc builders) that only Haskell can evaluate",
    "Writers": "the tests give Haskell values (Pandoc builders) that only Haskell can evaluate",
    "Writers/Docx.hs": "the tests compare the XML parts of the output archive with a Haskell normalization",
    "Writers/ODT.hs": "the test calls the FODT writer in Haskell and checks the image references in the XML output",
    "Writers/Powerpoint.hs": "the tests compare the XML parts of the output archive with a Haskell normalization",
    "Writers/Native.hs": "QuickCheck properties on random Haskell values",
    "Writers/AnnotatedTable.hs": "QuickCheck properties of an internal Haskell module",
    "Readers/EPUB.hs": "the tests inspect the media bag of the reader, which the command line does not show",
    "Shared.hs": "the tests call internal Haskell functions",
    "ImageSize.hs": "the tests call internal Haskell functions",
    "MediaBag.hs": "the tests call internal Haskell functions",
    "XML.hs": "the tests call internal Haskell functions",
}

# Modules that run tests through the tool, and helper modules with no tests.
HANDLED = {"Command.hs", "Old.hs", "Helpers.hs", "Writers/OOXML.hs", "Readers/Org/Shared.hs", "Readers/Docx.hs", "Readers/Pptx.hs",
           "Readers/Xlsx.hs", "Readers/ODT.hs", "Readers/FB2.hs", "Readers/RTF.hs"}


def haskell_modules(tests_dir: str) -> list:
    found = []
    for root, dirs, files in os.walk(tests_dir):
        dirs[:] = sorted(d for d in dirs if not d.startswith("._"))
        for name in sorted(files):
            if name.endswith(".hs") and not name.startswith("._"):
                found.append(os.path.relpath(os.path.join(root, name), tests_dir).replace(os.sep, "/"))
    return found


def module_group(rel: str) -> str:
    return "hs/" + rel[: -len(".hs")]


def discover_modules(suite: str, found: Discovery) -> None:
    tests_dir = os.path.join(suite, "Tests")

    def read(rel):
        with open(os.path.join(tests_dir, rel), encoding="utf-8") as handle:
            return handle.read()

    for rel in haskell_modules(tests_dir):
        if rel in HANDLED:
            continue
        count = count_tests(read(rel))
        reason = UNSUPPORTED_REASON.get(rel) or UNSUPPORTED_REASON.get(rel.split("/")[0], "not studied")
        if count:
            found.unsupported.append(Unsupported(module_group(rel), count, reason))

    for module, reader in BINARY_READERS.items():
        source = read(f"Readers/{module}")
        cases, skipped = parse_compare_tests(active_code(source), reader, f"readers/{reader}")
        found.cases.extend(cases)
        if skipped:
            found.unsupported.append(Unsupported(f"readers/{reader}", skipped, "reader options with no command line equivalent"))
        others = count_entries(source, "testMediaBag|testForWarningsWithOpts")
        if others:
            found.unsupported.append(Unsupported(
                f"readers/{reader}", others,
                "media bag and warning-list tests inspect reader state that the command line does not show"))
    odt = read("Readers/ODT.hs")
    found.cases.extend(parse_odt(odt))
    media = count_entries(odt, "testMediaBag")
    if media:
        found.unsupported.append(Unsupported("readers/odt", media, "media bag tests inspect reader state that the command line does not show"))
    found.cases.extend(parse_fb2(read("Readers/FB2.hs")))
    found.cases.extend(parse_rtf(read("Readers/RTF.hs")))


def discover(suite: str) -> Discovery:
    found = Discovery()
    discover_commands(suite, found)
    discover_old(suite, found)
    discover_modules(suite, found)
    return found
