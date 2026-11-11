#!/usr/bin/env python3
"""Time table of standing order 21c for the HTML reader.

The tool runs the Randoc program on inputs that repeat one construct
25,000 and 50,000 times, and prints a Markdown table with the minimum
time of 3 runs for each size and the ratio of the two times. A ratio
near 2 is linear; a ratio near 4 is quadratic.

Usage (from the root of the worktree, after `. env.sh`):

  cargo build -p randoc-cli
  crates/randoc-html-reader/tools/timing.py target/debug/randoc
"""

import subprocess
import sys
import time

UNITS = [
    ("paragraphs", "<p>a <b>b</b> <i>c</i></p>\n"),
    ("open b", "<b>"),
    ("close b", "</b>"),
    ("p with open b", "<p><b>x"),
    ("div with open b", "<div><b>x</div>"),
    ("list items", "<li>a</li>"),
    ("lists", "<ul><li>a</li></ul>"),
    ("open li", "<li>a"),
    ("nested lists", "<ul><li>a"),
    ("tables", "<table><tr><td>a</td></tr></table>"),
    ("rows", "<tr><td>a</td><td>b</td></tr>"),
    ("cells", "<td>a</td>"),
    ("col elements", "<col width=\"10%\">"),
    ("definition lists", "<dl><dt>a</dt><dd>b</dd></dl>"),
    ("failing definition lists", "<dl><dt>a</dt>"),
    ("open style", "<style>"),
    ("open svg", "<svg>"),
    ("scripts", "<script>x</script>"),
    ("note references", "<a href=\"#x\" role=\"doc-noteref\">1</a>"),
    ("headings", "<h1>x</h1>"),
    ("attributes", " a=\"1\""),
    ("class words", " c"),
    ("references", "&amp;&#65;&nosuch;"),
    ("comments", "<!-- c -->"),
    ("open q", "<q>"),
    ("open span", "<span>"),
    ("open div", "<div>"),
    ("stray close div", "</div>"),
    ("breaks", "a<br>"),
    ("links", "<a href=\"u\">x</a> "),
    ("images", "<img src=\"i.png\" alt=\"a\">"),
    ("code blocks", "<pre><code>x\n</code></pre>"),
    ("open table", "<table>"),
    ("open dl", "<dl>"),
    ("smart quotes", "\"a 'b' c\" "),
    ("open double quotes", "\""),
    ("open single quotes", "' '"),
    ("nested code", "<tt><b>"),
    ("dollar math", "$x$ "),
    ("open dollar text groups", "$\\text{"),
    ("open backslash math", "\\(a "),
    ("ref commands", "\\ref{a} "),
    ("open ref braces", "\\ref{"),
    ("open ref brackets", "\\ref["),
    ("open environments", "\\begin{equation}"),
    ("line blocks", "<div class=\"line-block\">a<br>b</div>"),
    ("relative col widths", "<col width=\"2*\">"),
    ("span cells", "<td colspan=\"2\" rowspan=\"2\">a</td>"),
    ("C1 references", "&#128;"),
    ("titles", "<title>a<!--c-->b</title>"),
    ("note headings", "<h1>a</h1>"),
]

WRAP = {
    "list items": ("<ul>", "</ul>"),
    "rows": ("<table>", "</table>"),
    "cells": ("<table><tr>", "</tr></table>"),
    "col elements": ("<table><colgroup>", "</colgroup><tr><td>a</td></tr></table>"),
    "relative col widths": ("<table><colgroup>", "</colgroup><tr><td>a</td></tr></table>"),
    "span cells": ("<table><tr>", "</tr></table>"),
    "titles": ("<head>", "</head>"),
    "nested code": ("", "x"),
    "note headings": ("<p><a role=\"doc-noteref\" href=\"#n\">1</a></p><section role=\"doc-endnotes\"><ol><li id=\"n\">", "</li></ol></section>"),
    "attributes": ("<span", ">x</span>"),
    "class words": ("<span class=\"", "\">x</span>"),
}

FORMAT = {
    "smart quotes": "html+smart",
    "open double quotes": "html+smart",
    "open single quotes": "html+smart",
    "dollar math": "html+tex_math_dollars",
    "open dollar text groups": "html+tex_math_dollars",
    "open backslash math": "html+tex_math_single_backslash",
    "ref commands": "html+raw_tex",
    "open ref braces": "html+raw_tex",
    "open ref brackets": "html+raw_tex",
    "open environments": "html+raw_tex",
}


def best_time(binary, data, fmt):
    best, code = None, None
    for _ in range(3):
        start = time.monotonic()
        result = subprocess.run(
            [binary, "-f", fmt, "-t", "json"],
            input=data,
            capture_output=True,
            timeout=120,
            check=False,
        )
        elapsed = time.monotonic() - start
        best = elapsed if best is None else min(best, elapsed)
        code = result.returncode
    return best, code


def main():
    binary = sys.argv[1]
    print("| Construct | 25,000 | 50,000 | Ratio |")
    print("| --- | --- | --- | --- |")
    for name, unit in UNITS:
        before, after = WRAP.get(name, ("", ""))
        fmt = FORMAT.get(name, "html")
        cells = []
        for count in (25_000, 50_000):
            data = (before + unit * count + after).encode()
            cells.append(best_time(binary, data, fmt))
        (small, code_small), (large, code_large) = cells
        ratio = large / small if small > 0 else 0.0
        print(
            f"| {name} | {small:.2f} s (exit {code_small}) "
            f"| {large:.2f} s (exit {code_large}) | {ratio:.1f} |",
            flush=True,
        )


if __name__ == "__main__":
    main()
