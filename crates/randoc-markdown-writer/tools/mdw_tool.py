#!/usr/bin/env python3
"""Test tools for the Markdown writer.

Subcommands:
  diff   Generate random document trees and compare the oracle with Randoc.
  corpus Classify the corpus documents whose writer output differs.
  cases  Write the conformance case files in conformance/cases/markdown-writer/.

The trees and the cases are original data (standing order 7a).
"""

import argparse
import collections
import concurrent.futures
import json
import os
import random
import re
import subprocess
import sys

API = [1, 23, 1, 2]

# ---------------------------------------------------------------- tree builders


def Str(s):
    return {"t": "Str", "c": s}


SP = {"t": "Space"}
SB = {"t": "SoftBreak"}
LB = {"t": "LineBreak"}


def T(text):
    """Split text into Str and Space nodes. A run of spaces gives one Space."""
    return [SP if m.group(1) else Str(m.group(2)) for m in re.finditer(r"( +)|([^ ]+)", text)]


def I(*parts):
    """Build an inline list from strings (split with T) and inline nodes."""
    out = []
    for p in parts:
        if isinstance(p, str):
            out.extend(T(p))
        elif isinstance(p, list):
            out.extend(p)
        else:
            out.append(p)
    return out


def A(ident="", cls=(), kv=()):
    return [ident, list(cls), [list(p) for p in kv]]


NA = A()


def _wrap(t):
    return lambda *xs: {"t": t, "c": I(*xs)}


Emph = _wrap("Emph")
Strong = _wrap("Strong")
Strike = _wrap("Strikeout")
Sup = _wrap("Superscript")
Sub = _wrap("Subscript")
SC = _wrap("SmallCaps")
Und = _wrap("Underline")


def Q1(*xs):
    return {"t": "Quoted", "c": [{"t": "SingleQuote"}, I(*xs)]}


def Q2(*xs):
    return {"t": "Quoted", "c": [{"t": "DoubleQuote"}, I(*xs)]}


def Code(s, attr=NA):
    return {"t": "Code", "c": [attr, s]}


def Math(s):
    return {"t": "Math", "c": [{"t": "InlineMath"}, s]}


def DMath(s):
    return {"t": "Math", "c": [{"t": "DisplayMath"}, s]}


def RawI(fmt, s):
    return {"t": "RawInline", "c": [fmt, s]}


def Link(xs, url, title="", attr=NA):
    return {"t": "Link", "c": [attr, I(xs), [url, title]]}


def Image(xs, url, title="", attr=NA):
    return {"t": "Image", "c": [attr, I(xs), [url, title]]}


def Note(*bs):
    return {"t": "Note", "c": list(bs)}


def Span(attr, *xs):
    return {"t": "Span", "c": [attr, I(*xs)]}


def cit(key, mode="NormalCitation", prefix="", suffix="", num=1):
    return {
        "citationId": key,
        "citationPrefix": I(prefix),
        "citationSuffix": I(suffix),
        "citationMode": {"t": mode},
        "citationNoteNum": num,
        "citationHash": 0,
    }


def Cite(cits, *xs):
    return {"t": "Cite", "c": [cits, I(*xs)]}


def Para(*xs):
    return {"t": "Para", "c": I(*xs)}


def Plain(*xs):
    return {"t": "Plain", "c": I(*xs)}


def LineBlock(*lines):
    return {"t": "LineBlock", "c": [I(x) for x in lines]}


def CodeBlock(s, attr=NA):
    return {"t": "CodeBlock", "c": [attr, s]}


def RawB(fmt, s):
    return {"t": "RawBlock", "c": [fmt, s]}


def BQ(*bs):
    return {"t": "BlockQuote", "c": list(bs)}


def _items(items):
    return [[Plain(it)] if isinstance(it, str) else list(it) for it in items]


def OL(items, start=1, style="Decimal", delim="Period"):
    return {"t": "OrderedList", "c": [[start, {"t": style}, {"t": delim}], _items(items)]}


def BL(items):
    return {"t": "BulletList", "c": _items(items)}


def DL(entries):
    """entries: list of (term, [definition, ...]); a definition is a str or a block list."""
    return {
        "t": "DefinitionList",
        "c": [[I(term), _items(defs)] for term, defs in entries],
    }


def H(level, xs, attr=NA):
    return {"t": "Header", "c": [level, attr, I(xs)]}


HR = {"t": "HorizontalRule"}


def Div(attr, *bs):
    return {"t": "Div", "c": [attr, list(bs)]}


def Fig(attr, caption, *bs, short=None):
    return {"t": "Figure", "c": [attr, [None if short is None else I(short), caption], list(bs)]}


def cell(content, align="AlignDefault", rs=1, cs=1):
    blocks = [Plain(content)] if isinstance(content, str) else list(content)
    if content == "":
        blocks = []
    return [NA, {"t": align}, rs, cs, blocks]


def row(cells):
    return [NA, [c if isinstance(c, list) and len(c) == 5 and isinstance(c[1], dict) else cell(c) for c in cells]]


def Table(head, body, aligns=None, widths=None, caption=None, foot=(), rowhead=0, attr=NA, short=None):
    ncols = max(len(r) for r in (list(head) + list(body) + list(foot)) or [[0]])
    aligns = aligns or ["AlignDefault"] * ncols
    widths = widths or [None] * ncols
    specs = [
        [{"t": a}, {"t": "ColWidthDefault"} if w is None else {"t": "ColWidth", "c": w}]
        for a, w in zip(aligns, widths)
    ]
    cap = [] if caption is None else ([Plain(caption)] if isinstance(caption, str) else caption)
    return {
        "t": "Table",
        "c": [
            attr,
            [None if short is None else I(short), cap],
            specs,
            [NA, [row(r) for r in head]],
            [[NA, rowhead, [], [row(r) for r in body]]],
            [NA, [row(r) for r in foot]],
        ],
    }


def doc(blocks, meta=None):
    if isinstance(blocks, dict):
        blocks, meta = blocks["blocks"], blocks["meta"]
    s = json.dumps(
        {"pandoc-api-version": API, "meta": meta or {}, "blocks": blocks},
        ensure_ascii=False,
        separators=(",", ":"),
    )
    for ch in "\u00a0\u200b\u2007\u202f\u2060\ufeff":
        s = s.replace(ch, "\\u%04x" % ord(ch))
    return s


# ---------------------------------------------------------------- random trees

WORDS = [
    "alpha", "beta", "gamma", "delta", "word", "text", "river", "stone", "lamp", "cloud",
    "*", "_", "`", "#", "+", "-", "1.", "2)", ">", "|", "<", "&", "$", "^", "~", "@", "[", "]",
    "!", "\\", "'", '"', "a*b", "x_y", "<b>", "&amp;", "$9", "^up", "~sub", "@key", "[x]", "![i]",
    "#tag", "1986.", "--", "...", "a\u00a0b", "\u00a0", "end*", "*start", "q\"", "it's",
    "http://e.x/p", "{.c}", ":::", "```", "~~~", "a|b", "\\*",
]

LONG = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore"

BLOCK_TYPES = [
    "Plain", "Para", "LineBlock", "CodeBlock", "RawBlock", "BlockQuote", "OrderedList", "BulletList",
    "DefinitionList", "Header", "HorizontalRule", "Table", "Figure", "Div",
]
INLINE_TYPES = [
    "Str", "Emph", "Underline", "Strong", "Strikeout", "Superscript", "Subscript", "SmallCaps", "Quoted",
    "Cite", "Code", "Space", "SoftBreak", "LineBreak", "Math", "RawInline", "Link", "Image", "Note", "Span",
]
STYLES = ["DefaultStyle", "Example", "Decimal", "LowerRoman", "UpperRoman", "LowerAlpha", "UpperAlpha"]
DELIMS = ["DefaultDelim", "Period", "OneParen", "TwoParens"]
ALIGNS = ["AlignLeft", "AlignRight", "AlignCenter", "AlignDefault"]
RAWFMTS = ["html", "tex", "latex", "markdown", "rst", "openxml"]


class Gen:
    def __init__(self, rng, no_math=False):
        self.r = rng
        self.no_math = no_math

    def word(self):
        r = self.r
        if r.random() < 0.05:
            return LONG
        return r.choice(WORDS)

    def attr(self):
        r = self.r
        if r.random() < 0.6:
            return NA
        return A(
            r.choice(["", "x1", "sec-2"]),
            r.sample(["c", "uri", "smallcaps", "underline", "emoji", "wide", "haskell"], r.randint(0, 2)),
            r.sample([("k", "v"), ("data-emoji", "smile"), ("width", "50%"), ("title", 'a "q"')], r.randint(0, 2)),
        )

    def text(self):
        return " ".join(self.word() for _ in range(self.r.randint(1, 3)))

    def inlines(self, depth, n=None):
        r = self.r
        n = r.randint(1, 5) if n is None else n
        out = []
        for _ in range(n):
            out.append(self.inline(depth))
            if r.random() < 0.6:
                out.append(SP)
        return out

    def inline(self, depth):
        r = self.r
        if depth <= 0 or r.random() < 0.35:
            return r.choice([Str(self.word()), Str(self.word()), SP, SB, LB, Code(self.text())])
        t = r.choice(INLINE_TYPES)
        if self.no_math and t == "Math":
            t = "Str"
        sub = lambda: self.inlines(depth - 1)
        if t in ("Emph", "Underline", "Strong", "Strikeout", "Superscript", "Subscript", "SmallCaps"):
            return {"t": t, "c": sub()}
        if t == "Quoted":
            return {"t": "Quoted", "c": [{"t": r.choice(["SingleQuote", "DoubleQuote"])}, sub()]}
        if t == "Cite":
            mode = r.choice(["AuthorInText", "SuppressAuthor", "NormalCitation"])
            cs = [cit(r.choice(["doe", "a:b", "x.y_z"]), mode, r.choice(["", "see"]), r.choice(["", ", p. 2"]))
                  for _ in range(r.randint(1, 2))]
            return Cite(cs, Str("[@cite]"))
        if t == "Code":
            return Code(r.choice(["x", "a`b", "``", " sp ", "<t>"]), self.attr())
        if t == "Math":
            return {"t": "Math", "c": [{"t": r.choice(["InlineMath", "DisplayMath"])}, r.choice(["x^2", "a $ b", "\\frac{1}{2}", " y "])]}
        if t == "RawInline":
            return RawI(r.choice(RAWFMTS), r.choice(["<b>", "\\emph{x}", "*m*", "`r`"]))
        if t == "Link":
            url = r.choice(["http://e.x/a", "mailto:p@e.x", "#frag", "a b.html", "/p(1)"])
            text = r.choice([sub(), [Str(url)], [Str(url.replace("mailto:", ""))]])
            return Link(text, url, r.choice(["", "ti", 'a "q"']), self.attr())
        if t == "Image":
            return Image(sub(), r.choice(["i.png", "a b.jpg"]), r.choice(["", "ti"]), self.attr())
        if t == "Note":
            return Note(*self.blocks(depth - 1, r.randint(1, 2)))
        if t == "Span":
            return Span(self.attr(), *sub())
        return Str(self.word())

    def items(self, depth):
        return [self.blocks(depth - 1, self.r.randint(1, 2)) for _ in range(self.r.randint(1, 3))]

    def blocks(self, depth, n=None):
        n = self.r.randint(1, 4) if n is None else n
        return [self.block(depth) for _ in range(n)]

    def block(self, depth):
        r = self.r
        if depth <= 0:
            return r.choice([Para(self.inlines(1)), Plain(self.inlines(1))])
        t = r.choice(BLOCK_TYPES)
        if t == "Plain":
            return {"t": "Plain", "c": self.inlines(depth)}
        if t == "Para":
            return {"t": "Para", "c": self.inlines(depth, r.randint(1, 12))}
        if t == "LineBlock":
            return {"t": "LineBlock", "c": [self.inlines(depth - 1) for _ in range(r.randint(1, 3))]}
        if t == "CodeBlock":
            return CodeBlock(r.choice(["x = 1", "```\nin\n```", "~~~~\n", "  lead\n\ttab", ""]), self.attr())
        if t == "RawBlock":
            return RawB(r.choice(RAWFMTS), r.choice(["<div>x</div>", "\\begin{x}\n\\end{x}", "# raw"]))
        if t == "BlockQuote":
            return BQ(*self.blocks(depth - 1))
        if t == "OrderedList":
            return {"t": "OrderedList", "c": [[r.choice([1, 0, 3, 10, 99]), {"t": r.choice(STYLES)}, {"t": r.choice(DELIMS)}], self.items(depth)]}
        if t == "BulletList":
            its = self.items(depth)
            if r.random() < 0.3:
                its[0] = [Plain(Str(r.choice(["\u2610", "\u2612"])), SP, *self.inlines(0))] + its[0][1:]
            return {"t": "BulletList", "c": its}
        if t == "DefinitionList":
            return {"t": "DefinitionList", "c": [[self.inlines(depth - 1), [self.blocks(depth - 1, r.randint(1, 2)) for _ in range(r.randint(1, 2))]] for _ in range(r.randint(1, 2))]}
        if t == "Header":
            return {"t": "Header", "c": [r.randint(1, 6), self.attr(), self.inlines(depth - 1)]}
        if t == "HorizontalRule":
            return HR
        if t == "Table":
            return self.table(depth)
        if t == "Figure":
            if r.random() < 0.5:
                cap = self.inlines(0)
                return Fig(self.attr(), [Plain(cap)], Plain(Image(cap, "f.png")))
            return Fig(self.attr(), [Plain(self.inlines(0))] if r.random() < 0.7 else [], *self.blocks(depth - 1, r.randint(1, 2)))
        return Div(self.attr(), *self.blocks(depth - 1))

    def table(self, depth):
        r = self.r
        ncols = r.randint(1, 4)

        def c():
            k = r.random()
            if k < 0.6:
                return cell([Plain(self.inlines(0))])
            if k < 0.8:
                return cell(self.blocks(max(depth - 2, 0), r.randint(1, 2)))
            return cell("")

        head = [[c() for _ in range(ncols)]] if r.random() < 0.7 else []
        body = [[c() for _ in range(ncols)] for _ in range(r.randint(1, 3))]
        if r.random() < 0.15 and ncols > 1:
            body[0] = [cell("span", cs=2)] + body[0][2:]
        foot = [[c() for _ in range(ncols)]] if r.random() < 0.1 else []
        widths = None if r.random() < 0.5 else [round(1.0 / ncols, 3)] * ncols
        cap = None if r.random() < 0.6 else [Plain(self.inlines(0))]
        return Table(head, body, [r.choice(ALIGNS) for _ in range(ncols)], widths, cap, foot,
                     rowhead=r.choice([0, 0, 1]))


def count_nodes(value):
    if isinstance(value, dict):
        return (1 if "t" in value else 0) + sum(count_nodes(v) for v in value.values())
    if isinstance(value, list):
        return sum(count_nodes(v) for v in value)
    return 0


def make_tree(seed, ident, no_math=False, small=False):
    rng = random.Random("%s:%s" % (seed, ident))
    if not small:
        return doc(Gen(rng, no_math).blocks(3, rng.randint(2, 6)))
    # A small tree: depth 2 and at most 6 nodes. Draw again until one fits.
    while True:
        blocks = Gen(rng, no_math).blocks(2, rng.randint(1, 2))
        if count_nodes(blocks) <= 6:
            return doc(blocks)


GEN_WORDS = sorted({w for w in WORDS + LONG.split() if w.isalpha()}, key=len, reverse=True)
WORD_RE = re.compile(r"(?<![A-Za-z])(" + "|".join(GEN_WORDS) + r")(?![A-Za-z])")


def signature(o, r):
    """The first differing line pair, with digits as N and generator words as W."""
    if o[1] == r[1]:
        return "exit %s vs %s / stderr differs" % (o[0], r[0])
    ol, rl = o[1].split("\n"), r[1].split("\n")
    for i in range(max(len(ol), len(rl))):
        a = ol[i] if i < len(ol) else "<END>"
        b = rl[i] if i < len(rl) else "<END>"
        if a != b:
            norm = lambda t: re.sub(r"[0-9]+", "N", WORD_RE.sub("W", t))[:70]
            return "%r | %r" % (norm(a), norm(b))
    return "same lines"


def node_types(value, seen):
    if isinstance(value, dict):
        if "t" in value:
            seen.add(value["t"])
        for v in value.values():
            node_types(v, seen)
    elif isinstance(value, list):
        for v in value:
            node_types(v, seen)
    return seen


# ---------------------------------------------------------------- diff


def run(cmd, tree):
    try:
        p = subprocess.run(cmd, input=tree.encode(), capture_output=True, timeout=60)
        return (p.returncode, p.stdout.decode(errors="replace"), p.stderr.decode(errors="replace"))
    except FileNotFoundError as e:
        return (-1, "", "cannot start: %s" % e)
    except subprocess.TimeoutExpired:
        return (-2, "", "time limit")


def extension_names(oracle, fmt):
    p = subprocess.run([oracle, "--list-extensions=" + fmt], capture_output=True, text=True)
    return [line[1:] for line in p.stdout.split() if line[:1] in "+-"]


def format_spec(seed, ident, fmt, exts):
    if not exts:
        return fmt
    rng = random.Random("ext:%s:%s:%s" % (seed, ident, fmt))
    return fmt + "".join(rng.choice("+-") + rng.choice(exts) for _ in range(20))


def cmd_diff(args):
    oracle = os.environ.get("RANDOC_ORACLE_PANDOC", "pandoc")
    formats = args.formats.split(",")
    exts = {f: extension_names(oracle, f.split("+")[0].split("-")[0]) if args.ext_changes else [] for f in formats}

    mk = lambda ident: make_tree(args.seed, ident, args.no_math, args.small)
    if args.show is not None:
        tree = mk(args.show)
        print(json.dumps(json.loads(tree), indent=1, ensure_ascii=False))
        for f in formats:
            spec = format_spec(args.seed, args.show, f, exts[f])
            o = run([oracle, "-f", "json", "-t", spec], tree)
            r = run([args.randoc, "-f", "json", "-t", spec], tree)
            for name, res in (("oracle", o), ("randoc", r)):
                print("==== %s %s (exit %d)" % (name, spec, res[0]))
                print(res[1] + (("---- stderr\n" + res[2]) if res[2] else ""), end="")
        return 0

    def job(ident):
        tree = mk(ident)
        out = {}
        for f in formats:
            spec = format_spec(args.seed, ident, f, exts[f])
            o = run([oracle, "-f", "json", "-t", spec], tree)
            if args.oracle_only:
                out[f] = o[0] != 0 and o[2].strip()
            elif o[0] == 251 or "Heap exhausted" in o[2]:
                out[f] = False
            else:
                r = run([args.randoc, "-f", "json", "-t", spec], tree)
                out[f] = o != r and signature(o, r)
        return ident, tree, out

    bad = {f: [] for f in formats}
    seen = set()
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for ident, tree, out in pool.map(job, range(args.count)):
            node_types(json.loads(tree), seen)
            for f, d in out.items():
                if d:
                    bad[f].append((ident, d))
    label = "oracle rejects" if args.oracle_only else "differ"
    for f in formats:
        ids = [str(i) for i, _ in sorted(bad[f])]
        print("%-16s %4d/%d %s%s" % (f, len(ids), args.count, label, (" first: " + " ".join(ids[:8])) if ids else ""))
        if args.oracle_only:
            for _, msg in sorted(bad[f])[:3]:
                print("    " + msg.splitlines()[0])
    if args.signatures and not args.oracle_only:
        counts = collections.Counter()
        example = {}
        for f in formats:
            for ident, sig in bad[f]:
                key = "%s: %s" % (f, sig)
                counts[key] += 1
                example.setdefault(key, ident)
        print("top signatures (count, first tree, format: oracle line | randoc line):")
        for key, n in counts.most_common(30):
            print("%5d  #%-4s %s" % (n, example[key], key))
    missing = [t for t in BLOCK_TYPES + INLINE_TYPES if t not in seen]
    print("node types not generated: " + (" ".join(missing) if missing else "none"))
    return 1 if any(bad.values()) or missing else 0


# ---------------------------------------------------------------- reduce

ITEM_LISTS = {"BulletList": lambda c: c, "OrderedList": lambda c: c[1], "DefinitionList": lambda c: c}


def is_node_list(value):
    return isinstance(value, list) and value and all(
        isinstance(v, dict) and v.get("t") in BLOCK_TYPES + INLINE_TYPES for v in value)


def child_node_lists(value):
    """The node lists inside one node, without the lists inside deeper nodes."""
    out = []

    def walk(v):
        if is_node_list(v):
            out.append(v)
        elif isinstance(v, list):
            for x in v:
                walk(x)
        elif isinstance(v, dict) and "t" not in v:
            for x in v.values():
                walk(x)

    walk(value.get("c"))
    return out


def all_lists(value, out):
    """Each node list and each item list in the tree, outermost first."""
    if is_node_list(value):
        out.append(("nodes", value))
    if isinstance(value, dict):
        if value.get("t") in ITEM_LISTS:
            out.append(("items", ITEM_LISTS[value["t"]](value["c"])))
        for v in value.values():
            all_lists(v, out)
    elif isinstance(value, list):
        for v in value:
            all_lists(v, out)
    return out


def kind_of(node):
    return "block" if node["t"] in BLOCK_TYPES else "inline"


def reduce_tree(tree, differs):
    """Greedy reduction: delete nodes and items, or replace a node by its children, while the outputs differ."""
    changed = True
    while changed:
        changed = False
        for _, lst in all_lists(tree, []):
            i = len(lst) - 1
            while i >= 0:
                if i >= len(lst):
                    i = len(lst) - 1
                    continue
                old = lst[i]
                candidates = [[]]
                if isinstance(old, dict) and "t" in old:
                    for sub in child_node_lists(old):
                        if all(kind_of(n) == kind_of(old) for n in sub):
                            candidates.append(list(sub))
                for repl in candidates:
                    lst[i:i + 1] = repl
                    if lst and differs(tree):
                        changed = True
                        break
                    lst[i:i + len(repl)] = [old]
                i -= 1
    return tree


def skeleton(value):
    if isinstance(value, dict):
        if "t" in value:
            subs = [skeleton(x) for x in child_node_lists(value)]
            inner = ",".join(s for s in subs if s)
            return value["t"] + ("[" + inner + "]" if inner else "")
        return ""
    if isinstance(value, list):
        return ",".join(s for s in (skeleton(v) for v in value) if s)
    return ""


def cmd_reduce(args):
    oracle = os.environ.get("RANDOC_ORACLE_PANDOC", "pandoc")
    fmt = args.format

    def outputs_differ(tree):
        text = json.dumps(tree, ensure_ascii=False)
        o = run([oracle, "-f", "json", "-t", fmt], text)
        if o[0] == 251 or "Heap exhausted" in o[2]:
            return False
        return o != run([args.randoc, "-f", "json", "-t", fmt], text)

    def job(ident):
        tree = json.loads(make_tree(args.seed, ident, args.no_math, args.small))
        if not outputs_differ(tree):
            return ident, None
        return ident, reduce_tree(tree, outputs_differ)

    counts = collections.Counter()
    example = {}
    with open(args.out, "w", encoding="utf-8") as fh, \
            concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for ident, tree in pool.map(job, range(args.count)):
            if tree is None:
                continue
            sk = skeleton(tree["blocks"])
            counts[sk] += 1
            example.setdefault(sk, ident)
            fh.write(json.dumps({"id": ident, "skeleton": sk, "tree": tree}, ensure_ascii=False) + "\n")
            fh.flush()
    print("%s: %d of %d trees differ" % (fmt, sum(counts.values()), args.count))
    for sk, n in counts.most_common():
        print("%5d  #%-4s %s" % (n, example[sk], sk[:150]))
    return 0


# ---------------------------------------------------------------- corpus

CORPUS_FORMATS = "markdown,gfm,commonmark,html,latex,json,native"


def corpus_signature(o, r):
    """The first differing line pair of two outcomes, with digits as N and words as W."""
    if o.stdout == r.stdout:
        return "exit %s vs %s / stderr differs" % (o.exit, r.exit)
    ol = o.stdout.decode(errors="replace").split("\n")
    rl = r.stdout.decode(errors="replace").split("\n")
    for i in range(max(len(ol), len(rl))):
        a = ol[i] if i < len(ol) else "<END>"
        b = rl[i] if i < len(rl) else "<END>"
        if a != b:
            norm = lambda t: re.sub(r"[A-Za-z]+", "W", re.sub(r"[0-9]+", "N", t))[:70]
            return "%r | %r" % (norm(a), norm(b))
    return "same lines"


def cmd_corpus(args):
    """Classifies each corpus document whose writer output differs.

    The writer runs on the oracle tree, so that a reader difference cannot
    hide or make a writer class. A document whose Randoc tree differs from
    the oracle tree is listed as a reader difference.
    """
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "tools", "corpus")
    sys.path.insert(0, os.path.normpath(root))
    import corpuslib
    import run as corpus_run

    corpus = os.path.abspath(os.environ["RANDOC_CORPUS"])
    oracle = os.path.abspath(os.environ.get("RANDOC_ORACLE_PANDOC", "pandoc"))
    randoc = os.path.abspath(args.randoc)
    formats = set(args.formats.split(","))
    documents = [d for d in corpuslib.load_manifest() if d.format in formats]
    os.makedirs(args.trees, exist_ok=True)
    home = corpus_run.HOME
    to = args.to

    def oracle_outcome(cache, document, cli):
        key = cache.key(document.sha256, cli)
        stored = cache.get(key)
        if stored is not None:
            return stored
        outcome = corpus_run.execute(oracle, cli, corpus, home, 60)
        cache.put(key, outcome)
        return outcome

    def job(document):
        cache = corpus_run.OracleCache(os.path.join(corpus, corpus_run.CACHE_NAME),
                                       corpus_run.fingerprint(oracle))
        try:
            json_cli = ["-f", document.format, "-t", "json", document.file_name]
            o_tree = oracle_outcome(cache, document, json_cli)
            o_out = oracle_outcome(cache, document, ["-f", document.format, "-t", to, document.file_name])
        finally:
            cache.close()
        if o_tree.exit != 0:
            return document, "oracle error", None
        tree_path = os.path.join(args.trees, document.name + ".json")
        with open(tree_path, "wb") as fh:
            fh.write(o_tree.stdout)
        r_out = corpus_run.execute(randoc, ["-f", "json", "-t", to, tree_path], corpus, home, 60)
        r_tree = corpus_run.execute(randoc, json_cli, corpus, home, 60)
        reader_same = r_tree.exit == 0 and corpus_run.json_equal(
            json.loads(o_tree.stdout), json.loads(r_tree.stdout))
        if (o_out.exit, o_out.stdout, o_out.stderr) == (r_out.exit, r_out.stdout, r_out.stderr):
            return document, "pass" if reader_same else "reader", None
        return document, "writer", corpus_signature(o_out, r_out)

    counts = collections.Counter()
    example = {}
    status = collections.Counter()
    readers = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for document, kind, sig in pool.map(job, documents):
            status[kind] += 1
            if kind == "reader":
                readers.append("%s (%s)" % (document.name, document.format))
            if sig is not None:
                counts[sig] += 1
                example.setdefault(sig, document.name)
    print("%s: %d documents; writer on the oracle tree: %s" % (
        to, len(documents), ", ".join("%s %d" % kv for kv in sorted(status.items()))))
    for sig, n in counts.most_common():
        print("%4d  %-40s %s" % (n, example[sig][:40], sig))
    for name in readers:
        print("reader difference only: %s" % name)
    return 0


# ---------------------------------------------------------------- cases

MD5 =["markdown", "gfm", "commonmark", "plain", "markdown_strict"]
MD6 = ["markdown", "gfm", "commonmark", "commonmark_x", "plain", "markdown_strict"]

FILES = {}
ORDER = []


def suite(name, comment, to, args=None):
    FILES[name] = {"comment": comment, "to": to, "args": args, "cases": []}
    ORDER.append(name)
    return lambda ident, blocks, to=None, args=None: FILES[name]["cases"].append((ident, blocks, to, args))


def toml_str(s):
    if "'" not in s and "\n" not in s:
        return "'%s'" % s
    if "'''" not in s and "\n" not in s and not s.endswith("'"):
        return "'''%s'''" % s
    return json.dumps(s)


def toml_list(xs):
    return "[" + ", ".join(json.dumps(x) for x in xs) + "]"


def slug(text):
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", text.lower())).strip("-")


ESC = [
    ("star", "*"), ("underscore", "_"), ("backtick", "`"), ("hash", "#"), ("plus", "+"), ("minus", "-"),
    ("one-dot", "1."), ("one-paren", "1)"), ("gt", ">"), ("pipe", "|"), ("lt", "<"), ("amp", "&"),
    ("dollar", "$"), ("caret", "^"), ("tilde", "~"), ("at", "@"), ("lbracket", "["), ("rbracket", "]"),
    ("bang", "!"), ("backslash", "\\"), ("squote", "'"), ("dquote", '"'), ("lbrace", "{"), ("equals", "="),
    ("colon", ":"), ("percent", "%"), ("tag", "<b>"), ("entity", "&amp;"), ("double-star", "**"),
    ("fence-backtick", "```"), ("fence-tilde", "~~~"), ("dashes", "---"), ("dollar-num", "$5"),
    ("snake", "a_b_c"), ("caret-pair", "2^3^"), ("tilde-pair", "H~2~O"), ("image-syntax", "![x]"),
    ("link-syntax", "[a](b)"), ("angle-url", "<http://e.x>"), ("escaped-star", "\\*"),
    ("letter-dot", "B."), ("citekey", "@doe99"), ("task", "[x]"), ("nbsp", "\u00a0"),
]

WRAP = [
    ("hash", "#"), ("hash2", "##"), ("plus", "+"), ("minus", "-"), ("star", "*"), ("one-dot", "1."),
    ("one-paren", "1)"), ("num-dot", "23."), ("gt", ">"), ("pipe", "|"), ("equals", "=="),
    ("fence-tilde", "~~~"), ("fence-backtick", "```"), ("dashes", "---"), ("stars", "***"),
    ("colon", ":"), ("percent", "%"), ("tag", "<div>"), ("note-def", "[^1]:"), ("ref-def", "[x]:"),
    ("letter-dot", "A."), ("roman-dot", "iv."), ("paren-num", "(1)"), ("example", "(@)"),
]

FILLER = " ".join(["abcd"] * 13 + ["abcdef"])  # 71 characters


def define_cases():
    # ---------------------------------------------------------- escaping
    c = suite("escaping", "Escaping of special characters in each position.", MD6)
    for name, tok in ESC:
        c("%s-alone" % name, [Para(Str(tok))])
        c("%s-start" % name, [Para(Str(tok), SP, Str("tail"))])
        c("%s-mid" % name, [Para(Str("lead"), SP, Str(tok), SP, Str("tail"))])
        c("%s-end" % name, [Para(Str("lead"), SP, Str(tok))])
        c("%s-glued" % name, [Para(Str("ab" + tok + "cd"))])
    ext = [
        ("dollars", "$x$ costs $5 and $", ["markdown", "markdown-tex_math_dollars", "gfm", "gfm-tex_math_dollars", "commonmark_x"]),
        ("supers", "a^b^c and ^", ["markdown", "markdown-superscript", "commonmark_x", "markdown_mmd"]),
        ("subs", "a~b~c and ~~d~~", ["markdown", "markdown-subscript", "markdown-strikeout", "markdown-subscript-strikeout", "gfm", "gfm-strikeout"]),
        ("cites", "@doe and [@doe] and a@b", ["markdown", "markdown-citations", "commonmark_x", "gfm"]),
        ("html-chars", "<span> & <x@y.z> &copy;", ["markdown", "markdown-raw_html", "gfm-raw_html", "commonmark-raw_html", "markdown_strict-raw_html"]),
        ("tex-chars", "\\alpha and \\(x\\) and \\[y\\]", ["markdown", "markdown-raw_tex", "markdown+tex_math_single_backslash", "markdown+tex_math_double_backslash"]),
        ("ascii-quotes", "\"q\" 'q' -- --- ... it's", ["markdown", "markdown-smart", "gfm", "gfm+smart", "commonmark+smart", "plain", "plain+smart"]),
        ("unicode-punct", "\u201ccurly\u201d \u2018single\u2019 \u2013 \u2014 \u2026 it\u2019s", ["markdown", "markdown-smart", "gfm", "gfm+smart", "commonmark+smart", "plain", "markdown_strict"]),
        ("pipe-para", "a|b | c", ["markdown", "markdown-pipe_tables", "gfm", "gfm-pipe_tables"]),
        ("bracket-span", "[text]{.cls}", ["markdown", "markdown-bracketed_spans", "commonmark_x", "gfm"]),
        ("div-fence", "::: note", ["markdown", "markdown-fenced_divs", "commonmark_x", "gfm"]),
        ("emoji-code", ":smile: and :x", ["gfm", "gfm-emoji", "markdown+emoji", "commonmark_x", "markdown"]),
        ("all-symbols", "a * b _ c \\ d < e", ["markdown", "markdown-all_symbols_escapable", "markdown+angle_brackets_escapable", "markdown_strict"]),
        ("intraword", "snake_case_name and a*b*c", ["markdown", "markdown-intraword_underscores", "markdown_strict", "gfm"]),
        ("wiki", "[[wiki page]] and [[a|b]]", ["markdown", "markdown+wikilinks_title_after_pipe", "gfm+wikilinks_title_before_pipe", "commonmark"]),
        ("mark", "==marked== text", ["markdown", "markdown+mark", "commonmark_x"]),
        ("pipe-row", "| a | b |", ["markdown", "markdown-pipe_tables", "gfm", "gfm-pipe_tables", "commonmark"]),
        ("title-block", "% Title", ["markdown", "markdown-pandoc_title_block", "markdown_mmd", "gfm"]),
        ("mmd-title", "Title: x", ["markdown_mmd", "markdown_mmd-mmd_title_block", "markdown"]),
        ("alert", "[!NOTE] text", ["gfm", "gfm-alerts", "markdown+alerts", "commonmark"]),
        ("fancy-letter", "a. item", ["markdown", "markdown-fancy_lists", "commonmark", "plain"]),
        ("example-marker", "(@) item", ["markdown", "markdown-example_lists", "commonmark"]),
        ("def-marker", ": definition", ["markdown", "markdown-definition_lists", "markdown_phpextra", "commonmark_x"]),
        ("lineblock-marker", "| line", ["markdown", "markdown-line_blocks", "gfm"]),
        ("short-subsup", "x^2 and y~1", ["markdown", "markdown+short_subsuperscripts", "markdown_mmd"]),
        ("abbrev", "*[HTML]: x", ["markdown", "markdown+abbreviations", "markdown_phpextra"]),
        ("old-dashes", "a-b--c", ["markdown", "markdown+old_dashes", "markdown-smart"]),
    ]
    for name, text, to in ext:
        c("ext-" + name, [Para(text)], to=to)
    c("ext-yaml-start", [Para("---"), Para("key: value")], to=["markdown", "markdown-yaml_metadata_block", "gfm", "commonmark"])
    c("ext-header-attr-text", [Para("Head {#id}")], to=["markdown", "markdown-header_attributes", "commonmark_x"])

    # ---------------------------------------------------------- wrapping
    c = suite("wrapping", "Special characters at the start of a wrapped line, after a soft break and after a hard break.", MD5)
    for name, tok in WRAP:
        c("wrap-" + name, [Para(FILLER + " " + tok + " tail words")])
        c("soft-" + name, [Para("short", SB, Str(tok), SP, Str("tail"))])
        c("hard-" + name, [Para("short", LB, Str(tok), SP, Str("tail"))])
    c("long-paragraph", [Para(" ".join([LONG] * 4))])
    c("long-word", [Para("a " + "x" * 90 + " b")])
    c("long-url-link", [Para("see ", Link("here", "http://example.invalid/" + "p" * 80), " now")])
    c("wrap-inside-emph", [Para(Emph(" ".join([LONG] * 2)))])
    c("wrap-inside-link-text", [Para(Link(" ".join([LONG] * 2), "u"))])
    c("wrap-nbsp", [Para(FILLER + " a\u00a0b\u00a0c\u00a0d tail")])
    c("wrap-in-list", [BL([" ".join([LONG] * 2), FILLER + " 1. x"])])
    c("wrap-in-quote", [BQ(Para(FILLER + " > quoted # hash"))])
    c("wrap-code-span", [Para(FILLER, " ", Code("a b c d e f"), " tail")])
    # A `%` at the start of a line needs a backslash only when it is a word of its own.
    for name, tok in [("percent-letter", "%x"), ("percent-nbsp", "%\u00a0x"), ("percent-twice", "%%"), ("percent-dot", "%.")]:
        c("start-" + name, [Para(Str(tok), SP, Str("b"))])
        c("hard-" + name, [Para("a", LB, Str(tok), SP, Str("b"))])
        c("soft-" + name, [Para("a", SB, Str(tok), SP, Str("b"))])

    # ---------------------------------------------------------- headings
    c = suite("headings", "Headings: levels, attributes, content.", MD6)
    for lvl in range(1, 7):
        c("level-%d" % lvl, [H(lvl, "Level text"), Para("body")])
    c("level-7", [H(7, "Deep"), Para("body")])
    c("empty", [H(2, []), Para("x")])
    c("id", [H(1, "Title", A("custom-id"))], to=MD6 + ["markdown-header_attributes", "markdown_mmd", "markdown_phpextra"])
    c("id-equals-auto", [H(1, "My Title", A("my-title"))], to=MD6 + ["markdown-auto_identifiers", "markdown+gfm_auto_identifiers"])
    c("id-equals-gfm-auto", [H(1, "My Title!", A("my-title"))], to=["markdown", "markdown+gfm_auto_identifiers", "gfm", "commonmark_x"])
    c("classes", [H(2, "Title", A("", ["a", "b"]))], to=MD6 + ["markdown-header_attributes"])
    c("kv", [H(2, "Title", A("i", ["c"], [("k", "v w")]))], to=MD6 + ["markdown-header_attributes"])
    c("unnumbered", [H(2, "Title", A("", ["unnumbered"]))])
    c("unnumbered-dash", [H(2, "Title", A("", ["-"]))])
    c("linebreak", [H(2, I("one", LB, "two"))])
    c("softbreak", [H(2, I("one", SB, "two"))])
    c("leading-hash", [H(2, "# not")])
    c("trailing-hash", [H(2, "Ends #")])
    c("trailing-hashes", [H(2, "C ##")])
    c("number-start", [H(3, "1. One")])
    c("emph", [H(1, I("A ", Emph("b"), " ", Strong("c")))])
    c("code", [H(1, I("Use ", Code("x`y")))])
    c("link", [H(1, I(Link("ref", "http://e.x")))])
    c("note", [H(1, I("Title", Note(Para("n"))))])
    c("image", [H(1, I(Image("alt", "i.png")))])
    c("long", [H(2, " ".join([LONG] * 2))])
    c("after-para", [Para("text"), H(2, "Next")])
    c("consecutive", [H(1, "A"), H(2, "B"), H(3, "C")])
    c("in-list", [BL([[H(2, "In item"), Para("x")]])])
    c("in-quote", [BQ(H(2, "Quoted"), Para("x"))])
    c("in-div", [Div(A("d"), H(2, "Inside"))], to=MD6 + ["markdown-fenced_divs"])
    c("equals-line", [H(1, "==="), Para("x")])
    c("dash-text", [H(2, "- item")])
    c("nbsp", [H(2, "a\u00a0b")])
    c("braces", [H(2, "Text {#x}")])

    # The automatic identifier of a heading counts the headings before it.
    ids = {
        "repeat": [("A", "a"), ("A", "a-1"), ("A", "a-2")],
        "gap": [("A", "a"), ("A", "a-2")],
        "explicit-first": [("A", "x"), ("A", "a")],
        "suffix-first": [("A", "a-1"), ("A", "a")],
        "taken-by-other": [("A", "a"), ("B", "a"), ("A", "a-1")],
        "other-first": [("B", "a"), ("A", "a")],
    }
    for name, hs in ids.items():
        c("unique-id-" + name, [H(2, t, A(i)) for t, i in hs], to=["markdown", "gfm", "markdown_phpextra"])
    c("unique-id-after-div", [Div(A("a"), Para("x")), H(2, "A", A("a"))], to=["markdown"])
    c("auto-id-with-class", [H(2, "A", A("a", ["c"]))], to=["markdown", "markdown_phpextra"])
    c("auto-id-with-kv", [H(2, "A", A("a", [], [("k", "v")]))], to=["markdown", "markdown_phpextra"])

    # ---------------------------------------------------------- blocks
    c = suite("blocks", "Paragraphs, plain blocks, block quotes, line blocks, rules, raw blocks.", MD6)
    c("empty-doc", [])
    c("empty-para", [Para()])
    c("empty-plain", [Plain()])
    c("space-only-para", [Para(SP)])
    c("para-para", [Para("one"), Para("two")])
    c("plain-plain", [Plain("one"), Plain("two")])
    c("plain-para", [Plain("one"), Para("two")])
    c("para-plain", [Para("one"), Plain("two")])
    c("leading-space-str", [Para(Str("  lead"))])
    c("trailing-space", [Para("end", SP)])
    c("leading-space", [Para(SP, "start")])
    c("quote", [BQ(Para("quoted"))])
    c("quote-two-paras", [BQ(Para("one"), Para("two"))])
    c("quote-nested", [BQ(BQ(Para("deep")))])
    c("quote-nested3", [BQ(Para("a"), BQ(Para("b"), BQ(Para("c"))))])
    c("quote-list", [BQ(BL(["a", "b"]), OL(["c"]))])
    c("quote-code", [BQ(CodeBlock("x = 1"))])
    c("quote-heading-hr", [BQ(H(2, "H"), HR, Para("p"))])
    c("quote-empty", [BQ()])
    c("quote-after-list", [BL(["a"]), BQ(Para("q"))])
    c("quote-plain", [BQ(Plain("bare"))])
    c("quotes-adjacent", [BQ(Para("one")), BQ(Para("two"))])
    c("lineblock", [LineBlock("first", "second")], to=MD6 + ["markdown-line_blocks"])
    c("lineblock-indent", [LineBlock("\u00a0\u00a0indented", "flush")], to=MD6 + ["markdown-line_blocks"])
    c("lineblock-empty-line", [LineBlock("a", [], "b")], to=MD6 + ["markdown-line_blocks"])
    c("lineblock-long", [LineBlock(" ".join([LONG] * 2), "x")])
    c("lineblock-special", [LineBlock("# h", "- b", "1. n")], to=MD6 + ["markdown-line_blocks"])
    c("lineblock-emph", [LineBlock(I(Emph("e"), " and ", Strong("s")))])
    c("hr", [Para("a"), HR, Para("b")])
    c("hr-first", [HR, Para("b")])
    c("hr-only", [HR])
    c("hr-in-list", [BL([[HR], "x"])])
    c("hr-in-quote", [BQ(HR)])
    c("hr-after-plain", [Plain("a"), HR])
    for fmt in ["html", "html5", "tex", "latex", "markdown", "rst", "openxml", "gfm", "commonmark", "plain", "markdown_strict"]:
        c("raw-block-" + fmt, [Para("a"), RawB(fmt, "<p>x</p>" if "html" in fmt else "\\x{y}" if "tex" in fmt else "*raw* text"), Para("b")],
          to=MD6 + ["markdown-raw_html", "markdown-raw_tex", "markdown-raw_attribute"])
    c("raw-html-blank-lines", [RawB("html", "<div>\n\nx\n\n</div>")], to=MD6 + ["markdown-raw_html"])
    c("raw-html-inline-tag", [RawB("html", "<b>")], to=MD6 + ["markdown-raw_html"])
    c("raw-latex-env", [RawB("latex", "\\begin{center}\nx\n\\end{center}")], to=MD6 + ["markdown-raw_tex", "markdown-raw_tex-raw_attribute"])
    c("raw-block-in-list", [BL([[RawB("html", "<hr>")], "x"])], to=MD6 + ["markdown-raw_html"])

    # Raw blocks: the text with its own trailing newlines, then the separator.
    rto = ["markdown", "gfm", "commonmark", "commonmark_x", "markdown_strict", "plain"]
    nexts = [("raw", RawB("html", "<j>")), ("raw-nl", RawB("html", "<j>\n")), ("raw-tex", RawB("latex", "\\j")),
             ("plain", Plain("a")), ("para", Para("a")), ("end", None)]
    for fmt in ["html", "latex"]:
        for tname, text in [("empty", ""), ("one", "<i>"), ("nl", "<i>\n"), ("two-nl", "<i>\n\n"), ("lines", "a\nb\n")]:
            for nname, nxt in nexts:
                c("raw-%s-%s-then-%s" % (fmt, tname, nname), [Para("z"), RawB(fmt, text)] + ([nxt] if nxt else []), to=rto)

    # ---------------------------------------------------------- code
    c = suite("code", "Code blocks and code spans.", MD6 + ["markdown-fenced_code_blocks-backtick_code_blocks"])
    c("block", [CodeBlock("x = 1")])
    c("block-class", [CodeBlock("x = 1", A("", ["python"]))])
    c("block-classes", [CodeBlock("x", A("", ["python", "numberLines"]))])
    c("block-id", [CodeBlock("x", A("c1"))])
    c("block-kv", [CodeBlock("x", A("c1", ["rust"], [("startFrom", "5")]))])
    c("block-empty", [CodeBlock("")])
    c("block-blank-lines", [CodeBlock("a\n\n\nb")])
    c("block-trailing-newline", [CodeBlock("a\n")])
    c("block-leading-spaces", [CodeBlock("    deep\nflat")])
    c("block-tabs", [CodeBlock("\tx\n\t\ty")])
    c("block-backticks", [CodeBlock("```\ninner\n```")])
    c("block-backticks4", [CodeBlock("````\ninner\n````", A("", ["md"]))])
    c("block-tildes", [CodeBlock("~~~\ninner\n~~~", A("", ["md"]))])
    c("block-both-fences", [CodeBlock("```\n~~~~\nx", A("", ["md"]))])
    c("block-in-list", [BL([[Para("item"), CodeBlock("code")]])])
    c("block-first-in-item", [BL([[CodeBlock("code")]])])
    c("block-in-ordered", [OL([[Para("item"), CodeBlock("code")]])])
    c("block-after-list", [BL(["a"]), CodeBlock("code")])
    c("block-in-quote", [BQ(CodeBlock("x", A("", ["c"])))])
    c("block-after-para", [Para("p"), CodeBlock("x")])
    c("block-html-chars", [CodeBlock("<a> & </a>")])
    c("block-only-class-attr", [CodeBlock("x", A("", [], [("k", "v")]))])
    c("block-attrs-ext", [CodeBlock("x", A("i", ["py"]))], to=["markdown", "markdown-fenced_code_attributes", "markdown-backtick_code_blocks", "markdown-fenced_code_blocks", "gfm", "commonmark_x"])
    c("span", [Para("use ", Code("x"), " now")])
    c("span-backtick", [Para(Code("a`b"))])
    c("span-two-backticks", [Para(Code("a``b"))])
    c("span-start-backtick", [Para(Code("`a"))])
    c("span-end-backtick", [Para(Code("a`"))])
    c("span-only-backticks", [Para(Code("```"))])
    c("span-spaces", [Para(Code(" a "))])
    c("span-space-only", [Para(Code(" "))])
    c("span-empty", [Para("e", Code(""), "e")])
    c("span-attr", [Para(Code("x", A("i", ["c"], [("k", "v")])))], to=MD6 + ["markdown-inline_code_attributes"])
    c("span-class", [Para(Code("x", A("", ["haskell"])))], to=MD6 + ["markdown-inline_code_attributes"])
    c("span-html", [Para(Code("<b>&amp;"))])
    c("span-long", [Para(FILLER, " ", Code(" ".join(["word"] * 12)))])
    c("span-backslash", [Para(Code("a\\b\\"))])
    c("span-newline", [Para(Code("a\nb"))])
    c("span-pipe-in-table", [Table([["h"]], [[[Plain(Code("a|b"))]]])])
    c("span-in-emph", [Para(Emph(Code("c")))])
    c("span-adjacent", [Para(Code("a"), Code("b"))])

    # ---------------------------------------------------------- lists
    c = suite("lists", "Bullet, ordered, task, example and definition lists.", MD5)
    c("bullet-tight", [BL(["one", "two", "three"])])
    c("bullet-loose", [BL([[Para("one")], [Para("two")]])])
    c("bullet-mixed", [BL([[Para("one")], "two"])])
    c("bullet-multi-block", [BL([[Para("one"), Para("more")], "two"])])
    c("bullet-nested", [BL([[Plain("a"), BL(["b", "c"])], "d"])])
    c("bullet-nested3", [BL([[Plain("a"), BL([[Plain("b"), BL([[Plain("c"), BL(["d"])]])]])]])])
    c("bullet-nested5", [BL([[Plain("1"), BL([[Plain("2"), BL([[Plain("3"), BL([[Plain("4"), BL(["5"])]])]])]])]])])
    c("bullet-empty-item", [BL([[], "x"])])
    c("bullet-empty", [BL([])])
    c("bullet-heading-first", [BL([[H(3, "h"), Para("p")]])])
    c("bullet-quote", [BL([[BQ(Para("q"))]])])
    c("bullet-table", [BL([[Table([["a", "b"]], [["1", "2"]])]])])
    c("bullet-long", [BL([" ".join([LONG] * 2)])])
    c("bullet-after-bullet", [BL(["a"]), BL(["b"])])
    c("bullet-after-ordered", [OL(["a"]), BL(["b"])])
    c("ordered-after-ordered", [OL(["a"]), OL(["b"])])
    c("ordered-after-bullet", [BL(["a"]), OL(["b"])])
    c("ordered-tight", [OL(["one", "two"])], to=MD5 + ["markdown-fancy_lists"])
    c("ordered-loose", [OL([[Para("one")], [Para("two")]])])
    c("ordered-nested", [OL([[Plain("a"), OL(["b"], style="LowerAlpha")], "c"])], to=MD5 + ["markdown-fancy_lists"])
    c("ordered-ten-items", [OL(["i%d" % n for n in range(1, 12)])])
    c("ordered-ten-nested", [OL(["x"] * 9 + [[Plain("ten"), BL(["sub"])]])])
    for start in [0, 2, 9, 10, 100, -3]:
        c("ordered-start-%s" % str(start).replace("-", "neg"), [OL(["a", "b"], start=start)], to=MD5 + ["markdown-startnum", "commonmark_x"])
    for style in STYLES:
        for delim in DELIMS:
            c("ordered-%s-%s" % (slug(style), slug(delim)), [OL(["a", "b"], start=3 if style != "Example" else 1, style=style, delim=delim)],
              to=MD5 + ["markdown-fancy_lists", "commonmark_x"])
    c("task-bullet", [BL(["\u2610 open", "\u2612 done"])], to=MD5 + ["markdown-task_lists", "commonmark_x"])
    c("task-str-split", [BL([[Plain(Str("\u2610"), SP, Str("open"))]])], to=MD5 + ["markdown-task_lists"])
    c("task-ordered", [OL(["\u2610 a", "\u2612 b"])], to=MD5 + ["markdown-task_lists"])
    c("task-no-space", [BL(["\u2610open"])], to=MD5 + ["markdown-task_lists"])
    c("task-not-first", [BL(["x \u2610"])], to=MD5 + ["markdown-task_lists"])
    c("task-para", [BL([[Para("\u2612 done"), Para("more")]])], to=MD5 + ["markdown-task_lists"])
    c("task-mixed", [BL(["\u2610 a", "plain"])], to=MD5 + ["markdown-task_lists"])
    c("example-one", [OL(["first", "second"], style="Example", delim="TwoParens")], to=MD5 + ["markdown-example_lists"])
    c("example-two-lists", [OL(["a"], style="Example"), Para("text"), OL(["b"], start=2, style="Example")], to=MD5 + ["markdown-example_lists"])
    dl_to = ["markdown", "markdown_phpextra", "markdown_mmd", "gfm", "commonmark_x", "plain", "markdown-definition_lists", "markdown_strict"]
    c("def-tight", [DL([("term", ["def"])])], to=dl_to)
    c("def-loose", [DL([("term", [[Para("def")]])])], to=dl_to)
    c("def-multi-defs", [DL([("term", ["one", "two"])])], to=dl_to)
    c("def-multi-terms", [DL([("a", ["x"]), ("b", ["y"])])], to=dl_to)
    c("def-term-format", [DL([(I(Emph("t"), " ", Code("c")), ["d"])])], to=dl_to)
    c("def-code", [DL([("t", [[Para("p"), CodeBlock("x")]])])], to=dl_to)
    c("def-list-inside", [DL([("t", [[Plain("p"), BL(["a", "b"])]])])], to=dl_to)
    c("def-empty-defs", [DL([("t", [])])], to=dl_to)
    c("def-long", [DL([("t", [" ".join([LONG] * 2)])])], to=dl_to)
    c("def-nested", [DL([("outer", [[Plain("x"), DL([("inner", ["y"])])]])])], to=dl_to)
    c("four-space-rule", [BL([[Para("a"), Para("b")]]), OL([[Para("c"), CodeBlock("d")]])], to=["markdown", "markdown+four_space_rule", "gfm", "commonmark"])
    c("list-after-para-no-blank", [Para("text"), BL(["a"])], to=MD5 + ["markdown+lists_without_preceding_blankline"])
    c("item-starts-with-number", [BL(["1. not ordered"])])
    c("item-code-then-text", [BL([[CodeBlock("c"), Para("p")]])])

    # ---------------------------------------------------------- tables
    tto = ["markdown", "gfm", "commonmark", "plain", "markdown_strict", "markdown-simple_tables",
           "markdown-simple_tables-multiline_tables", "markdown-simple_tables-multiline_tables-grid_tables",
           "markdown-simple_tables-multiline_tables-grid_tables-pipe_tables", "markdown_phpextra"]
    c = suite("tables", "Table forms and the choice between them.", tto)
    L, R, C, D = "AlignLeft", "AlignRight", "AlignCenter", "AlignDefault"
    c("simple", [Table([["a", "b"]], [["1", "2"], ["3", "4"]])])
    c("aligns", [Table([["l", "r", "c", "d"]], [["1", "2", "3", "4"]], [L, R, C, D])])
    c("no-header", [Table([], [["1", "2"], ["3", "4"]])])
    c("no-header-aligns", [Table([], [["1", "2"]], [R, C])])
    c("empty-header-cells", [Table([["", ""]], [["1", "2"]])])
    c("widths", [Table([["a", "b"]], [["1", "2"]], widths=[0.3, 0.7])])
    c("widths-aligns", [Table([["a", "b"]], [["1", "2"]], [R, C], widths=[0.5, 0.5])])
    c("long-cells", [Table([["name", "description"]], [["x", " ".join([LONG] * 2)]])])
    c("long-cells-widths", [Table([["name", "description"]], [["x", " ".join([LONG] * 2)]], widths=[0.2, 0.8])])
    c("caption", [Table([["a"]], [["1"]], caption="The caption")])
    c("caption-long", [Table([["a"]], [["1"]], caption=" ".join([LONG] * 2))])
    c("caption-widths", [Table([["a"]], [["1"]], widths=[0.5], caption="Cap")])
    c("short-caption", [Table([["a"]], [["1"]], caption="Long", short="S")])
    c("multi-block-cell", [Table([["a", "b"]], [[[Para("p1"), Para("p2")], "x"]])])
    c("list-in-cell", [Table([["a"]], [[[BL(["i", "j"])]]])])
    c("code-in-cell", [Table([["a"]], [[[CodeBlock("x = 1")]]])])
    c("linebreak-in-cell", [Table([["a"]], [[[Plain("one", LB, "two")]]])])
    c("pipe-in-cell", [Table([["a|b"]], [["c | d"]])])
    c("colspan", [Table([["a", "b"]], [[cell("wide", cs=2)], ["1", "2"]])])
    c("rowspan", [Table([["a", "b"]], [[cell("tall", rs=2), "1"], ["2"]])])
    c("row-head", [Table([["k", "v"]], [["a", "1"], ["b", "2"]], rowhead=1)])
    c("foot", [Table([["a"]], [["1"]], foot=[["sum"]])])
    c("attr-id", [Table([["a"]], [["1"]], attr=A("tbl"), caption="C")])
    c("single-cell", [Table([], [["only"]])])
    c("empty-body", [Table([["a", "b"]], [])])
    c("cell-align", [Table([["a"]], [[cell("x", align=R)]])])
    c("emph-cells", [Table([[[Plain(Strong("h"))]]], [[[Plain(Emph("e"), " ", Code("c"))]]])])
    c("note-in-cell", [Table([["a"]], [[[Plain("x", Note(Para("n")))]]])])
    c("wide-many-columns", [Table([["k%d" % i for i in range(9)]], [["value%d" % i for i in range(9)]])])
    c("unicode-width", [Table([["\u65e5\u672c", "b"]], [["\u00e9t\u00e9", "\U0001F600"]])])
    c("gfm-variants", [Table([["a", "b"]], [["1", "2"]], [L, R])], to=["gfm", "gfm-pipe_tables", "commonmark_x", "commonmark_x-pipe_tables", "markdown_mmd", "markdown_github"])
    c("gfm-multi-block", [Table([["a"]], [[[Para("p1"), Para("p2")]]])], to=["gfm", "gfm-pipe_tables", "commonmark_x", "markdown_mmd"])
    c("gfm-caption", [Table([["a"]], [["1"]], caption="Cap")], to=["gfm", "commonmark_x", "markdown_mmd", "markdown-table_captions"])
    c("list-tables", [Table([["a"]], [["1"]])], to=["markdown", "markdown_mmd", "markua"])

    # A table, then one more block: one blank line between them in each form.
    wide = "a cell text that is long " * 4
    follow = {
        "simple": Table([["h1", "h2"]], [["a", "b"]]),
        "simple-no-head": Table([], [["a", "b"]]),
        "simple-caption": Table([["h1", "h2"]], [["a", "b"]], caption="Cap"),
        "multiline": Table([["h1", "h2"]], [[wide, "b"]]),
        "multiline-widths": Table([["h1", "h2"]], [["a", "b"]], widths=[0.7, 0.3]),
        "multiline-caption": Table([["h1", "h2"]], [[wide, "b"]], caption="Cap"),
        "grid": Table([["h1", "h2"]], [[[Para("a"), Para("b")], "c"]]),
        "pipe": Table([["h1", "h2"]], [["a", "b"]]),
    }
    for name, table in follow.items():
        to = ["markdown", "markdown-simple_tables", "gfm", "plain"] if name == "pipe" else ["markdown", "plain"]
        for nname, nxt in [("para", Para("next")), ("heading", H(2, "Next")), ("table", table)]:
            c("then-%s-%s" % (nname, name), [table, nxt], to=to)

    # ---------------------------------------------------------- table form matrix
    fto = ["markdown", "markdown-simple_tables", "markdown-simple_tables-multiline_tables",
           "markdown-simple_tables-multiline_tables-grid_tables",
           "markdown-simple_tables-multiline_tables-grid_tables-pipe_tables",
           "markdown-pipe_tables", "markdown-multiline_tables-grid_tables", "gfm", "commonmark", "plain"]
    c = suite("table-forms", "The choice of the table form: content kinds x alignments x widths x caption x header.", fto)
    kinds = {
        "short": lambda: [["ab", "c d"], ["e", "fg"]],
        "long": lambda: [["k", " ".join([LONG] * 2)], ["m", "n"]],
        "wide": lambda: [["p" * 40, "q" * 40], ["r", "s"]],
        "para": lambda: [[[Para("ab")], [Para("cd")]], ["e", "f"]],
        "two-paras": lambda: [[[Para("p1"), Para("p2")], "x"], ["y", "z"]],
        "list": lambda: [[[BL(["i", "j"])], "x"], ["y", "z"]],
        "code": lambda: [[[CodeBlock("v = 1")], "x"], ["y", "z"]],
        "break": lambda: [[[Plain("one", LB, "two")], "x"], ["y", "z"]],
        "empty": lambda: [["", ""], ["", ""]],
        "pipe-char": lambda: [["a|b", "c"], ["d", "e | f"]],
    }
    for kind, body in kinds.items():
        for al_name, al in (("default", None), ("lr", [L, R])):
            for w_name, w in (("auto", None), ("w37", [0.3, 0.7])):
                for cap_name, cap in (("", None), ("-cap", "Cap")):
                    for h_name, head in (("", [["h1", "h2"]]), ("-nohead", [])):
                        c("%s-%s-%s%s%s" % (kind, al_name, w_name, cap_name, h_name),
                          [Table(head, body(), al, w, cap)])

    # ---------------------------------------------------------- edge matrix
    c = suite("edges", "Spaces at the edges of inline containers, spaces before list markers, block separation.",
              ["markdown", "gfm", "commonmark", "plain"])
    containers = {
        "emph": lambda *x: {"t": "Emph", "c": list(x)},
        "strong": lambda *x: {"t": "Strong", "c": list(x)},
        "strike": lambda *x: {"t": "Strikeout", "c": list(x)},
        "underline": lambda *x: {"t": "Underline", "c": list(x)},
        "smallcaps": lambda *x: {"t": "SmallCaps", "c": list(x)},
        "super": lambda *x: {"t": "Superscript", "c": list(x)},
        "sub": lambda *x: {"t": "Subscript", "c": list(x)},
        "span": lambda *x: Span(A("", ["c"]), *x),
        "quoted": lambda *x: Q2(*x),
        "link": lambda *x: Link(list(x), "u"),
    }
    strong = lambda *x: {"t": "Strong", "c": list(x)}
    contents = {
        "spaced": lambda: [SP, Str("x"), SP],
        "softs": lambda: [SB, Str("x"), SB],
        "space-only": lambda: [SP],
        "soft-only": lambda: [SB],
        "empty": lambda: [],
        "nested-spaced": lambda: [strong(SP, Str("x"), SP)],
        "nested-space-only": lambda: [strong(SP)],
        "nested-empty": lambda: [strong()],
        "lead-nested": lambda: [strong(SP, Str("x")), Str("y")],
    }
    for cname, make in containers.items():
        for kname, content in contents.items():
            c("%s-%s-mid" % (cname, kname), [Para(Str("a"), make(*content()), Str("b"))])
            c("%s-%s-start" % (cname, kname), [Para(make(*content()), Str("b"))])
    for name, tok in [("dash", "-"), ("plus", "+"), ("one-dot", "1."), ("one-paren", "1)"), ("nine-dot", "9."),
                      ("year", "1986."), ("letter", "p."), ("parens", "(1)"), ("roman", "iv."), ("hash-dot", "#."),
                      ("star", "*"), ("hash", "#"), ("gt", ">")]:
        c("marker-%s-space-word" % name, [Para(SP, Str(tok), SP, Str("x"))])
        c("marker-%s-soft-end" % name, [Plain(SB, Str(tok))])
        c("marker-%s-space-end" % name, [Plain(SP, Str(tok))])
        c("marker-%s-wrapped" % name, [Para(Str("w " * 34 + "w"), SP, Str(tok), SP, Str("tail"))])
    prevs = {"para": Para("a"), "plain": Plain("a"), "raw-tex": RawB("latex", "\\x"), "raw-html": RawB("html", "<i>"),
             "raw-md": RawB("markdown", "# m"), "code": CodeBlock("c"), "rule": HR, "quote": BQ(Para("q"))}
    for pname, prev in prevs.items():
        c("join-%s-plain" % pname, [prev, Plain("b")])
        c("join-%s-para" % pname, [prev, Para("b")])
        c("join-%s-after-plain" % pname, [Plain("b"), prev])

    for n in [0, -1, 3999, 4000]:
        for style in ["LowerRoman", "UpperRoman", "LowerAlpha", "UpperAlpha"]:
            c("number-%s-%s" % (style.lower(), str(n).replace("-", "minus")),
              [OL(["a", "b"], start=n, style=style)], to=["markdown", "commonmark", "plain"])
    cites = {
        "in-text-prefix": [cit("k", "AuthorInText", "see", "")],
        "in-text-suffix": [cit("k", "AuthorInText", "", ", p. 2")],
        "in-text-both": [cit("k", "AuthorInText", "see", ", p. 2")],
        "in-text-two": [cit("k", "AuthorInText", "", ","), cit("j", "AuthorInText", "see", "")],
        "in-text-normal": [cit("k", "AuthorInText", "", ""), cit("j", "NormalCitation", "see", "x")],
        "normal-in-text": [cit("k", "NormalCitation", "see", ""), cit("j", "AuthorInText", "", ", p")],
        "suppress-two": [cit("k", "SuppressAuthor", "", ""), cit("j", "SuppressAuthor", "see", "")],
        "in-text-suppress": [cit("k", "AuthorInText", "", ""), cit("j", "SuppressAuthor", "", "")],
    }
    for name, cs in cites.items():
        c("cite-" + name, [Para(Cite(cs, Str("[x]")))], to=["markdown", "commonmark_x", "plain"])
    wide_attr = A("id1", ["c1", "c2"], [("k", "v"), ("w", "50%")])
    fill = Str("w " * 28 + "w")
    for name, el in [("link", Link([Str("t")], "u", attr=wide_attr)), ("span", Span(wide_attr, Str("t"))),
                     ("code", Code("t", wide_attr)), ("image", Image([Str("t")], "i.png", attr=wide_attr))]:
        c("attr-wrap-" + name, [Para(fill, SP, el, SP, Str("end"))], to=["markdown", "commonmark_x", "gfm"])

    # ---------------------------------------------------------- figures, divs, spans
    c = suite("figures-divs-spans", "Figures, implicit figures, divs and spans.", MD6 + ["markdown-implicit_figures"])
    c("implicit-figure", [Fig(NA, [Plain("cap")], Plain(Image("cap", "f.png")))])
    c("implicit-figure-title", [Fig(NA, [Plain("cap")], Plain(Image("cap", "f.png", "ti")))])
    c("implicit-figure-id", [Fig(A("fig1"), [Plain("cap")], Plain(Image("cap", "f.png")))])
    c("implicit-figure-image-attr", [Fig(NA, [Plain("cap")], Plain(Image("cap", "f.png", attr=A("i", [], [("width", "50%")]))))])
    c("figure-caption-differs", [Fig(NA, [Plain("caption")], Plain(Image("alt", "f.png")))])
    c("figure-no-caption", [Fig(NA, [], Plain(Image("alt", "f.png")))])
    c("figure-empty-alt", [Fig(NA, [Plain("cap")], Plain(Image([], "f.png")))])
    c("figure-para-image", [Fig(NA, [Plain("cap")], Para(Image("cap", "f.png")))])
    c("figure-two-images", [Fig(NA, [Plain("cap")], Plain(Image("a", "a.png"), SP, Image("b", "b.png")))])
    c("figure-text", [Fig(A("f"), [Plain("cap")], Para("just text"))])
    c("figure-short-caption", [Fig(NA, [Plain("cap")], Plain(Image("cap", "f.png")), short="S")])
    c("figure-multi-para-caption", [Fig(NA, [Para("one"), Para("two")], Plain(Image("one", "f.png")))])
    c("figure-classes", [Fig(A("", ["wide"]), [Plain("cap")], Plain(Image("cap", "f.png")))])
    c("figure-nested", [Fig(NA, [Plain("outer")], Fig(NA, [Plain("inner")], Plain(Image("inner", "f.png"))))])
    c("image-alone-para", [Para(Image("alt", "f.png"))])
    c("image-fig-title", [Para(Image("alt", "f.png", "fig:"))])
    div_to = MD6 + ["markdown-fenced_divs", "markdown-fenced_divs-native_divs", "markdown-fenced_divs-native_divs-raw_html"]
    c("div", [Div(NA, Para("in"))], to=div_to)
    c("div-id", [Div(A("d1"), Para("in"))], to=div_to)
    c("div-class", [Div(A("", ["note"]), Para("in"))], to=div_to)
    c("div-classes-kv", [Div(A("d", ["a", "b"], [("k", "v"), ("t", 'q "x"')]), Para("in"))], to=div_to)
    c("div-nested", [Div(A("o"), Para("a"), Div(A("i"), Para("b")))], to=div_to)
    c("div-nested3", [Div(A("a"), Div(A("b"), Div(A("c"), Para("x"))))], to=div_to)
    c("div-empty", [Div(A("e"))], to=div_to)
    c("div-list", [Div(A("d"), BL(["a", "b"]))], to=div_to)
    c("div-in-list", [BL([[Div(A("d"), Para("x"))]])], to=div_to)
    c("div-plain", [Div(A("d"), Plain("bare"))], to=div_to)
    c("div-section", [Div(A("s", ["section"]), H(2, "H"), Para("p"))], to=div_to)
    c("div-cell-wrapper", [Div(A("", ["cell"]), CodeBlock("x"))], to=div_to)
    span_to = MD6 + ["markdown-bracketed_spans", "markdown-bracketed_spans-native_spans", "markdown-bracketed_spans-native_spans-raw_html"]
    c("span", [Para("a ", Span(NA, "s"), " b")], to=span_to)
    c("span-id", [Para(Span(A("s1"), "s"))], to=span_to)
    c("span-class", [Para(Span(A("", ["c"]), "s"))], to=span_to)
    c("span-kv", [Para(Span(A("i", ["c"], [("k", "v")]), "s"))], to=span_to)
    c("span-nested", [Para(Span(A("o"), "a ", Span(A("i"), "b")))], to=span_to)
    c("span-empty", [Para("x", Span(A("e")), "y")], to=span_to)
    c("span-smallcaps-class", [Para(Span(A("", ["smallcaps"]), "sc"))], to=span_to)
    c("span-underline-class", [Para(Span(A("", ["underline"]), "u"))], to=span_to)
    c("span-mark-class", [Para(Span(A("", ["mark"]), "m"))], to=span_to + ["markdown+mark"])
    c("span-brackets", [Para(Span(A("", ["c"]), "[x]"))], to=span_to)

    # The fence of a div counts each div inside it, through divs only.
    dv = lambda *bs: Div(A("", ["c"]), *bs)
    px = Para("x")
    fences = {
        "one": dv(dv(px)), "two": dv(dv(px), dv(px)), "three": dv(dv(px), dv(px), dv(px)),
        "nested": dv(dv(dv(px))), "nested-and-sibling": dv(dv(dv(px)), dv(px)), "between-para": dv(dv(px), px, dv(px)),
        "in-list": dv(BL([[dv(px)], [dv(px)]])), "in-quote": dv(BQ(dv(px), dv(px))),
        "after-list": dv(BL([[dv(px)]]), dv(px)),
        "in-note": dv(Para(Note(dv(px)))),
    }
    for name, tree in fences.items():
        c("fence-count-" + name, [tree], to=["markdown", "commonmark_x"])
    c("fence-count-top-siblings", [dv(dv(px)), dv(px)], to=["markdown", "commonmark_x"])

    # ---------------------------------------------------------- links and images
    c = suite("links-images", "Links, autolinks and images.", MD6 + ["markdown-link_attributes"])
    c("inline", [Para(Link("text", "http://e.x/p"))])
    c("title", [Para(Link("text", "http://e.x", "The title"))])
    c("title-quotes", [Para(Link("text", "u", 'say "hi"'))])
    c("title-single-quote", [Para(Link("text", "u", "it's"))])
    c("url-spaces", [Para(Link("text", "a b.html"))])
    c("url-parens", [Para(Link("text", "/p(1)"))])
    c("url-unbalanced", [Para(Link("text", "/p(1"))])
    c("url-angle", [Para(Link("text", "a<b>c"))])
    c("url-empty", [Para(Link("text", ""))])
    c("text-empty", [Para(Link([], "http://e.x"))])
    c("autolink", [Para(Link("http://e.x/p", "http://e.x/p"))])
    c("autolink-uri-class", [Para(Link("http://e.x", "http://e.x", attr=A("", ["uri"])))])
    c("autolink-trailing-text", [Para("see ", Link("http://e.x", "http://e.x"), ".")])
    c("autolink-with-title", [Para(Link("http://e.x", "http://e.x", "t"))])
    c("email", [Para(Link("p@e.x", "mailto:p@e.x"))])
    c("email-class", [Para(Link("p@e.x", "mailto:p@e.x", attr=A("", ["email"])))])
    c("email-text-differs", [Para(Link("mail me", "mailto:p@e.x"))])
    c("text-url-no-scheme", [Para(Link("e.x", "http://e.x"))])
    c("attr", [Para(Link("t", "u", attr=A("l1", ["c"], [("k", "v")])))])
    c("attr-target", [Para(Link("t", "u", attr=A("", [], [("target", "_blank")])))])
    c("brackets-text", [Para(Link("[x]", "u"))])
    c("emph-text", [Para(Link(I(Emph("e"), " ", Strong("s")), "u"))])
    c("code-text", [Para(Link(Code("c"), "u"))])
    c("image-in-link", [Para(Link(Image("alt", "i.png"), "u"))])
    c("link-in-emph", [Para(Emph(Link("t", "u")))])
    c("fragment", [Para(Link("sec", "#sec"))])
    c("relative", [Para(Link("doc", "doc.md"))])
    c("special-url", [Para(Link("q", "http://e.x/?a=1&b=2#c"))])
    c("url-backslash", [Para(Link("q", "a\\b"))])
    c("wikilink", [Para(Link("Page", "Page", "wikilink"))], to=["markdown", "markdown+wikilinks_title_after_pipe", "markdown+wikilinks_title_before_pipe", "gfm", "commonmark_x"])
    c("wikilink-text", [Para(Link("text", "Page", "wikilink"))], to=["markdown", "markdown+wikilinks_title_after_pipe", "gfm+wikilinks_title_before_pipe"])
    c("image", [Para("a ", Image("alt", "i.png"), " b")])
    c("image-title", [Para("a ", Image("alt", "i.png", "ti"))])
    c("image-attr", [Para("a ", Image("alt", "i.png", attr=A("i", ["c"], [("width", "50%"), ("height", "2em")])))])
    c("image-empty-alt", [Para("a ", Image([], "i.png"))])
    c("image-format-alt", [Para("a ", Image(I(Emph("e"), " x"), "i.png"))])
    c("image-url-space", [Para("a ", Image("x", "a b.png"))])
    c("image-adjacent", [Para(Image("a", "a.png"), Image("b", "b.png"))])
    c("ref-like-text", [Para("[a]: http://e.x")])

    # The attributes of an image break between their parts, as those of a link.
    words = "word " * 10
    img_attr = A("fig-one", ["wide", "framed"], [("height", "11px"), ("width", "11px")])
    c("image-attributes-wrap", [Para(words, Image("small icon", "images/icon.svg", attr=img_attr), " after")])
    c("image-attributes-wrap-figure", [Fig(NA, [Plain("cap")], Plain(Image("cap", "images/icon-with-a-long-name.svg", attr=img_attr)))])
    c("link-attributes-wrap", [Para(words, Link("small link", "https://e.x/page", attr=img_attr), " after")])
    tag_attr = A("an-id", ["c1"], [("k", "v w")])
    c("html-link-tag-wrap-long-href", [Para("word " * 12, Link("t", "https://e.x/" + "p" * 40, attr=tag_attr))], to=["gfm", "markdown-link_attributes"])
    c("html-link-tag-wrap-title", [Para("word " * 10, Link("t", "https://e.x/p", title="the title here", attr=tag_attr), " end")], to=["gfm", "markdown-link_attributes"])
    c("html-span-tag-wrap", [Para("word " * 11, Span(tag_attr, Str("t")))], to=["gfm", "markdown-bracketed_spans-native_spans"])

    # ---------------------------------------------------------- math and raw inlines
    mto = ["markdown", "markdown-tex_math_dollars", "markdown-tex_math_dollars+tex_math_single_backslash",
           "markdown-tex_math_dollars+tex_math_double_backslash", "gfm", "gfm-tex_math_gfm", "gfm-tex_math_dollars-tex_math_gfm",
           "commonmark", "commonmark_x", "plain", "markdown_strict"]
    c = suite("math-raw", "Math per extension and raw inlines per format.", mto)
    c("inline", [Para("a ", Math("x^2"), " b")])
    c("inline-dollar", [Para(Math("a $ b"))])
    c("inline-spaces", [Para(Math(" x "))])
    c("inline-backslash", [Para(Math("\\frac{a}{b}"))])
    c("inline-backtick", [Para(Math("a`b"))])
    c("inline-empty", [Para("e", Math(""), "e")])
    c("inline-adjacent-digit", [Para(Math("x"), "5")])
    c("display-in-para", [Para("a ", DMath("x^2"), " b")])
    c("display-alone", [Para(DMath("\\int f"))])
    c("display-multiline", [Para(DMath("a\\\\\nb"))])
    c("display-dollar", [Para(DMath("a $ b"))])
    c("math-in-emph", [Para(Emph(Math("x")))])
    c("math-in-heading", [H(2, I("H ", Math("y")))])
    c("math-in-link", [Para(Link(Math("z"), "u"))])
    c("math-long", [Para(FILLER, " ", Math("a + b + c + d"))])
    rto = ["markdown", "markdown-raw_html", "markdown-raw_tex", "markdown-raw_attribute", "markdown-raw_html-raw_tex-raw_attribute",
           "gfm", "commonmark", "commonmark_x", "plain", "markdown_strict"]
    for fmt, text in [("html", "<b>x</b>"), ("html5", "<i>y</i>"), ("tex", "\\emph{x}"), ("latex", "\\textbf{y}"),
                      ("markdown", "*m*"), ("openxml", "<w:br/>"), ("rtf", "\\par"), ("gfm", "~~g~~"),
                      ("commonmark", "**c**"), ("plain", "pl"), ("markdown_strict", "_s_")]:
        c("raw-inline-" + fmt, [Para("a ", RawI(fmt, text), " b")], to=rto)
    c("raw-inline-html-comment", [Para(RawI("html", "<!-- c -->"))], to=rto)
    c("raw-inline-backtick", [Para(RawI("rst", "a`b"))], to=rto)
    c("raw-inline-tex-macro", [Para(RawI("tex", "\\foo"), "bar")], to=rto)
    c("raw-inline-empty", [Para("x", RawI("html", ""), "y")], to=rto)

    # White space at the ends of math: inline math loses it before the
    # conversion and before the warning; display math keeps it; `gfm`
    # writes the TeX without a warning and drops the outer newlines of a display.
    edge_to = ["plain", "markdown_strict", "gfm", "markdown-tex_math_dollars", "markdown"]
    for name, tex in [("spaces", " y^2 "), ("newline", "  c+d\n"), ("two-newlines", "\n\nc+d\n\n"),
                      ("bad-spaces", " \\sqrt{ "), ("bad-newline", "\n\\nosuch{y} ")]:
        c("edge-space-inline-" + name, [Para("a ", Math(tex), " b")], to=edge_to)
        c("edge-space-display-" + name, [Para("a ", DMath(tex), " b")], to=edge_to)

    # ---------------------------------------------------------- notes and citations
    nto = ["markdown", "markdown-footnotes", "gfm", "commonmark", "commonmark_x", "plain", "markdown_strict", "markdown_phpextra", "markdown_mmd"]
    c = suite("notes-citations", "Notes and citations.", nto)
    c("note", [Para("text", Note(Para("note")))])
    c("note-two", [Para("a", Note(Para("n1")), " b", Note(Para("n2")))])
    c("note-multi-para", [Para("a", Note(Para("p1"), Para("p2")))])
    c("note-code", [Para("a", Note(Para("p"), CodeBlock("x = 1")))])
    c("note-list", [Para("a", Note(BL(["i", "j"])))])
    c("note-in-list", [BL([[Plain("item", Note(Para("n")))]])])
    c("note-in-quote", [BQ(Para("q", Note(Para("n"))))])
    c("note-in-heading", [H(2, I("H", Note(Para("n"))))])
    c("note-plain", [Para("a", Note(Plain("bare")))])
    c("note-empty", [Para("a", Note())])
    c("note-long", [Para("a", Note(Para(" ".join([LONG] * 2))))])
    c("note-sections", [H(1, "A"), Para("x", Note(Para("n1"))), H(1, "B"), Para("y", Note(Para("n2")))])
    c("note-after-blocks", [Para("a", Note(Para("n"))), Para("after"), BL(["z"])])
    c("note-start-special", [Para("a", Note(Para("# h")))])
    c("note-in-note", [Para("a", Note(Para("b", Note(Para("c")))))])
    cto = ["markdown", "markdown-citations", "gfm", "commonmark", "commonmark_x", "plain", "markdown_strict"]
    c("cite-normal", [Para(Cite([cit("doe")], Str("[@doe]")))], to=cto)
    c("cite-intext", [Para(Cite([cit("doe", "AuthorInText")], Str("@doe")))], to=cto)
    c("cite-suppress", [Para(Cite([cit("doe", "SuppressAuthor")], Str("[-@doe]")))], to=cto)
    c("cite-prefix-suffix", [Para(Cite([cit("doe", prefix="see", suffix=", p. 3")], T("[see @doe, p. 3]")))], to=cto)
    c("cite-suffix-no-comma", [Para(Cite([cit("doe", suffix="p. 3")], T("[@doe p. 3]")))], to=cto)
    c("cite-intext-suffix", [Para(Cite([cit("doe", "AuthorInText", suffix="p. 3")], T("@doe [p. 3]")))], to=cto)
    c("cite-multiple", [Para(Cite([cit("a"), cit("b", suffix="ch. 2")], T("[@a; @b, ch. 2]")))], to=cto)
    c("cite-multiple-modes", [Para(Cite([cit("a", "SuppressAuthor"), cit("b")], T("[-@a; @b]")))], to=cto)
    c("cite-key-special", [Para(Cite([cit("doe:2020.x_y")], Str("[@doe:2020.x_y]")))], to=cto)
    c("cite-key-braces", [Para(Cite([cit("a b")], Str("[@{a b}]")))], to=cto)
    c("cite-empty-fallback", [Para(Cite([cit("doe")]))], to=cto)
    c("cite-intext-multiple", [Para(Cite([cit("a", "AuthorInText"), cit("b", "AuthorInText")], T("@a @b")))], to=cto)
    c("cite-in-note", [Para("a", Note(Para(Cite([cit("doe")], Str("[@doe]")))))], to=cto)
    c("cite-prefix-format", [Para(Cite([cit("doe", prefix=I(Emph("cf.")))], T("[cf. @doe]")))], to=cto)

    # The separator `; ` between citations does not break; the other spaces do.
    words = "word " * 9
    keys = [("alphaalphaalpha_one", "", ""), ("betabetabeta_two", "", ""), ("gammagamma_three", "", "")]
    c("cite-group-no-break", [Para(words, Cite([cit(k, prefix=p, suffix=x) for k, p, x in keys], Str("x")), " after words here")])
    c("cite-prefix-breaks", [Para(words, Cite([cit("alphaalphaalpha_one", prefix="see also the", suffix="p. 33 and more"), cit("betabetabeta_two")], Str("x")), " after")])
    c("cite-in-text-breaks", [Para(words, Cite([cit("alphaalpha_one", "AuthorInText", suffix="p. 33 and more words"), cit("betabeta_two")], Str("x")), " after")])
    c("cite-many-keys", [Para("text ", Cite([cit("key_number_%d" % n) for n in range(8)], Str("x")), ".")])

    # The writer converts the metadata before the body, also for a fragment:
    # the notes of the metadata come first, in the order of the keys.
    def mi(*xs):
        return {"t": "MetaInlines", "c": I(*xs)}
    body_note = [Para("body", Note(Para("body note")))]
    metas = {
        "author": {"author": {"t": "MetaList", "c": [mi("Ann", Note(Para("author note")))]}},
        "title": {"title": mi("T", Note(Para("title note")))},
        "custom": {"foo": mi("F", Note(Para("foo note")))},
        "blocks": {"abstract": {"t": "MetaBlocks", "c": [Para("Abs", Note(Para("abstract note")))]}},
        "two-keys": {"title": mi("T", Note(Para("t1"))), "author": {"t": "MetaList", "c": [mi("Ann", Note(Para("a1")))]}},
    }
    for name, meta in metas.items():
        c("meta-note-" + name, {"blocks": body_note, "meta": meta}, to=["markdown", "gfm", "plain"])

    # ---------------------------------------------------------- inline styles
    c = suite("inlines", "Emphasis, quotes, breaks, emoji, non-breaking spaces and empty inlines.", MD6)
    c("emph", [Para("a ", Emph("e"), " b")])
    c("strong", [Para("a ", Strong("s"), " b")])
    c("emph-in-strong", [Para(Strong("a ", Emph("b"), " c"))])
    c("strong-in-emph", [Para(Emph("a ", Strong("b"), " c"))])
    c("emph-emph", [Para(Emph("a ", Emph("b")))])
    c("emph-intraword", [Para("a", Emph("b"), "c")])
    c("strong-intraword", [Para("a", Strong("b"), "c")])
    c("emph-adjacent", [Para(Emph("a"), Emph("b"))])
    c("emph-strong-adjacent", [Para(Emph("a"), Strong("b"))])
    c("emph-leading-space", [Para("x", Emph(SP, "a"))])
    c("emph-trailing-space", [Para(Emph("a", SP), "x")])
    c("emph-empty", [Para("x", Emph(), "y")])
    c("strong-empty", [Para("x", Strong(), "y")])
    c("emph-star-inside", [Para(Emph("a*b"))])
    c("emph-underscore-inside", [Para(Emph("a_b"))])
    c("emph-punct-after", [Para(Emph("a"), "!")])
    c("emph-long", [Para(Emph(" ".join([LONG] * 2)))])
    sto = MD6 + ["markdown-strikeout", "markdown-superscript-subscript"]
    c("strike", [Para("a ", Strike("s"), " b")], to=sto)
    c("strike-empty", [Para("a", Strike(), "b")], to=sto)
    c("strike-tilde", [Para(Strike("a~b"))], to=sto)
    c("super", [Para("x", Sup("2"))], to=sto + ["markdown_mmd"])
    c("super-space", [Para("x", Sup("a b"))], to=sto + ["markdown_mmd", "markdown+short_subsuperscripts"])
    c("super-caret", [Para("x", Sup("a^b"))], to=sto)
    c("sub", [Para("H", Sub("2"), "O")], to=sto + ["markdown_mmd"])
    c("sub-space", [Para("x", Sub("a b"))], to=sto + ["markdown_mmd", "markdown+short_subsuperscripts"])
    c("sub-emph", [Para("x", Sub(Emph("i")))], to=sto)
    c("smallcaps", [Para("a ", SC("Sc"), " b")], to=MD6 + ["markdown-bracketed_spans", "markdown-bracketed_spans-native_spans", "markdown-bracketed_spans-native_spans-raw_html"])
    c("underline", [Para("a ", Und("u"), " b")], to=MD6 + ["markdown-bracketed_spans", "markdown-bracketed_spans-native_spans", "markdown-bracketed_spans-native_spans-raw_html"])
    c("underline-empty", [Para("a", Und(), "b")])
    qto = MD6 + ["markdown-smart", "gfm+smart", "commonmark+smart", "plain+smart"]
    c("quote-double", [Para(Q2("q"))], to=qto)
    c("quote-single", [Para(Q1("q"))], to=qto)
    c("quote-nested", [Para(Q2("a ", Q1("b"), " c"))], to=qto)
    c("quote-empty", [Para(Q2())], to=qto)
    c("quote-with-emph", [Para(Q1(Emph("e")))], to=qto)
    c("quote-apostrophe-text", [Para("don't 'x' \"y\"")], to=qto)
    c("quote-unicode-text", [Para("\u201ca\u201d \u2018b\u2019 don\u2019t")], to=qto)
    c("dashes-unicode", [Para("a\u2013b \u2014 c\u2026")], to=qto)
    c("dashes-ascii", [Para("a--b --- c...")], to=qto)
    bto = MD6 + ["markdown+hard_line_breaks", "markdown-escaped_line_breaks", "gfm+hard_line_breaks"]
    c("linebreak", [Para("a", LB, "b")], to=bto)
    c("linebreak-end", [Para("a", LB)], to=bto)
    c("linebreak-start", [Para(LB, "a")], to=bto)
    c("linebreak-double", [Para("a", LB, LB, "b")], to=bto)
    c("linebreak-in-emph", [Para(Emph("a", LB, "b"))], to=bto)
    c("softbreak", [Para("a", SB, "b")], to=bto)
    c("softbreak-end", [Para("a", SB)], to=bto)
    c("softbreak-space", [Para("a", SP, SB, "b")], to=bto)
    eto = MD6 + ["gfm-emoji", "markdown+emoji", "commonmark+emoji"]
    c("emoji", [Para("hi ", Span(A("", ["emoji"], [("data-emoji", "smile")]), "\U0001F604"))], to=eto)
    c("emoji-no-attr", [Para(Span(A("", ["emoji"]), "\U0001F604"))], to=eto)
    c("emoji-plus-one", [Para(Span(A("", ["emoji"], [("data-emoji", "+1")]), "\U0001F44D"))], to=eto)
    c("emoji-other-class", [Para(Span(A("", ["emoji", "x"], [("data-emoji", "smile")]), "\U0001F604"))], to=eto)
    c("nbsp-mid", [Para("a\u00a0b")])
    c("nbsp-start", [Para("\u00a0a")])
    c("nbsp-end", [Para("a\u00a0")])
    c("nbsp-only", [Para("\u00a0")])
    c("nbsp-before-dash", [Para("a\u00a0- b")])
    c("narrow-nbsp", [Para("a\u202fb\u2007c")])
    c("zero-width", [Para("a\u200bb")])
    c("unicode", [Para("caf\u00e9 \u65e5\u672c \U0001F600 \u0645\u0631\u062d\u0628\u0627")])
    c("control-tab", [Para("a\tb")])
    c("str-newline", [Para("a\nb")])
    c("empty-strs", [Para(Str(""), SP, Str(""))])
    c("empty-link-span-mix", [Para(Link([], ""), Span(NA), Emph(Strong()))])
    c("only-spaces", [Para(SP, SP)])
    c("many-styles", [Para(Strong(Emph(Strike(Sup(SC(Und("x")))))))])

    # ---------------------------------------------------------- writer options
    c = suite("options", "Writer options on the command line. These units wait for the command-line workstream.", ["markdown", "gfm", "plain"])
    long_para = [Para(" ".join([LONG] * 2), SB, "after soft break")]
    # Standalone output: the title block forms over the extension set, the
    # metadata kinds and the format.
    def mi(*xs):
        return {"t": "MetaInlines", "c": I(*xs)}

    def ms(text):
        return {"t": "MetaString", "c": text}

    def ml(*xs):
        return {"t": "MetaList", "c": list(xs)}

    def mm(**kw):
        return {"t": "MetaMap", "c": kw}

    def mb(*bs):
        return {"t": "MetaBlocks", "c": list(bs)}

    def mbool(b):
        return {"t": "MetaBool", "c": b}

    standalone_to = ["markdown", "markdown-yaml_metadata_block", "markdown-yaml_metadata_block-pandoc_title_block",
                     "markdown-yaml_metadata_block-pandoc_title_block+mmd_title_block", "markdown_mmd",
                     "markdown_strict", "markdown_phpextra", "gfm", "commonmark", "commonmark_x", "plain"]
    meta_kinds = {
        "none": {},
        "title": {"title": mi("A Short Title")},
        "title-author-date": {"title": mi("Field Notes"), "author": ml(mi("Ann Lee"), mi("Bo")), "date": mi("2020-01-02")},
        "author-single": {"author": mi("Cy Moss")},
        "date-only": {"date": ms("May 2001")},
        "other-keys": {"title": mi("T"), "lang": ms("en"), "tags": ml(ms("red"), ms("blue"))},
        "only-other": {"keywords": ml(mi("graph"))},
        "string-plain": {"note": ms("plain words here")},
        "string-colon": {"note": ms("left: right")},
        "string-hash": {"note": ms("before # after")},
        "string-number": {"note": ms("42")},
        "string-true": {"note": ms("true")},
        "string-y": {"note": ml(ms("y"), ms("n"), ms("yes"))},
        "bool": {"draft": mbool(True), "final": mbool(False)},
        "emphasis": {"note": mi(Emph("loud"), " words")},
        "quotes": {"note": mi('say "so"')},
        "empty-values": {"note": ms(""), "list": ml()},
        "map": {"info": mm(place=mi("Oslo"), codes=ml(mi("z")))},
        "blocks": {"abstract": mb(Para("first part"), Para("second part"))},
        "one-block": {"abstract": mb(Para("only part"))},
        "long-value": {"note": mi(" ".join(["value"] * 18))},
        "dash-start": {"note": ms("- item")},
        "brace-start": {"note": ms("{x}")},
        "bracket": {"note": ms("[x]")},
        "nested-list": {"grid": ml(ml(mi("a"), mi("b")))},
        "list-of-maps": {"people": ml(mm(name=mi("Di")), mm(name=mi("Ed")))},
        "line-break": {"note": mi("one", LB, "two")},
        "note-in-title": {"title": mi("Titled", Note(Para("title note")))},
        "title-long": {"title": mi(" ".join(["heading"] * 12))},
        "authors-long": {"author": ml(mi("A Very Long Name Of A Person"), mi("Another Long Name Of Someone"), mi("Third Person"))},
    }
    for name, meta in meta_kinds.items():
        c("standalone-" + name, {"blocks": [Para("Body text.")], "meta": meta}, to=standalone_to, args=["-s"])
    for a in [["--wrap=none"], ["--wrap=preserve"], ["--columns=20"], ["--columns=120"], ["--wrap=auto", "--columns=30"]]:
        c("wrap-" + slug(" ".join(a)), long_para, args=a)
    links = [Para(Link("one", "http://e.x/1", "t"), " and ", Link("two", "http://e.x/2"), " and ", Image("i", "i.png")), Para("x", Note(Para("n")))]
    c("reference-links", links, args=["--reference-links"])
    c("reference-links-commonmark", links, to=["commonmark", "markdown_strict"], args=["--reference-links"])
    c("reference-links-same-text", [Para(Link("a", "u1"), " ", Link("a", "u2"))], args=["--reference-links"])
    c("reference-links-block", links + [Para("tail")], args=["--reference-links", "--reference-location=block"])
    secs = [H(1, "A"), Para("x", Note(Para("n1")), " ", Link("l", "u")), H(1, "B"), Para("y", Note(Para("n2")))]
    for loc in ["block", "section", "document"]:
        c("reference-location-" + loc, secs, to=["markdown", "gfm", "commonmark_x"], args=["--reference-location=" + loc])
    c("reference-location-section-links", secs, to=["markdown", "gfm"], args=["--reference-links", "--reference-location=section"])
    heads = [H(1, "One"), Para("p"), H(2, "Two"), H(3, "Three")]
    c("setext", heads, to=["markdown", "gfm", "commonmark", "markdown_strict"], args=["--markdown-headings=setext"])
    c("setext-attr", [H(1, "One", A("x")), H(2, I("a", LB, "b"))], to=["markdown", "gfm", "commonmark"], args=["--markdown-headings=setext"])
    c("atx", heads, to=["markdown", "gfm", "commonmark"], args=["--markdown-headings=atx"])
    nested = [BL([[Plain("a"), BL([[Plain("b"), OL(["c"])]])]]), CodeBlock("\tx")]
    c("tab-stop-2", nested, to=["markdown", "gfm", "commonmark", "markdown_strict"], args=["--tab-stop=2"])
    c("tab-stop-8", nested, to=["markdown", "gfm", "commonmark"], args=["--tab-stop=8"])
    asc = [Para("caf\u00e9 \u2014 \u201cq\u201d \u00a0 \U0001F600 \u00e6"), H(1, "\u00e9t\u00e9")]
    c("ascii", asc, to=["markdown", "gfm", "commonmark", "plain"], args=["--ascii"])
    c("ascii-smart-off", asc, to=["markdown-smart", "gfm"], args=["--ascii"])
    c("id-prefix", [H(1, "Head", A("h")), Para("x", Note(Para("n")))], to=["markdown", "gfm", "commonmark_x"], args=["--id-prefix=p-"])
    c("number-sections", [H(1, "A"), H(2, "B"), H(1, "C", A("", ["unnumbered"]))], to=["markdown", "gfm", "plain"], args=["--number-sections"])
    c("top-level-chapter", [H(1, "A"), H(2, "B")], to=["markdown", "gfm", "plain"], args=["--top-level-division=chapter"])
    c("top-level-part", [H(1, "A")], to=["markdown", "gfm"], args=["--top-level-division=part"])
    tbl = [Table([["a", "b"]], [["1", "2"]], caption="C")]
    c("list-tables", tbl, to=["markdown", "gfm", "markdown_mmd"], args=["--list-tables"])
    c("columns-table", [Table([["name", "description"]], [["x", " ".join([LONG] * 2)]])], to=["markdown", "gfm", "plain"], args=["--columns=40"])
    c("columns-wide-table", [Table([["k%d" % i for i in range(9)]], [["value%d" % i for i in range(9)]])], to=["markdown", "gfm", "plain"], args=["--columns=200"])
    c("wrap-none-table", [Table([["name", "description"]], [["x", " ".join([LONG] * 2)]])], to=["markdown", "gfm", "plain"], args=["--wrap=none"])
    c("wrap-preserve-hard", [Para("a", SB, "# b", SB, "1. c")], to=["markdown", "gfm", "commonmark", "plain"], args=["--wrap=preserve"])
    c("wrap-none-line-start", [Para(FILLER, " # not heading")], to=["markdown", "gfm", "commonmark"], args=["--wrap=none"])
    c("columns-20-escapes", [Para("aaaa bbbb cccc - dddd + eeee 1. ffff # gggg > hhhh")], to=["markdown", "gfm", "commonmark", "markdown_strict"], args=["--columns=20"])
    c("columns-20-list", [BL([" ".join([LONG] * 1)]), OL([" ".join([LONG] * 1)], start=10)], args=["--columns=20"])
    c("columns-20-quote", [BQ(Para(LONG))], args=["--columns=20"])
    c("toc", [H(1, "A"), H(2, "B"), Para("x")], args=["--toc"])
    c("toc-standalone", [H(1, "A"), H(2, "B"), Para("x")], args=["-s", "--toc"])
    c("standalone-meta", [Para("x")], args=["-s", "--metadata=title:T"])
    c("dpi-image", [Para(Image("i", "i.png", attr=A("", [], [("width", "300px")])))], to=["markdown", "gfm", "markdown-link_attributes+raw_html"], args=["--dpi=72"])
    c("eol-crlf", [Para("a"), Para("b")], args=["--eol=crlf"])
    c("reference-links-figure", [Fig(NA, [Plain("cap")], Plain(Image("cap", "f.png")))], to=["markdown", "gfm"], args=["--reference-links"])
    c("wrap-none-notes", [Para("x", Note(Para(" ".join([LONG] * 2))))], args=["--wrap=none"])
    c("columns-120-lineblock", [LineBlock(" ".join([LONG] * 2))], args=["--columns=120"])


def cmd_cases(args):
    define_cases()
    os.makedirs(args.out, exist_ok=True)
    total = units = 0
    for name in ORDER:
        f = FILES[name]
        ids = [i for i, *_ in f["cases"]]
        dup = {i for i in ids if ids.count(i) > 1}
        if dup:
            sys.exit("duplicate ids in %s: %s" % (name, sorted(dup)))
        lines = ["# Generated by crates/randoc-markdown-writer/tools/mdw_tool.py cases. Do not edit.",
                 "# " + f["comment"], 'from = "json"', "to = " + toml_list(f["to"])]
        if f["args"]:
            lines.append("args = " + toml_list(f["args"]))
        for ident, blocks, to, a in f["cases"]:
            lines += ["", "[[case]]", 'id = "%s"' % ident]
            if to:
                lines.append("to = " + toml_list(to))
            if a:
                lines.append("args = " + toml_list(a))
            lines.append("input = " + toml_str(doc(blocks)))
            units += len(to or f["to"])
        total += len(f["cases"])
        with open(os.path.join(args.out, name + ".toml"), "w", encoding="utf-8") as fh:
            fh.write("\n".join(lines) + "\n")
        print("%-20s %4d cases" % (name, len(f["cases"])))
    print("total %d cases, %d write units, %.2f formats per case" % (total, units, units / total))
    if args.check:
        oracle = os.environ.get("RANDOC_ORACLE_PANDOC", "pandoc")
        jobs = [(name, ident, t, (a or FILES[name]["args"] or []), doc(blocks))
                for name in ORDER for ident, blocks, to, a in FILES[name]["cases"] for t in (to or FILES[name]["to"])]

        def check(job):
            name, ident, t, a, tree = job
            return job, run([oracle, "-f", "json", "-t", t] + a, tree)

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            for (name, ident, t, a, _), (rc, _, err) in pool.map(check, jobs):
                if rc != 0 or err:
                    print("%s/%s %s %s exit %d: %s" % (name, ident, t, " ".join(a), rc, err.strip().replace("\n", " | ")[:200]))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("diff", help="compare the oracle with Randoc on random trees")
    d.add_argument("--seed", default="1")
    d.add_argument("--count", type=int, default=100)
    d.add_argument("--formats", default="markdown,gfm,commonmark,commonmark_x,markdown_strict,plain")
    d.add_argument("--ext-changes", action="store_true")
    d.add_argument("--randoc", default="target/debug/randoc")
    d.add_argument("--show", type=int)
    d.add_argument("--oracle-only", action="store_true")
    d.add_argument("--jobs", type=int, default=2)
    d.add_argument("--no-math", action="store_true", help="generate no Math nodes")
    d.add_argument("--small", action="store_true", help="trees of depth 2 with at most 6 nodes")
    d.add_argument("--signatures", action="store_true", help="count the first differing line pairs")
    r = sub.add_parser("reduce", help="reduce each differing random tree and count the reduced shapes")
    r.add_argument("--seed", default="7")
    r.add_argument("--count", type=int, default=200)
    r.add_argument("--format", default="markdown")
    r.add_argument("--randoc", default="target/debug/randoc")
    r.add_argument("--jobs", type=int, default=2)
    r.add_argument("--no-math", action="store_true")
    r.add_argument("--small", action="store_true")
    r.add_argument("--out", required=True, help="a JSON-lines file for the reduced trees")
    k = sub.add_parser("cases", help="write the conformance case files")
    k.add_argument("--out", default="conformance/cases/markdown-writer")
    k.add_argument("--check", action="store_true", help="run the oracle on each unit; print each error and warning")
    c = sub.add_parser("corpus", help="classify the corpus documents whose writer output differs")
    c.add_argument("--to", default="markdown")
    c.add_argument("--formats", default=CORPUS_FORMATS, help="the input formats of the documents")
    c.add_argument("--randoc", default="target/debug/randoc")
    c.add_argument("--jobs", type=int, default=2)
    c.add_argument("--trees", required=True, help="a folder for the oracle trees")
    a = ap.parse_args()
    commands = {"diff": cmd_diff, "reduce": cmd_reduce, "cases": cmd_cases, "corpus": cmd_corpus}
    sys.exit(commands[a.cmd](a))


if __name__ == "__main__":
    main()
