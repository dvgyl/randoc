#!/usr/bin/env python3
"""Differential test of the Randoc HTML reader against Pandoc 3.13.

The tool runs both readers on the same inputs and prints the difference
classes by signature. A signature is the path of element names to the
first difference of the two document trees.

Usage (from the root of the worktree, after `. env.sh`):

  cargo build -p randoc-html-reader --examples
  tools/differential.py cases FILE...      compare the cases of case files
  tools/differential.py random SEED COUNT  HTML from a random grammar
  tools/differential.py mutate SEED COUNT  changed versions of the pages below
  tools/differential.py toml FILE...       the cases of conformance case files
  tools/differential.py sweep SEED COUNT   each element and attribute in each
                                           container, well formed and broken,
                                           with each extension on and off

A case file holds cases that a line `%%%%` separates. A first line
`@FORMAT` gives the format, for example `@html+raw_html`.

The oracle runs in one process for each batch of 400 inputs (`pandoc lua`),
with a time limit of 60 seconds. Set RANDOC_ORACLE_PANDOC to the Pandoc 3.13 program.
Use `-v` to print each different case.
"""

import json
import os
import random
import re
import subprocess
import sys
import tempfile

SEPARATOR = "\n%%%%\n"

LUA = r"""
local f = assert(io.open(arg[1], "r"))
local data = f:read("a")
f:close()
local pos = 1
local sep = "\n%%%%\n"
local n = 0
while true do
  local s, e = data:find(sep, pos, true)
  local chunk = data:sub(pos, s and (s - 1) or #data)
  local fmt = "html"
  local m = chunk:match("^@([^\n]*)\n")
  if m then fmt = m; chunk = chunk:sub(#m + 3) end
  -- The program ends each input with a newline and adds one more.
  if chunk ~= "" then
    if chunk:sub(-1) ~= "\n" then chunk = chunk .. "\n" end
    chunk = chunk .. "\n"
  end
  n = n + 1
  local msgs = {}
  local ok, res = pcall(function()
    local doc
    msgs = pandoc.log.silence(function() doc = pandoc.read(chunk, fmt) end)
    return pandoc.write(doc, "json")
  end)
  io.write("--- " .. n .. "\n")
  if ok then io.write(res, "\n") else io.write("ERROR: " .. tostring(res):gsub("\n", " ") .. "\n") end
  for i = #msgs, 1, -1 do
    local j = pandoc.json.decode(pandoc.json.encode(msgs[i]), false)
    io.write("LOG: " .. j.verbosity .. " " .. j.pretty:gsub("\n", "\\n") .. "\n")
  end
  if not s then break end
  pos = e + 1
end
"""

# Pages for the mutation mode. Each one is written for this tool.
PAGES = [
    """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="author" content="A. Writer">
<title>A small page</title>
</head>
<body>
<h1 id="top">A small page</h1>
<p>The first paragraph has <em>emphasis</em>, <strong>strong text</strong>,
a <a href="other.html" title="Other">link</a> and <code>code</code>.</p>
<ul>
<li>One</li>
<li>Two <b>bold</b>
<ul><li>Nested</li></ul></li>
</ul>
<blockquote><p>A quotation.</p></blockquote>
<pre><code class="language-c">int main(void) { return 0; }
</code></pre>
<table>
<caption>Numbers</caption>
<thead><tr><th>Name</th><th style="text-align: right;">Value</th></tr></thead>
<tbody><tr><td>one</td><td style="text-align: right;">1</td></tr>
<tr><td>two</td><td style="text-align: right;">2</td></tr></tbody>
</table>
<hr>
<p>Last line<br>with a break and an image <img src="a.png" alt="An image">.</p>
</body>
</html>
""",
    """<div class="note" id="n1">
<h2>Section <span class="tag">one</span></h2>
<p>Text with a note<a href="#fn1" class="footnote-ref" id="fnref1" role="doc-noteref"><sup>1</sup></a>.</p>
<dl><dt>Term</dt><dd>Definition</dd><dt>Other</dt><dd><p>Second</p></dd></dl>
<ol start="3" type="a"><li>Three</li><li>Four</li></ol>
<figure><img src="f.png" alt="alt"><figcaption>Caption text</figcaption></figure>
</div>
<section id="footnotes" class="footnotes footnotes-end-of-document" role="doc-endnotes">
<hr />
<ol>
<li id="fn1"><p>The note.<a href="#fnref1" class="footnote-back" role="doc-backlink">back</a></p></li>
</ol>
</section>
""",
]

BLOCKS = ["p", "div", "blockquote", "ul", "ol", "li", "h1", "h2", "h3", "pre", "table", "tr",
          "td", "th", "dl", "dt", "dd", "section", "figure", "figcaption", "hr", "thead",
          "tbody", "caption", "aside", "header", "main", "nav", "article", "details",
          "summary", "form", "center", "address", "fieldset", "footer", "tfoot"]
INLINES = ["b", "i", "em", "strong", "a", "span", "code", "sub", "sup", "u", "s", "q", "br",
           "img", "small", "kbd", "del", "ins", "mark", "abbr", "cite", "big", "label",
           "tt", "var", "samp", "dfn", "strike", "bdo", "font", "button", "select", "option"]
OTHERS = ["script", "style", "title", "meta", "link", "head", "body", "html", "input", "svg",
          "video", "audio", "object", "noscript", "textarea", "col", "colgroup", "custom",
          "x:y", "iframe", "canvas", "legend", "math"]
ATTRIBUTES = ['id="i"', 'class="c d"', 'href="u"', 'src="s.png"', 'title="t"', 'lang="en"',
              'data-k="v"', 'style="text-align: left"', 'align="right"', 'colspan="2"',
              'rowspan="2"', 'width="50%"', 'start="4"', 'type="A"', 'alt="a b"', 'name="n"',
              'role="note"', 'dir="rtl"', 'hidden', 'X="1"']
WORDS = ["alpha", "beta", " ", "\n", "&amp;", "&copy", "x < y", "1 &lt; 2", " gamma ", "<!-- c -->",
         "&#65;", "a\tb", "delta."]


def random_element(rng, depth):
    """Returns the text of one random element. The grammar is well formed."""
    if depth <= 0 or rng.random() < 0.3:
        return rng.choice(WORDS)
    kind = rng.random()
    names = BLOCKS if kind < 0.45 else INLINES if kind < 0.9 else OTHERS
    name = rng.choice(names)
    attributes = "".join(" " + rng.choice(ATTRIBUTES) for _ in range(rng.choice([0, 0, 0, 1, 2])))
    if name in ("br", "hr", "img", "input", "meta", "link", "col"):
        return "<%s%s>" % (name, attributes)
    children = "".join(random_element(rng, depth - 1) for _ in range(rng.choice([0, 1, 1, 2, 3])))
    return "<%s%s>%s</%s>" % (name, attributes, children, name)


def random_structure(rng):
    """Returns a random document of nested structures that authors write."""
    def inline(depth):
        if depth <= 0 or rng.random() < 0.4:
            return rng.choice(["word", "two words", "a &amp; b", " spaced ", "x"])
        name = rng.choice(["b", "i", "em", "strong", "code", "span", "a", "sub", "sup", "u", "s", "q"])
        attributes = ' href="u"' if name == "a" else rng.choice(["", "", ' class="k"', ' id="x"'])
        return "<%s%s>%s</%s>" % (name, attributes, inlines(depth - 1), name)

    def inlines(depth):
        return "".join(inline(depth) + rng.choice(["", " ", "\n"]) for _ in range(rng.choice([1, 2, 3])))

    def block(depth):
        choice = rng.randrange(12) if depth > 0 else rng.randrange(3)
        if choice == 0:
            return "<p>%s</p>" % inlines(2)
        if choice == 1:
            level = rng.randrange(1, 7)
            return "<h%d>%s</h%d>" % (level, inlines(1), level)
        if choice == 2:
            return inlines(2)
        if choice == 3:
            return "<div%s>%s</div>" % (rng.choice(["", ' class="c"', ' id="d"']), blocks(depth - 1))
        if choice == 4:
            return "<blockquote>%s</blockquote>" % blocks(depth - 1)
        if choice in (5, 6):
            tag = rng.choice(["ul", "ol"])
            items = "".join("<li>%s</li>" % rng.choice([inlines(1), blocks(depth - 1)])
                            for _ in range(rng.choice([1, 2, 3])))
            return "<%s>%s</%s>" % (tag, items, tag)
        if choice == 7:
            items = "".join("<dt>%s</dt><dd>%s</dd>" % (inlines(1), rng.choice([inlines(1), blocks(depth - 1)]))
                            for _ in range(rng.choice([1, 2])))
            return "<dl>%s</dl>" % items
        if choice == 8:
            rows = "".join("<tr>%s</tr>" % "".join(
                "<%s>%s</%s>" % (cell, rng.choice([inlines(1), blocks(depth - 1)]), cell)
                for cell in [rng.choice(["td", "td", "th"]) for _ in range(rng.choice([1, 2, 3]))])
                for _ in range(rng.choice([1, 2, 3])))
            return "<table>%s</table>" % rows
        if choice == 9:
            return "<pre><code>%s</code></pre>" % rng.choice(["x = 1\n", "a\n  b\n", "&lt;p&gt;"])
        if choice == 10:
            return "<figure>%s<figcaption>%s</figcaption></figure>" % (blocks(depth - 1), inlines(1))
        return "<hr>"

    def blocks(depth):
        return "".join(block(depth) + rng.choice(["", "\n"]) for _ in range(rng.choice([1, 2, 3])))

    return blocks(3)


# The sweep: element forms, attribute sets, containers and breakages.
SWEEP_ELEMENTS = [
    "p", "div", "section", "article", "aside", "header", "footer", "main", "nav", "blockquote",
    "h1", "h3", "h6", "pre", "address", "center", "figure", "details", "summary", "form",
    "fieldset", "ul", "ol", "dl", "table", "hr", "br", "b", "i", "em", "strong", "u", "s",
    "del", "ins", "strike", "sub", "sup", "small", "big", "mark", "kbd", "dfn", "abbr", "cite",
    "code", "tt", "samp", "var", "q", "a", "span", "img", "bdo", "font", "label", "button",
    "select", "option", "textarea", "input", "video", "audio", "source", "object", "embed",
    "iframe", "canvas", "svg", "math", "script", "style", "noscript", "template", "title",
    "meta", "link", "base", "head", "body", "html", "x-custom", "wbr", "time", "data", "ruby",
]
SWEEP_ATTRIBUTES = [
    "", 'id="i"', 'class="c d"', 'lang="de"', 'dir="rtl"', 'style="text-align: right"',
    'data-k="v"', 'epub:type="footnote"', 'role="doc-noteref"', 'title="t"', 'href="u"',
    'src="s.png"', 'alt="a"', 'width="50%"', 'start="3"', 'type="a"', 'colspan="2"',
    'id="i" class="line-block"', 'class="line-block"', 'role="main"', 'xml:lang="fr"', 'cite="u"',
]
SWEEP_CONTENT = ["x", "a b", "x<br>y", " \"q\" ", "$m$ \\(n\\)", "\\ref{r}", "a -- b...", "&amp;&#128;"]
SWEEP_CONTAINERS = [
    "%s", "<p>%s</p>", "<div>%s</div>", "<li>%s</li>", "<ul><li>%s</li></ul>", "<ol><li>%s</li></ol>",
    "<dl><dt>t</dt><dd>%s</dd></dl>", "<blockquote>%s</blockquote>",
    "<table><tr><td>%s</td></tr></table>", "<table><caption>%s</caption><tr><td>c</td></tr></table>",
    "<h2>%s</h2>", "<figure>%s<figcaption>f</figcaption></figure>", "<b>%s</b>", "<a href=\"u\">%s</a>",
    "<q>%s</q>", "<pre>%s</pre>", "<head>%s</head><p>b</p>", "<section role=\"doc-endnotes\"><ol><li id=\"n\">%s</li></ol></section>",
]
SWEEP_EXTENSIONS = [
    "", "+raw_html", "-native_divs", "-native_spans", "-line_blocks", "+smart", "+raw_tex",
    "+tex_math_dollars", "+tex_math_single_backslash", "+tex_math_double_backslash",
    "+epub_html_exts", "+task_lists", "+empty_paragraphs", "-auto_identifiers",
    "+gfm_auto_identifiers", "+ascii_identifiers", "+east_asian_line_breaks",
]


def sweep_element(rng, name, attributes, content):
    """Returns one element in a well formed or a broken form."""
    open_tag = "<%s%s>" % (name, " " + attributes if attributes else "")
    form = rng.randrange(6)
    if form == 0:
        return open_tag + content
    if form == 1:
        return open_tag + content + "</%s></%s>" % (name, name)
    if form == 2:
        return "<b>" + open_tag + content + "</b></%s>" % name
    if form == 3:
        return content + "</%s>" % name
    return open_tag + content + "</%s>" % name


def sweep(rng, count):
    """Returns `count` documents: each element with each attribute set, in
    each container, with each extension, in a well formed or broken form."""
    cases = []
    while len(cases) < count:
        for name in SWEEP_ELEMENTS:
            attributes = rng.choice(SWEEP_ATTRIBUTES)
            element = sweep_element(rng, name, attributes, rng.choice(SWEEP_CONTENT))
            container = rng.choice(SWEEP_CONTAINERS)
            extension = rng.choice(SWEEP_EXTENSIONS)
            prefix = "@html%s\n" % extension if extension else ""
            cases.append(prefix + container % element)
    return cases[:count]


def mutate(rng, page):
    """Returns `page` with a few random changes: a removed, doubled or moved tag or text."""
    parts = re.findall(r"<[^>]*>|[^<]+", page)
    for _ in range(rng.choice([1, 1, 2, 3])):
        if not parts:
            break
        index = rng.randrange(len(parts))
        action = rng.randrange(5)
        if action == 0:
            del parts[index]
        elif action == 1:
            parts.insert(index, parts[index])
        elif action == 2:
            parts.insert(rng.randrange(len(parts)), parts.pop(index))
        elif action == 3:
            parts[index] = parts[index].upper()
        else:
            parts.insert(index, rng.choice(["<b>", "</b>", "<p>", "</p>", "<div>", "</div>", "<li>",
                                            "</td>", "<!--", "&", "<", "</ul>", "<span>", "<br>"]))
    return "".join(parts)


def run(command, path):
    """Runs one program on a case file and returns the list of (document, messages)."""
    output = subprocess.run(command + [path], capture_output=True, timeout=60, check=False)
    text = output.stdout.decode("utf-8", "replace")
    if output.returncode != 0 and not text:
        sys.exit("%s failed: %s" % (command[0], output.stderr.decode("utf-8", "replace")[:500]))
    results = []
    for chunk in re.split(r"^--- \d+\n", text, flags=re.M)[1:]:
        lines = chunk.split("\n")
        logs = sorted(line for line in lines if line.startswith("LOG: "))
        body = "\n".join(line for line in lines if not line.startswith("LOG: ")).strip()
        results.append((body, logs))
    return results


def first_difference(a, b, path):
    """Returns the signature of the first difference of two JSON values, or None."""
    if isinstance(a, dict) and isinstance(b, dict) and "t" in a and "t" in b:
        if a["t"] != b["t"]:
            return "%s: %s != %s" % ("/".join(path), a["t"], b["t"])
        return first_difference(a.get("c"), b.get("c"), path + [a["t"]])
    if isinstance(a, dict) and isinstance(b, dict):
        for key in sorted(set(a) | set(b)):
            found = first_difference(a.get(key), b.get(key), path + [key])
            if found:
                return found
        return None
    if isinstance(a, list) and isinstance(b, list):
        for x, y in zip(a, b):
            found = first_difference(x, y, path)
            if found:
                return found
        if len(a) != len(b):
            longer, side = (a, "oracle") if len(a) > len(b) else (b, "randoc")
            extra = longer[min(len(a), len(b))]
            name = extra.get("t", "value") if isinstance(extra, dict) else type(extra).__name__
            return "%s: %s has one more %s" % ("/".join(path), side, name)
        return None
    if a != b:
        return "%s: value" % "/".join(path)
    return None


BATCH = 400


def compare(cases, verbose):
    """Runs both readers on the cases. Returns the number of different cases."""
    oracle = os.environ.get("RANDOC_ORACLE_PANDOC")
    if not oracle:
        sys.exit("set RANDOC_ORACLE_PANDOC")
    batch = os.path.join(os.environ.get("CARGO_TARGET_DIR", "target"), "debug", "examples", "html_batch")
    expected, actual = [], []
    with tempfile.TemporaryDirectory(prefix="html-reader-diff-") as folder:
        script = os.path.join(folder, "probe.lua")
        with open(script, "w", encoding="utf-8") as handle:
            handle.write(LUA)
        path = os.path.join(folder, "cases.txt")
        for start in range(0, len(cases), BATCH):
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(SEPARATOR.join(cases[start:start + BATCH]))
            expected += run([oracle, "lua", script], path)
            actual += run([batch], path)
    classes = {}
    equal = 0
    for index, case in enumerate(cases):
        want = expected[index] if index < len(expected) else ("MISSING", [])
        got = actual[index] if index < len(actual) else ("MISSING", [])
        signature = None
        if want[0].startswith("ERROR") or got[0].startswith("ERROR") or "MISSING" in (want[0], got[0]):
            if want[0] != got[0]:
                signature = "error: oracle %s, randoc %s" % (want[0][:40], got[0][:40])
        else:
            try:
                signature = first_difference(json.loads(want[0]), json.loads(got[0]), [])
            except ValueError:
                signature = "output is not JSON"
        if signature is None and want[1] != got[1]:
            signature = "messages differ"
        if signature is None:
            equal += 1
            continue
        classes.setdefault(signature, []).append(index)
        if verbose:
            only_oracle = [log for log in want[1] if log not in got[1]]
            only_randoc = [log for log in got[1] if log not in want[1]]
            print("=== case %d: %s\n%s\n  oracle: %s %s\n  randoc: %s %s" % (
                index + 1, signature, case, want[0], only_oracle, got[0], only_randoc))
    print("cases: %d, equal: %d, different: %d, classes: %d" % (
        len(cases), equal, len(cases) - equal, len(classes)))
    for signature, indexes in sorted(classes.items(), key=lambda item: -len(item[1])):
        example = cases[indexes[0]].replace("\n", "\\n")
        print("%5d  %s\n       example %d: %s" % (len(indexes), signature, indexes[0] + 1, example[:300]))
    return len(cases) - equal


def toml_cases(path):
    """Returns the inputs of a conformance case file, with the format of each case."""
    import tomllib
    with open(path, "rb") as handle:
        data = tomllib.load(handle)
    cases = []
    for case in data.get("case", []):
        if "input" in case and "cmd" not in case:
            source = case.get("from", data.get("from", "html"))
            cases.append("@%s\n%s" % (source, case["input"]))
    return cases


def main():
    arguments = [argument for argument in sys.argv[1:] if not argument.startswith("-")]
    verbose = "-v" in sys.argv
    if len(arguments) < 2:
        sys.exit(__doc__)
    mode = arguments[0]
    if mode == "cases":
        cases = []
        for path in arguments[1:]:
            with open(path, encoding="utf-8") as handle:
                cases.extend(handle.read().split(SEPARATOR))
    elif mode == "toml":
        cases = [case for path in arguments[1:] for case in toml_cases(path)]
    elif mode == "sweep":
        cases = sweep(random.Random(int(arguments[1])), int(arguments[2]) if len(arguments) > 2 else 3000)
    elif mode in ("random", "mutate", "structure"):
        rng = random.Random(int(arguments[1]))
        count = int(arguments[2]) if len(arguments) > 2 else 200
        formats = ["", "", "", "@html+raw_html\n", "@html-native_divs-native_spans\n"]
        if mode == "random":
            cases = [rng.choice(formats) + "".join(random_element(rng, 4) for _ in range(rng.choice([1, 2, 3])))
                     for _ in range(count)]
        elif mode == "structure":
            cases = [rng.choice(formats) + random_structure(rng) for _ in range(count)]
        else:
            cases = [rng.choice(formats) + mutate(rng, rng.choice(PAGES)) for _ in range(count)]
    else:
        sys.exit(__doc__)
    different = compare(cases, verbose)
    sys.exit(1 if different else 0)


if __name__ == "__main__":
    main()
