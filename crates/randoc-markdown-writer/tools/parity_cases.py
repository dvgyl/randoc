#!/usr/bin/env python3
"""Oracle cases of the Wave 9 parity fixes of the Markdown writer.

Each case is an original input (native, Markdown or LaTeX text), a format
and options. `record` runs the oracle: the input to JSON, then the JSON to
the format. It writes `tests/parity_cases.jsonl`, one object per line with
`name`, `format`, `opts`, `doc` (the JSON tree), `expected` (standard
output) and `warnings` (the first line of each warning). The test `tests/parity.rs` replays the file.

Usage:
  python3 tools/parity_cases.py record
"""
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "tests", "parity_cases.jsonl")
P = os.environ.get("RANDOC_ORACLE_PANDOC", "pandoc")

# The command-line form of each option of `tests/parity.rs`.
FLAGS = {
    "columns": lambda v: [f"--columns={v}"],
    "wrap": lambda v: [f"--wrap={v}"],
    "tab_stop": lambda v: [f"--tab-stop={v}"],
    "refloc": lambda v: [f"--reference-location={v}"],
    "ascii": lambda v: ["--ascii"] if v else [],
    "toc": lambda v: ["--toc"] if v else [],
    "standalone": lambda v: ["-s"] if v else [],
    "setext": lambda v: ["--markdown-headings=setext"] if v else [],
    "reference_links": lambda v: ["--reference-links"] if v else [],
}

CASES = []


def case(name, src, fmt, frm="native", **opts):
    CASES.append((name, src, fmt, frm, opts))


def para(*words):
    """A paragraph of words separated by spaces."""
    inlines = ", Space, ".join(f'Str "{w}"' for w in words)
    return f"Para [{inlines}]"


# MW-1-1: the reader accepts a roman numeral only in its grammar, and needs
# two spaces after an upper-case numeral with a period whose value is that
# of one letter.
for marker in ["IV.", "IIIII.", "IVI.", "XCXX.", "ID.", "XM.", "IC.", "VV.", "LL.", "il.",
               "iiiii.", "CDC.", "CDC)", "V.", "V)", "I.", "MMMMM.", "Iv."]:
    case(f"MW-1-1 line start {marker}", f'[Para [Str "{marker}", Space, Str "rest"]]', "markdown")
    case(f"MW-1-1 list wrap {marker}",
         f'[BulletList [[Plain [Str "wwwwwwwwwwwwwwwwwww", Space, Str "{marker}", Space, Str "end"]]]]',
         "markdown", columns=20)
case("MW-1-1 a definition breaks before ID.",
     '[DefinitionList [([Code ("",[],[]) "halt", Space, Str "ID"], [['
     + para("Stops", "a", "pending", "upload,", "fetch", "or", "copy", "of", "the", "volume",
            "with", "the", "specified", "ID.", "Use", "the", "jobs", "command", "to", "list",
            "running", "transfers.")
     + "]])]]", "markdown")

# MW-1-2: `four_space_rule` puts definitions at the tab stop.
DEFS = ('[DefinitionList [([Str "Term"], [[CodeBlock ("",[],[]) "block\\n\\n  : not a term"]]),'
        ' ([Str "Other"], [[' + para("first", "line") + ', CodeBlock ("",[],[]) "code"]])]]')
for tab in (4, 8):
    case(f"MW-1-2 four space rule tab {tab}", DEFS, "markdown+four_space_rule", tab_stop=tab)
case("MW-1-2 no four space rule", DEFS, "markdown")

# MW-1-3: a plain block before a fenced div in a list item.
ITEM = ('[OrderedList (1,Decimal,Period) [[Plain [Str "Intro"], Div ("",["box"],[]) '
        '[Plain [Str "inside"]]], [Plain [Str "Next"], BlockQuote [Para [Str "quote"]]]]]')
for fmt in ("markdown", "gfm"):
    case(f"MW-1-3 plain before a div {fmt}", ITEM, fmt)

# MW-1-4: Project Gutenberg conventions.
case("MW-1-4 gutenberg headings",
     "# Opening *words*\n\nSome **bold** text.\n\n## Part two\n\nMore text.\n\n"
     "### Detail\n\nEnd.\n\n# Close\n", "plain+gutenberg", frm="markdown")
case("MW-1-4 gutenberg notes", "# Title^[A note with *emphasis*.]\n\nText.\n",
     "plain+gutenberg", frm="markdown")

# MW-1-5: only a line of fence characters alone counts for the fence.
for i, code in enumerate(["call ```` here", "````", "  ````  ", "one\\n  ```   \\ntwo",
                          "`` ````", "````rest"]):
    case(f"MW-1-5 fence {i}", f'[CodeBlock ("",["lang"],[]) "{code}"]', "markdown")

# MW-1-6: an ATX heading in literate Haskell is a paragraph.
for level in (1, 3):
    case(f"MW-1-6 lhs heading level {level}",
         f'[Header {level} ("",[],[]) [Str "Setup", Space, Emph [Str "now"]], Para [Str "text"]]',
         "markdown+lhs")
case("MW-1-6 lhs setext heading", '[Header 2 ("",[],[]) [Str "Setup"]]', "markdown+lhs",
     setext=True)

# MW-1-7: a notebook cell does not give its identifier to its heading.
case("MW-1-7 toc and cells",
     "::: {.cell .markdown #c1}\n# Overview\n\ntext\n:::\n\n::: {.other #o1}\n# Other\n:::\n\n"
     "# Later\n\n## Inner\n", "markdown", frm="markdown", toc=True, standalone=True)

# MW-1-8: GitHub alerts.
TITLE = 'Div ("",["title"],[]) [Para [Str "Heads"]]'
ALERTS = {
    "two blocks": f'Div ("",["warning"],[]) [{TITLE}, Para [Str "one"], Para [Str "two"]]',
    "no title": 'Div ("",["tip"],[]) [Para [Str "plain"]]',
    "id and class": f'Div ("a1",["note","extra"],[]) [{TITLE}, Plain [Str "x"], Plain [Str "y"]]',
    "title attributes": 'Div ("",["note"],[]) [Div ("t",["title"],[]) [], Para [Str "kept"]]',
    "second class": f'Div ("",["extra","note"],[]) [{TITLE}, Para [Str "kept"]]',
    "nested": f'Div ("",["tip"],[]) [{TITLE}, Div ("",["caution"],[]) [{TITLE}, Para [Str "in"]]]',
    "list": f'Div ("",["important"],[]) [{TITLE}, BulletList [[Plain [Str "a"]],[Plain [Str "b"]]]]',
}
for name, div in ALERTS.items():
    for fmt in ("gfm", "commonmark", "commonmark_x"):
        case(f"MW-1-8 alert {name} {fmt}", f"[{div}]", fmt)

# MW-1-9: task markers.
TASKS = ('[BulletList [[Plain [Str "\\10063", Space, Str "open"]], [Plain [Str "\\10003", Space, Str "done"]],'
         ' [Plain [Str "\\9744"]], [Plain [Str "\\9746"], Para [Str "more"]],'
         ' [Plain [Str "\\9745", Space, Str "text"]]]]')
for fmt in ("gfm", "markdown"):
    case(f"MW-1-9 task markers {fmt}", TASKS, fmt)
case("MW-1-9 task marker wrap",
     '[BulletList [[Plain [Str "\\9744", Space, Str "verylongunbrokenword", Space, Str "x"]]]]',
     "gfm", columns=12)

# MW-1-10: the div of a highlighted code block.
SOURCE = 'Div ("cb2",["sourceCode"],[]) [CodeBlock ("",["sourceCode","python"],[]) "print(1)"]'
for fmt in ("gfm", "markdown", "commonmark"):
    case(f"MW-1-10 source code div {fmt}", f"[{SOURCE}]", fmt)
case("MW-1-10 source code div with two blocks",
     '[Div ("",["sourceCode"],[]) [CodeBlock ("",["sourceCode"],[]) "x", Para [Str "p"]]]', "gfm")

# MW-1-11: blank lines in raw HTML for CommonMark.
for i, raw in enumerate(['<p title=\\"a\\n  \\n\\nb\\">x</p>', "<div>\\n\\n\\nend</div>",
                         "<i>\\n\\t\\n\\160\\nz</i>"]):
    case(f"MW-1-11 raw html {i}", f'[RawBlock (Format "html") "{raw}", Para [Str "after"]]',
         "commonmark")
case("MW-1-11 html table",
     '<table><tr><td title="m\n  \n\nn"><p>a</p><p>b</p></td></tr></table>', "commonmark",
     frm="html")

# MW-2-1: a note inside a note takes its number after the notes of the flush.
NOTES = ('[Header 1 ("s1",[],[]) [Str "One"], Para [Str "start", Note [Para [Str "first", Space, Str "note",'
         ' Note [Para [Str "deeper"]]]], Space, Str "more", Note [Para [Str "next"]]], Header 1 ("s2",[],[]) [Str "Two"],'
         ' Para [Str "later", Note [Para [Str "last"]]]]')
for loc in ("document", "section", "block"):
    case(f"MW-2-1 nested notes {loc}", NOTES, "markdown", refloc=loc)
case("MW-2-1 nested notes plain", NOTES, "plain")
case("MW-2-1 note in a heading",
     '[Header 2 ("",[],[]) [Str "Head", Note [Para [Str "one", LineBreak, Str "two"]]]]', "markdown")
case("MW-2-1 note in small caps",
     '[Para [SmallCaps [Str "caps", Note [Para [Str "lower", Note [Para [Str "deeper"]]]]]]]', "plain")
case("MW-2-1 note in a plain image",
     '[Para [Image ("",[],[]) [Str "pic", Note [Para [Str "about"]]] ("p.png",""), Note [Para [Str "last"]]]]',
     "plain")
case("MW-2-1 note links",
     '[Para [Link ("",[],[]) [Str "first"] ("http://a.example",""), Note [Para [Link ("",[],[]) [Str "inner"] ("http://b.example","")]]]]',
     "markdown", reference_links=True)

# MW-2-2: a link whose text is its URL before URI escaping is an autolink.
LINKS = ('[Para [Link ("",[],[]) [Str "File:Map.png|Old map"] ("File:Map.png%7COld%20map",""), Space,'
         ' Link ("",[],[]) [Str "https://example.org/a b"] ("https://example.org/a%20b",""), Space,'
         ' Link ("",[],[]) [Str "https://example.org/x^y"] ("https://example.org/x%5Ey","")]]')
for fmt in ("markdown", "gfm", "plain"):
    case(f"MW-2-2 escaped autolinks {fmt}", LINKS, fmt)

# MW-2-3: empty classes.
case("MW-2-3 empty classes",
     '[Para [Link ("",["","",""],[]) [Str "site"] ("https://example.org",""), Space,'
     ' Span ("",["","c"],[]) [Str "s"]]]', "markdown")

# MW-2-4: quotes with `--ascii`.
QUOTES = '[Para [Quoted DoubleQuote [Str "say", Space, Quoted SingleQuote [Str "hi"]], Str "\\8217s", Space, Str "caf\\233"]]'
for fmt in ("markdown-smart", "commonmark", "plain", "markua", "markdown"):
    case(f"MW-2-4 ascii quotes {fmt}", QUOTES, fmt, ascii=True)

# MW-2-5: superscripts and subscripts in Unicode characters.
SCRIPTS = ('[Para [Str "a", Superscript [Str "4", Space, Str "5"], Space, Str "b", Superscript [Span ("",["k"],[]) [Str "6"]],'
           ' Space, Str "c", Superscript [Emph [Str "7"], Space, Str "8"], Space, Str "d", Subscript [Str "9", Space, Str "0"],'
           ' Space, Str "e", Superscript [Str "x"]]]')
for fmt in ("commonmark-raw_html", "markdown-superscript-subscript-raw_html", "plain"):
    case(f"MW-2-5 scripts {fmt}", SCRIPTS, fmt)
case("MW-2-5 plain subscript of other inlines",
     '[Para [Str "x", Subscript [Emph [Str "1"], Code ("",[],[]) "2"], Str "y"]]', "plain")
case("MW-2-5 math scripts", "$\\beta^{1+4} \\cdot \\gamma_{2-3}$", "plain", frm="latex")
case("MW-2-5 soft break in a script",
     '[Para [Str "x", Superscript [Str "1", SoftBreak, Str "2"], Str "y"]]', "markdown",
     wrap="preserve")

# MW-2-6: the top line of a grid table with spans.
case("MW-2-6 span grid top line",
     "<table><tr><td colspan=\"2\">P</td><td colspan=\"2\">Q</td><td rowspan=\"2\">R</td></tr>"
     "<tr><td>S</td><td colspan=\"3\">T</td></tr><tr><td colspan=\"5\">U</td></tr></table>",
     "markdown", frm="html")
case("MW-2-6 span grid aligned top line",
     "<table><tr><td colspan=\"2\" style=\"text-align:right\">P</td><td>Q</td></tr>"
     "<tr><td colspan=\"3\">R</td></tr></table>", "markdown", frm="html")

# MW-2-7: default widths beside explicit widths.
WIDTHS = ('<table><colgroup><col width="10%"><col><col></colgroup><tr><td><p>x</p><p>y</p></td>'
          '<td>words of a long cell that needs more than one line in the table to fit the text of'
          ' the cell</td><td>short cell</td></tr></table>')
for columns in (72, 40):
    case(f"MW-2-7 widths {columns}", WIDTHS, "markdown", frm="html", columns=columns)


def oracle(args, data):
    out = subprocess.run([P, *args], input=data, capture_output=True, timeout=60)
    if out.returncode != 0:
        sys.exit(f"oracle failed: {args}: {out.stderr.decode()}")
    return out.stdout.decode(), out.stderr.decode()


def record():
    with open(DATA, "w", encoding="utf-8") as fh:
        for name, src, fmt, frm, opts in CASES:
            tree, _ = oracle(["-f", frm, "-t", "json"], src.encode())
            args = [flag for key, value in opts.items() for flag in FLAGS[key](value)]
            expected, stderr = oracle(["-f", "json", "-t", fmt, *args], tree.encode())
            # The first line of a warning names it.
            warnings = "".join(line + "\n" for line in stderr.splitlines() if line.startswith("["))
            row = {"name": name, "format": fmt, "opts": opts, "doc": tree.strip(),
                   "expected": expected, "warnings": warnings}
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(f"{len(CASES)} cases")


if __name__ == "__main__":
    if sys.argv[1:] != ["record"]:
        sys.exit(__doc__)
    record()
