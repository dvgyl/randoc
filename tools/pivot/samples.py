"""Supply the fixed forms, highlighting samples and formulas of the pivot set."""

import json
import tomllib


EMPTY_ATTR = ["", [], []]
PIXEL = (
    "data:image/png;base64,"
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGOwmLL1PwAFCgKBOCFgpAAAAABJRU5ErkJggg=="
)
MATH_ENVIRONMENTS = ("matrix", "pmatrix", "bmatrix", "array", "split", "aligned",
                     "cases", "gathered", "vmatrix", "Vmatrix")
INVALID_MATH = (
    r"\frac{r_3}{", r"\sqrt[5]{v", r"l^{t", r"\surveyMissingSymbol{3}",
    r"\begin{matrix}r&4\end{pmatrix}", r"\mathcal{T", r"\hat{z",
    r"\text{survey", r"\sqrt[9", r"\frac{r_1}", r"\begin{array}{lc}r&5",
    r"\begin{cases}s&3\end{array}", r"\underline{q", r"\vec{uv",
    r"\binom{7}", r"\overset{4}", r"\sum_{k=4", r"\operatorname{count",
    r"\left(q\right", r"\begin{aligned}n&=4\end{gathered}",
)


def node(tag, contents=None):
    return {"t": tag} if contents is None else {"t": tag, "c": contents}


def inline_text(text):
    return [node("Str", text)]


def plain(text):
    return node("Plain", inline_text(text))


def one(block):
    return {"meta": {}, "blocks": [block]}


def table_forms():
    alignment = node("AlignDefault")

    def cell(text, rows=1, columns=1):
        return [EMPTY_ATTR, alignment, rows, columns, [plain(text)]]

    def row(*cells):
        return [EMPTY_ATTR, list(cells)]

    regular = row(cell("East"), cell("West"))
    table_head = row(cell("Day"), cell("Night"))
    table_foot = row(cell("Total"), cell("Count"))
    units = [[alignment, node("ColWidthDefault")]] * 2
    specified = [[node("AlignLeft"), node("ColWidth", 0.4)],
                 [node("AlignRight"), node("ColWidth", 0.6)]]
    for name, columns, head, rows, foot, short in (
            ("column-width", specified, [], [regular], [], None),
            ("short-caption", units, [], [regular], [], inline_text("Short")),
            ("head-and-foot", units, [table_head], [regular], [table_foot], None),
            ("column-span", units, [], [row(cell("Both", columns=2))], [], None),
            ("row-span", units, [], [row(cell("Two", rows=2), cell("First")), row(cell("Second"))], [], None),
    ):
        body = [EMPTY_ATTR, 0, [], rows]
        yield name, node("Table", [EMPTY_ATTR, [short, [plain("Survey")]], columns,
                                   [EMPTY_ATTR, head], [body], [EMPTY_ATTR, foot]])
    body_rows = [regular]
    parts = [
        ("row-head", [[EMPTY_ATTR, 1, [], body_rows]]),
        ("body-head", [[EMPTY_ATTR, 0, [table_head], body_rows]]),
        ("two-bodies", [[EMPTY_ATTR, 0, [], body_rows], [EMPTY_ATTR, 0, [], [table_foot]]]),
    ]
    for name, bodies in parts:
        yield name, node("Table", [EMPTY_ATTR, [None, []], units,
                                   [EMPTY_ATTR, []], bodies, [EMPTY_ATTR, []]])


def forms(root):
    first = root / "conformance/fixtures/files/sample-cell.json"
    yield "forms/sample-cell", json.loads(first.read_text(encoding="utf-8"))
    source = root / "conformance/cases/json/nodes.toml"
    for case in tomllib.loads(source.read_text(encoding="utf-8"))["case"]:
        name = case["id"]
        if name.startswith(("inline-", "block-", "meta-")):
            tree = json.loads(case["input"])
            if name == "inline-image":
                tree["blocks"][0]["c"][0]["c"][2][0] = PIXEL
            yield f"forms/{name}", tree
    for name, table in table_forms():
        yield f"forms/table-{name}", one(table)
    for name, attrs in (
            ("id", ["shift-report", [], []]),
            ("classes", ["", ["notice", "selected"], []]),
            ("value", ["", [], [["data-label", '<day & "night">']]]),
            ("combined", ["colour", ["panel"], [["lang", "fr"], ["title", "Réserve"]]]),
    ):
        yield f"forms/attributes-{name}", one(node("Div", [attrs, [plain("Reading")]]))


def highlight_sample(language):
    if language in {"python", "ruby", "bash", "perl", "r", "zsh", "fish"}:
        lines = [f"# Observation {number}\nreading_{number} = '{number * 4}.5 m'"
                 for number in range(1, 9)]
    elif language in {"json", "yaml", "toml", "ini", "xml", "html", "css", "markdown"}:
        patterns = {"json": '  "reading_{0}": {1}', "yaml": 'reading_{0}: {1}',
                    "toml": 'reading_{0} = {1}', "ini": 'reading_{0}={1}',
                    "xml": '  <reading id="r{0}">{1}</reading>',
                    "html": '  <p id="r{0}">{1}</p>',
                    "css": '.reading-{0} {{ width: {1}px; }}',
                    "markdown": '* **Reading {0}** is `{1}`.'}
        lines = [patterns[language].format(number, number * 4) for number in range(1, 16)]
        if language == "json":
            lines = ["{", ",\n".join(lines), "}"]
        if language == "xml":
            lines = ["<survey>", *lines, "</survey>"]
    else:
        lines = [
            "// Survey readings",
            "/* Values are in metres. */",
            '#define SCALE 4',
            '-- Use the north marker.',
            'module Survey;',
            'const scale = 4;',
            'let reading = 52.5;',
            'let label = "north";',
            'let mark = \'N\';',
            'if (reading >= 50) {',
            '    total += reading * scale;',
            '} else {',
            '    total = 0;',
            '}',
            'return [label, total];',
        ]
    return "\n".join(lines)


def highlighting(root):
    inventory = root / "spec/inventory/highlight-languages.json"
    for language in json.loads(inventory.read_text(encoding="utf-8")):
        attr = [f"sample-{language}", [language], []]
        yield f"highlight/{language}", one(node("CodeBlock", [attr, highlight_sample(language)]))


def math_values(kind, expressions):
    return {"meta": {}, "blocks": [node("Para", [node("Math", [node(kind), expr])])
                                  for expr in expressions]}


def math(root):
    source = root / "crates/randoc-math/tools/katex-names.txt"
    names = [line for line in source.read_text(encoding="utf-8").splitlines()
             if line and not line.startswith("#")]
    for index in range(10):
        expressions = [f"\\{name}{{r}}{{s}}" for name in names[index::10]]
        for kind in ("InlineMath", "DisplayMath"):
            yield f"math/commands-{index + 1:02d}-{kind}", math_values(kind, expressions)
    for environment in MATH_ENVIRONMENTS:
        columns = "{lc}" if environment == "array" else ""
        expression = rf"\begin{{{environment}}}{columns}r&5\\s&7\end{{{environment}}}"
        for kind in ("InlineMath", "DisplayMath"):
            yield f"math/{environment}-{kind}", math_values(kind, [expression])
    for index, expression in enumerate(INVALID_MATH, start=1):
        kind = "InlineMath" if index % 2 else "DisplayMath"
        yield f"math/invalid-{index:02d}-{kind}", math_values(kind, [expression])


def fixed_trees(root):
    yield from forms(root)
    yield from highlighting(root)
    yield from math(root)
