#!/usr/bin/env python3
"""Builds the data files of randoc-highlight.

Commands:

  syntax SKYLIGHTING_XML_DIR
      The directory is `xml/` of the Hackage package skylighting-core-0.15.
      Writes data/syntax-definitions.txt (the permissive syntax definitions,
      concatenated), NOTICE.md and third_party/licences/syntax-definitions.txt.
      Only files with a permissive `license=` attribute go in.

  cases
      Writes the conformance case files in conformance/cases/highlight/ from
      the samples in data/samples.txt.

  oracle
      Runs the oracle ($RANDOC_ORACLE_PANDOC) on the samples in
      data/samples.txt and writes data/replay.txt with the HTML and LaTeX
      output for each sample.

  long
      Runs the oracle on the long inputs of LONG (the samples of the
      reviews) and writes data/long.txt with the hashes of the HTML and
      LaTeX output. A run that does not end in 60 seconds gets the status
      `timeout`, and the replay test does not compare it.

Run from the root of the worktree.
"""

import json
import os
import re
import subprocess
import sys

CRATE = "crates/randoc-highlight"
SEP = "\x0c"

PERMISSIVE = {
    "MIT": "MIT",
    "New BSD License": "BSD-3-Clause",
    "BSD3": "BSD-3-Clause",
    "WTFPL": "WTFPL",
    "LLVM Release License": "NCSA",
}

MIT = """Permission is hereby granted, free of charge, to any person obtaining a
copy of this software and associated documentation files (the "Software"),
to deal in the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense,
and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE."""

BSD3 = """Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from this
   software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE."""

NCSA = """University of Illinois/NCSA Open Source License

Developed by: LLVM Team, University of Illinois at Urbana-Champaign,
http://llvm.org

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to
deal with the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense,
and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

    * Redistributions of source code must retain the above copyright
      notice, this list of conditions and the following disclaimers.

    * Redistributions in binary form must reproduce the above copyright
      notice, this list of conditions and the following disclaimers in the
      documentation and/or other materials provided with the distribution.

    * Neither the names of the LLVM Team, University of Illinois at
      Urbana-Champaign, nor the names of its contributors may be used to
      endorse or promote products derived from this Software without
      specific prior written permission.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
CONTRIBUTORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
WITH THE SOFTWARE."""

WTFPL = """DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE
Version 2, December 2004

Everyone is permitted to copy and distribute verbatim or modified copies of
this license document, and changing it is allowed as long as the name is
changed.

DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE
TERMS AND CONDITIONS FOR COPYING, DISTRIBUTION AND MODIFICATION

0. You just DO WHAT THE FUCK YOU WANT TO."""

TEXTS = {"MIT": MIT, "BSD-3-Clause": BSD3, "NCSA": NCSA, "WTFPL": WTFPL}

SOURCE = (
    "skylighting-core 0.15 as released on Hackage, directory `xml/` "
    "(https://hackage.haskell.org/package/skylighting-core-0.15, tarball SHA-256 "
    "485721e90b3f5e52369a11f08994969a224c038ae4c812734233ff3b30b6fa61, source "
    "repository https://github.com/jgm/skylighting). The files come from the KDE "
    "syntax-highlighting project (https://invent.kde.org/frameworks/syntax-highlighting)."
)

COPYRIGHT_LINE = re.compile(r"^\s*(SPDX-FileCopyrightText:.*|Copyright\s*(\(c\)|\(C\)|©|\d).*)$")


def licence_of(text):
    m = re.search(r'<language[^>]*?\slicense="([^"]*)"', text, re.S)
    return m.group(1) if m else ""


def attr_of(text, name):
    m = re.search(r'<language[^>]*?\s' + name + r'="([^"]*)"', text, re.S)
    return m.group(1) if m else ""


def syntax(xml_dir):
    names = sorted(n for n in os.listdir(xml_dir) if n.endswith(".xml") and not n.startswith("._"))
    chosen = []
    for name in names:
        with open(os.path.join(xml_dir, name), encoding="utf-8") as f:
            text = f.read()
        lic = licence_of(text)
        if lic in PERMISSIVE:
            chosen.append((name, text, PERMISSIVE[lic], lic))
    if len(chosen) != 63:
        sys.exit(f"expected 63 permissive files, found {len(chosen)}")
    with open(f"{CRATE}/data/syntax-definitions.txt", "w", encoding="utf-8") as out:
        for name, text, _, _ in chosen:
            out.write(f"{SEP}{name}\n{text}")
            if not text.endswith("\n"):
                out.write("\n")
    notice = syntax_notice(chosen)
    with open("third_party/licences/syntax-definitions.txt", "w", encoding="utf-8") as f:
        f.write(notice)
    with open("third_party/licences/skylighting-core.txt", encoding="utf-8") as f:
        skylighting = f.read().rstrip("\n")
    with open(f"{CRATE}/NOTICE.md", "w", encoding="utf-8") as f:
        f.write(
            "# Third-party notices for `randoc-highlight`\n\n"
            "This crate contains material from the projects below. Each one keeps its own\n"
            "licence. The rest of the crate has the licence `MIT OR Apache-2.0`.\n\n"
            "- The tokenizer, the regular expression engine, the definition parser, the\n"
            "  styles and the HTML and LaTeX formatters: ported from `skylighting-core` 0.15,\n"
            "  `skylighting-format-blaze-html` 0.1.2.2 and `skylighting-format-latex` 0.1\n"
            "  (BSD-3-Clause, https://github.com/jgm/skylighting, commit\n"
            "  9a414fca3ac94dee81fb76e0ef4f88c9e6037618).\n"
            "- `data/syntax-definitions.txt`: 63 KDE syntax definitions with a permissive\n"
            "  licence (MIT, BSD-3-Clause, NCSA, WTFPL). The notices follow below.\n\n"
            "This file is generated by `tools/data.py syntax`.\n\n"
            "## `skylighting-core` 0.15\n\n```text\n" + skylighting + "\n```\n\n"
            "## Syntax definitions\n\n```text\n" + notice.rstrip("\n") + "\n```\n"
        )
    print(f"wrote {len(chosen)} definitions")


def syntax_notice(chosen):
    parts = [
        "Notices for the syntax definitions in data/syntax-definitions.txt",
        "",
        "Source: " + SOURCE,
        "",
    ]
    for name, text, spdx, raw in chosen:
        lines = []
        for line in text.splitlines()[:80]:
            m = COPYRIGHT_LINE.match(line)
            if m:
                lines.append(m.group(1).strip())
        author = attr_of(text, "author")
        parts.append(f"{name}: licence {spdx} (license=\"{raw}\")")
        for line in lines:
            parts.append(f"  {line}")
        if author:
            parts.append(f"  Copyright: {author}")
        if not lines and not author:
            parts.append("  Copyright: the authors of the KDE syntax-highlighting project")
    parts.append("")
    for spdx in ["MIT", "BSD-3-Clause", "NCSA", "WTFPL"]:
        parts.append(f"--- {spdx} ---")
        parts.append("")
        parts.append(TEXTS[spdx])
        parts.append("")
    return "\n".join(parts)


MUTATIONS_PER_LANGUAGE = 200


def fnv64(text):
    h = 0xCBF29CE484222325
    for b in text.encode("utf-8"):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


class Lcg:
    """A deterministic random generator. The replay file stores the
    operations that it chooses, so the Rust test needs no generator."""

    def __init__(self, seed):
        self.state = seed

    def below(self, n):
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        return (self.state >> 33) % max(n, 1)


def apply_ops(code, ops):
    """Applies mutation operations. The Rust replay test has the same rules:
    d<i> deletes character i, u<i> duplicates character i, K<j> deletes line
    j, L<j> duplicates line j. An index out of range does nothing."""
    for op in ops:
        kind, n = op[0], int(op[1:])
        if kind in "du":
            if n < len(code):
                code = code[:n] + (code[n] * 2 if kind == "u" else "") + code[n + 1 :]
        else:
            lines = code.split("\n")
            if n < len(lines):
                lines[n : n + 1] = [lines[n]] * (2 if kind == "L" else 0)
                code = "\n".join(lines)
    return code


def mutations(samples):
    by_lang = {}
    for i, (lang, _) in enumerate(samples):
        by_lang.setdefault(lang, []).append(i)
    out = []
    rng = Lcg(20261110)
    for lang in sorted(by_lang):
        for _ in range(MUTATIONS_PER_LANGUAGE):
            base = by_lang[lang][rng.below(len(by_lang[lang]))]
            code = samples[base][1]
            ops = []
            for _ in range(1 + rng.below(3)):
                kind = "duKL"[rng.below(4)]
                if kind in "du":
                    ops.append(f"{kind}{rng.below(len(code))}")
                else:
                    ops.append(f"{kind}{rng.below(code.count(chr(10)) + 1)}")
                code = apply_ops(code, ops[-1:])
            out.append((base, lang, ops, code))
    return out


STYLES = ["pygments", "tango", "espresso", "zenburn", "kate", "monochrome", "breezedark", "haddock"]


def toml_str(text):
    return json.dumps(text, ensure_ascii=False).replace("\x7f", "\\u007f")


def tree(blocks):
    return json.dumps({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks}, ensure_ascii=False)


def cases():
    with open(f"{CRATE}/data/samples.txt", encoding="utf-8") as f:
        samples = parse_samples(f.read())
    by_lang = {}
    for lang, code in samples:
        by_lang.setdefault(lang, []).append(code)
    langs = sorted(by_lang)
    folder = "conformance/cases/highlight"
    os.makedirs(folder, exist_ok=True)
    header = (
        "# Generated by crates/randoc-highlight/tools/data.py cases from the samples\n"
        "# of the crate. Each case is one code block of one language.\n"
        'from = "json"\nto = ["html", "latex"]\n'
    )
    groups = [langs[0:21], langs[21:42], langs[42:63]]
    for n, group in enumerate(groups, 1):
        with open(f"{folder}/blocks-{n}.toml", "w", encoding="utf-8") as f:
            f.write(header)
            for lang in group:
                for k, code in enumerate(by_lang[lang][:5]):
                    block = {"t": "CodeBlock", "c": [["", [lang], []], code]}
                    f.write(f'\n[[case]]\nid = "{lang}-{k}"\ninput = {toml_str(tree([block]))}\n')
    attrs = [
        ("number-lines", "", ["numberLines"], []),
        ("number-lines-dash", "", ["number-lines"], []),
        ("start-from", "", ["numberLines"], [["startFrom", "7"]]),
        ("start-from-only", "", [], [["startFrom", "3"]]),
        ("start-from-negative", "", ["numberLines"], [["startFrom", "-2"]]),
        ("start-from-bad", "", ["numberLines"], [["startFrom", "x"]]),
        ("identifier", "my-code", [], []),
        ("identifier-numbered", "n1", ["numberLines"], []),
        ("extra-classes", "", ["first", "second"], []),
        ("key-values", "", [], [["data-x", "1"], ["title", "t"]]),
        ("unknown-language", "", ["nosuchlanguage"], []),
        ("no-class", "", [], []),
    ]
    with open(f"{folder}/attributes.toml", "w", encoding="utf-8") as f:
        f.write(header.replace("one code block of one language", "one attribute form"))
        for i, lang in enumerate(langs):
            code = by_lang[lang][1]
            name, ident, classes, kvs = attrs[i % len(attrs)]
            cls = [lang] + classes if name not in ("unknown-language", "no-class") else classes
            block = {"t": "CodeBlock", "c": [[ident, cls, kvs], code]}
            f.write(f'\n[[case]]\nid = "{name}-{lang}"\ninput = {toml_str(tree([block]))}\n')
            first = code.split("\n")[0]
            inline = {"t": "Para", "c": [{"t": "Str", "c": "x"}, {"t": "Space"},
                      {"t": "Code", "c": [[ident, cls, kvs], first]}]}
            f.write(f'\n[[case]]\nid = "inline-{name}-{lang}"\ninput = {toml_str(tree([inline]))}\n')
        many = [{"t": "CodeBlock", "c": [["", [lang], []], by_lang[lang][2]]} for lang in langs[:10]]
        f.write(f'\n[[case]]\nid = "ten-blocks"\ninput = {toml_str(tree(many))}\n')
        f.write('\n[[case]]\nid = "no-highlight"\nargs = ["--no-highlight"]\n'
                f'input = {toml_str(tree(many[:2]))}\n')
    sample = tree([{"t": "CodeBlock", "c": [["", ["rust"], []], by_lang["rust"][0]]}])
    with open(f"{folder}/styles.toml", "w", encoding="utf-8") as f:
        f.write("# Generated by crates/randoc-highlight/tools/data.py cases: the highlight styles.\n")
        f.write('\n[[case]]\nid = "list-languages"\ncmd = ["--list-highlight-languages"]\n')
        f.write('\n[[case]]\nid = "list-styles"\ncmd = ["--list-highlight-styles"]\n')
        for style in STYLES:
            f.write(f'\n[[case]]\nid = "print-{style}"\ncmd = ["--print-highlight-style", "{style}"]\n')
            for fmt, ext in [("html", "html"), ("latex", "tex")]:
                var = "highlighting-css" if fmt == "html" else "highlighting-macros"
                f.write(
                    f'\n[[case]]\nid = "{var}-{style}"\n'
                    f'cmd = ["-f", "json", "-t", "{fmt}", "-s", "--template=t.{ext}", "--highlight-style={style}"]\n'
                    f'files = {{ "t.{ext}" = "${var}$\\n" }}\ninput = {toml_str(sample)}\n'
                )
            f.write(
                f'\n[[case]]\nid = "body-{style}"\nargs = ["--highlight-style={style}"]\n'
                f'from = "json"\nto = ["html", "latex"]\ninput = {toml_str(sample)}\n'
            )
    print("wrote the case files")


def oracle():
    pandoc = os.environ["RANDOC_ORACLE_PANDOC"]
    with open(f"{CRATE}/data/samples.txt", encoding="utf-8") as f:
        samples = parse_samples(f.read())
    muts = mutations(samples)
    codes = [(lang, code) for lang, code in samples] + [(lang, code) for _, lang, _, code in muts]
    results = {"html": [], "latex": []}
    chunk = 500
    for start in range(0, len(codes), chunk):
        part = codes[start : start + chunk]
        for fmt in results:
            results[fmt].extend(run_chunk(pandoc, part, fmt))
        print(f"{start + len(part)} of {len(codes)}", file=sys.stderr)
    with open(f"{CRATE}/data/replay.txt", "w", encoding="utf-8") as f:
        for i, (lang, _) in enumerate(samples):
            f.write(f"{SEP}s {i} {lang}\n{results['html'][i]}\n\x0b\n{results['latex'][i]}\n")
        for k, (base, lang, ops, _) in enumerate(muts):
            i = len(samples) + k
            f.write(
                f"{SEP}m {base} {lang} {'.'.join(ops)} "
                f"{fnv64(results['html'][i])} {fnv64(results['latex'][i])}\n"
            )
    print(f"wrote {len(samples)} samples and {len(muts)} mutations")


ADOC = "Lorem ipsum dolor sit amet, see `code` and *strong* text. "
NUMS = "123,456,789,"
CALLS = "x=foo(12,34);y=x+1;"

# name, language, prefix, unit, repetitions, suffix, rows, row separator, tail.
# The code is `sep.join([prefix + unit * reps + suffix] * rows) + tail`.
LONG = [
    ("rust-array", "rust", "let data = [", NUMS, 4831, "0];", 1, "", ""),
    ("julia-array", "julia", "values = [", NUMS, 1383, "0]", 1, "", ""),
    ("rust-minified", "rust", "fn main(){let mut x=0;let mut y=0;", CALLS, 2747, "}", 1, "", ""),
    ("julia-minified", "julia", "function f();x=0;y=0;", CALLS, 641, "end", 1, "", ""),
    ("julia-text", "julia", "v = [", NUMS, 1000, "0]", 35, "\n", "\nx = 1\n"),
    ("asciidoc-line", "asciidoc", "", ADOC, 100, "", 1, "", ""),
    ("asciidoc-text", "asciidoc", "", ADOC, 100, "", 20, "\n\n", ""),
    ("asciidoc-two", "asciidoc", "", ADOC, 100, "", 2, "\n\n", ""),
]
for size in (100_000, 500_000):
    LONG += [
        (f"rust-array-{size}", "rust", "let data = [", NUMS, size // 12, "0];", 1, "", ""),
        (f"julia-array-{size}", "julia", "values = [", NUMS, size // 12, "0]", 1, "", ""),
        (f"rust-minified-{size}", "rust", "fn main(){", CALLS, size // 19, "}", 1, "", ""),
        (f"julia-minified-{size}", "julia", "function f();", CALLS, size // 19, "end", 1, "", ""),
        (f"asciidoc-line-{size}", "asciidoc", "", ADOC, size // 58, "", 1, "", ""),
    ]


def long_code(prefix, unit, reps, suffix, rows, sep, tail):
    return sep.join([prefix + unit * reps + suffix] * rows) + tail


def oracle_long():
    pandoc = os.environ["RANDOC_ORACLE_PANDOC"]
    out = []
    for name, lang, *spec in LONG:
        code = long_code(*spec)
        procs = {fmt: start_chunk(pandoc, [(lang, code)], fmt) for fmt in ("html", "latex")}
        hashes = []
        for fmt, (proc, tree_text) in procs.items():
            try:
                stdout, _ = proc.communicate(tree_text, timeout=60)
                hashes.append(fnv64(split_output(stdout, fmt, 1)[0]) if proc.returncode == 0 else "error")
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.communicate()
                hashes.append("timeout")
        status = "ok" if all(len(h) == 16 for h in hashes) else "-".join(sorted(set(h for h in hashes if len(h) != 16)))
        fields = [name, lang, status, *hashes] + [json.dumps(x, ensure_ascii=False) for x in spec]
        out.append("\t".join(fields) + "\n")
        print(name, status, len(code), file=sys.stderr)
    with open(f"{CRATE}/data/long.txt", "w", encoding="utf-8") as f:
        f.writelines(out)


MARK = "RANDOCHIGHLIGHTSEPARATOR"


def start_chunk(pandoc, codes, fmt):
    doc = []
    for lang, code in codes:
        ident = "c" if fmt == "html" else ""
        doc.append({"t": "CodeBlock", "c": [[ident, [lang], []], code]})
        doc.append({"t": "Para", "c": [{"t": "Str", "c": MARK}]})
    tree = {"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": doc}
    proc = subprocess.Popen(
        [pandoc, "-f", "json", "-t", fmt, "--wrap=none"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return proc, json.dumps(tree)


def split_output(stdout, fmt, n):
    para = "<p>" + MARK + "</p>" if fmt == "html" else MARK
    pieces = [p.strip("\n") for p in stdout.split(para)][:n]
    if len(pieces) != n:
        sys.exit("bad oracle split")
    return pieces


def run_chunk(pandoc, codes, fmt):
    doc = []
    for lang, code in codes:
        ident = "c" if fmt == "html" else ""
        doc.append({"t": "CodeBlock", "c": [[ident, [lang], []], code]})
        doc.append({"t": "Para", "c": [{"t": "Str", "c": MARK}]})
    tree = {"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": doc}
    res = subprocess.run(
        [pandoc, "-f", "json", "-t", fmt, "--wrap=none"],
        input=json.dumps(tree),
        capture_output=True,
        text=True,
        check=True,
    )
    para = "<p>" + MARK + "</p>" if fmt == "html" else MARK
    pieces = [p.strip("\n") for p in res.stdout.split(para)][: len(codes)]
    if len(pieces) != len(codes):
        sys.exit("bad oracle split")
    return pieces


def parse_samples(text):
    samples = []
    for part in text.split(SEP)[1:]:
        head, _, body = part.partition("\n")
        if body.endswith("\n"):
            body = body[:-1]
        samples.append((head.strip(), body))
    return samples


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "syntax":
        syntax(sys.argv[2])
    elif len(sys.argv) == 2 and sys.argv[1] == "cases":
        cases()
    elif len(sys.argv) == 2 and sys.argv[1] == "oracle":
        oracle()
    elif len(sys.argv) == 2 and sys.argv[1] == "long":
        oracle_long()
    else:
        sys.exit(__doc__)
