#!/usr/bin/env python3
"""Generates `src/option_tests.rs`: unit tests whose expected text and
warnings come from the Pandoc oracle.

Each test gives document blocks, writer options and a format. The script
converts the blocks with `pandoc -f json -t FORMAT` and the matching
command-line options, and writes a Rust test that calls the writer with the
matching `WriterOptions`. The program adds the final newline, so a test
expects the oracle output without it.

Usage: RANDOC_ORACLE_PANDOC=... python3 tools/oracle_tests.py
"""
import json, os, subprocess

P = os.environ["RANDOC_ORACLE_PANDOC"]
A = ["", [], []]
S = lambda t: {"t": "Str", "c": t}
SP = {"t": "Space"}
Pa = lambda *x: {"t": "Para", "c": list(x)}
PL = lambda *x: {"t": "Plain", "c": list(x)}
note = lambda t: {"t": "Note", "c": [Pa(S(t))]}
H = lambda level, ident, ils, cls=(), kv=(): {"t": "Header", "c": [level, [ident, list(cls), [list(p) for p in kv]], ils]}
Div = lambda attr, *b: {"t": "Div", "c": [attr, list(b)]}
Span = lambda attr, *x: {"t": "Span", "c": [attr, list(x)]}
Link = lambda attr, ils, url, title="": {"t": "Link", "c": [attr, ils, [url, title]]}
Img = lambda attr, ils, url="i.png", title="": {"t": "Image", "c": [attr, ils, [url, title]]}
Cell = lambda blocks, rs=1, cs=1, attr=A, al="AlignDefault": [attr, {"t": al}, rs, cs, blocks]
Row = lambda *cells, attr=A: [attr, list(cells)]
Spec = lambda al="AlignDefault": [{"t": al}, {"t": "ColWidthDefault"}]
def Table(specs, head, bodies, foot=(), caption=()):
    return {"t": "Table", "c": [A, [None, list(caption)], specs, [A, list(head)], bodies, [A, list(foot)]]}
Body = lambda rows, rh=0, heads=(): [A, rh, list(heads), rows]

# Each option: (Rust setup line, pandoc arguments).
def option_code(key, value):
    if key == "wrap":
        variant = {"none": "WrapNone", "preserve": "WrapPreserve"}[value]
        return f"options.wrap_text = WrapOption::{variant};", [f"--wrap={value}"]
    if key == "columns":
        return f"options.columns = {value};", [f"--columns={value}"]
    if key == "section_divs":
        return "options.section_divs = true;", ["--section-divs"]
    if key == "number_sections":
        return "options.number_sections = true;", ["--number-sections"]
    if key == "number_offset":
        return (f"options.number_offset = vec!{value};".replace(" ", ""),
                ["--number-offset=" + ",".join(map(str, value))])
    if key == "id_prefix":
        return f"options.identifier_prefix = {json.dumps(value)}.into();", [f"--id-prefix={value}"]
    if key == "q_tags":
        return "options.html_q_tags = true;", ["--html-q-tags"]
    if key == "email":
        return "options.email_obfuscation = ObfuscationMethod::ReferenceObfuscation;", ["--email-obfuscation=references"]
    if key == "ascii":
        return "options.prefer_ascii = true;", ["--ascii"]
    if key == "math":
        rust = {"mathjax": "HtmlMathMethod::MathJax(String::new())",
                "katex": "HtmlMathMethod::KaTex(String::new())",
                "webtex": 'HtmlMathMethod::WebTex("https://latex.codecogs.com/png.latex?".into())',
                "gladtex": "HtmlMathMethod::GladTex"}[value]
        return f"options.html_math_method = {rust};", [f"--math-method={value}"]
    if key == "refloc":
        variant = {"block": "EndOfBlock", "section": "EndOfSection"}[value]
        return f"options.reference_location = ReferenceLocation::{variant};", [f"--reference-location={value}"]
    if key == "ext":
        return "options.extensions.enable(Extension::EmptyParagraphs);", []
    raise KeyError(key)

OLD = [('wrap_none', [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'one'}, {'t': 'Space'}, {'t': 'Str', 'c': 'two'}, {'t': 'Space'}, {'t': 'Str', 'c': 'three'}, {'t': 'Space'}, {'t': 'Str', 'c': 'four'}, {'t': 'Space'}, {'t': 'Str', 'c': 'five'}, {'t': 'Space'}, {'t': 'Str', 'c': 'six'}, {'t': 'Space'}, {'t': 'Str', 'c': 'seven'}, {'t': 'Space'}, {'t': 'Str', 'c': 'eight'}, {'t': 'SoftBreak'}, {'t': 'Str', 'c': 'nine'}, {'t': 'Space'}, {'t': 'Str', 'c': 'ten'}, {'t': 'LineBreak'}, {'t': 'Str', 'c': 'end'}]}], {'wrap': 'none'}, 'html'), ('wrap_preserve', [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'one'}, {'t': 'Space'}, {'t': 'Str', 'c': 'two'}, {'t': 'Space'}, {'t': 'Str', 'c': 'three'}, {'t': 'Space'}, {'t': 'Str', 'c': 'four'}, {'t': 'Space'}, {'t': 'Str', 'c': 'five'}, {'t': 'Space'}, {'t': 'Str', 'c': 'six'}, {'t': 'Space'}, {'t': 'Str', 'c': 'seven'}, {'t': 'Space'}, {'t': 'Str', 'c': 'eight'}, {'t': 'SoftBreak'}, {'t': 'Str', 'c': 'nine'}, {'t': 'Space'}, {'t': 'Str', 'c': 'ten'}, {'t': 'LineBreak'}, {'t': 'Str', 'c': 'end'}]}], {'wrap': 'preserve'}, 'html'), ('columns_20', [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'one'}, {'t': 'Space'}, {'t': 'Str', 'c': 'two'}, {'t': 'Space'}, {'t': 'Str', 'c': 'three'}, {'t': 'Space'}, {'t': 'Str', 'c': 'four'}, {'t': 'Space'}, {'t': 'Str', 'c': 'five'}, {'t': 'Space'}, {'t': 'Str', 'c': 'six'}, {'t': 'Space'}, {'t': 'Str', 'c': 'seven'}, {'t': 'Space'}, {'t': 'Str', 'c': 'eight'}, {'t': 'SoftBreak'}, {'t': 'Str', 'c': 'nine'}, {'t': 'Space'}, {'t': 'Str', 'c': 'ten'}, {'t': 'LineBreak'}, {'t': 'Str', 'c': 'end'}]}], {'columns': 20}, 'html'), ('section_divs', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}]}, {'t': 'Header', 'c': [2, ['u', ['unnumbered'], []], [{'t': 'Str', 'c': 'U'}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'z'}]}]]}], {'section_divs': True}, 'html'), ('section_divs_html4', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}]}, {'t': 'Header', 'c': [2, ['u', ['unnumbered'], []], [{'t': 'Str', 'c': 'U'}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'z'}]}]]}], {'section_divs': True}, 'html4'), ('number_sections', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}]}, {'t': 'Header', 'c': [2, ['u', ['unnumbered'], []], [{'t': 'Str', 'c': 'U'}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'z'}]}]]}], {'number_sections': True}, 'html'), ('number_sections_html4', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}]}, {'t': 'Header', 'c': [2, ['u', ['unnumbered'], []], [{'t': 'Str', 'c': 'U'}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'z'}]}]]}], {'number_sections': True}, 'html4'), ('number_sections_offset', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}]}, {'t': 'Header', 'c': [2, ['u', ['unnumbered'], []], [{'t': 'Str', 'c': 'U'}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'z'}]}]]}], {'number_sections': True, 'number_offset': [3, 2]}, 'html'), ('number_sections_divs', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}]}, {'t': 'Header', 'c': [2, ['u', ['unnumbered'], []], [{'t': 'Str', 'c': 'U'}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'z'}]}]]}], {'number_sections': True, 'section_divs': True}, 'html'), ('id_prefix', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}, {'t': 'Str', 'c': 'z'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'w'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n4'}]}]}]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'v'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n5'}]}]}]}, {'t': 'BlockQuote', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'q'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n6'}]}]}]}]}], {'id_prefix': 'p-'}, 'html'), ('q_tags', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'q_tags': True, 'math': 'mathjax'}, 'html'), ('email_references', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'email': 'references', 'math': 'mathjax'}, 'html'), ('ascii', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'ascii': True, 'math': 'mathjax'}, 'html'), ('mathjax', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'math': 'mathjax'}, 'html'), ('katex', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'math': 'katex'}, 'html'), ('webtex', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'math': 'webtex'}, 'html'), ('gladtex', [{'t': 'Para', 'c': [{'t': 'Quoted', 'c': [{'t': 'DoubleQuote'}, [{'t': 'Str', 'c': 'q'}]]}, {'t': 'Space'}, {'t': 'Quoted', 'c': [{'t': 'SingleQuote'}, [{'t': 'Str', 'c': 's'}]]}, {'t': 'Space'}, {'t': 'Str', 'c': 'é→'}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'me@x.org'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Link', 'c': [['', [], []], [{'t': 'Str', 'c': 'mail'}, {'t': 'Space'}, {'t': 'Str', 'c': 'me'}], ['mailto:me@x.org', '']]}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'InlineMath'}, 'a^2 + b']}, {'t': 'Space'}, {'t': 'Math', 'c': [{'t': 'DisplayMath'}, 'x<y']}]}, {'t': 'Para', 'c': [{'t': 'Span', 'c': [['s', [], [['title', 'é']]], [{'t': 'Str', 'c': 'ü'}]]}]}], {'math': 'gladtex'}, 'html'), ('notes_block', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}, {'t': 'Str', 'c': 'z'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'w'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n4'}]}]}]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'v'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n5'}]}]}]}, {'t': 'BlockQuote', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'q'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n6'}]}]}]}]}], {'refloc': 'block'}, 'html'), ('notes_block_html4', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}, {'t': 'Str', 'c': 'z'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'w'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n4'}]}]}]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'v'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n5'}]}]}]}, {'t': 'BlockQuote', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'q'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n6'}]}]}]}]}], {'refloc': 'block'}, 'html4'), ('notes_section', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}, {'t': 'Str', 'c': 'z'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'w'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n4'}]}]}]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'v'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n5'}]}]}]}, {'t': 'BlockQuote', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'q'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n6'}]}]}]}]}], {'refloc': 'section'}, 'html'), ('notes_section_divs', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'y'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}, {'t': 'Str', 'c': 'z'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'w'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n4'}]}]}]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'v'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n5'}]}]}]}, {'t': 'BlockQuote', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'q'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n6'}]}]}]}]}], {'refloc': 'section', 'section_divs': True}, 'html'), ('notes_block_in_div', [{'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'a'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'b'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}]}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'c'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}], {'refloc': 'block'}, 'html'), ('notes_section_in_div', [{'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'a'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Header', 'c': [1, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'b'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}]}]]}, {'t': 'Header', 'c': [1, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'c'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}], {'refloc': 'section'}, 'html'), ('notes_block_div_section', [{'t': 'Div', 'c': [['', ['d'], []], [{'t': 'Header', 'c': [2, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'a'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'b'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n2'}]}]}]}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'c'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n3'}]}]}]}], {'refloc': 'block'}, 'html'), ('notes_section_divs_deep', [{'t': 'Header', 'c': [1, ['a', [], []], [{'t': 'Str', 'c': 'A'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'a'}, {'t': 'Note', 'c': [{'t': 'Para', 'c': [{'t': 'Str', 'c': 'n1'}]}]}]}, {'t': 'Header', 'c': [2, ['b', [], []], [{'t': 'Str', 'c': 'B'}]]}, {'t': 'Header', 'c': [3, ['c', [], []], [{'t': 'Str', 'c': 'C'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'c'}]}, {'t': 'Header', 'c': [2, ['d', [], []], [{'t': 'Str', 'c': 'D'}]]}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'd'}]}, {'t': 'Header', 'c': [1, ['e', [], []], [{'t': 'Str', 'c': 'E'}]]}], {'refloc': 'section', 'section_divs': True}, 'html'), ('empty_paragraphs', [{'t': 'Para', 'c': []}, {'t': 'Para', 'c': [{'t': 'Str', 'c': 'x'}]}], {'ext': '+empty_paragraphs'}, 'html')]

X = [S("x")]
H0 = {"t": "Header", "c": [0, ["", [], []], [S("n")]]}
TWO = [Spec(), Spec()]
# Regression tests for the findings of the round 1 reviews. The name gives the finding.
REVIEW = [
    ("rh01_section_numbers_wrap_around", [H(1, "", [S("x")])], {"number_sections": True, "number_offset": [9223372036854775807]}, "html"),
    ("rh03_duplicate_keys_warn_and_keep_the_first", [Pa(Span(["", [], [["foo", "1"], ["foo", "2"]]], *X))], {}, "html"),
    ("rh03_empty_key_is_dropped", [Pa(Span(["", [], [["", "v"]]], *X))], {}, "html"),
    ("f07_duplicates_classes_and_id_keys", [Pa(Span(["i", ["a", "a", ""], [["k", "1"], ["k", "2"], ["id", "q"], ["class", "z"], ["inlist", "v"]]], *X))], {}, "html"),
    ("f07_html4_identifier_needs_a_letter", [Pa(Span([" ", [], []], *X), Link(A, X, "#1a"), Span(["1a", [], []], *X))], {}, "html4"),
    ("f07_html4_heading_keeps_few_keys", [H(2, "h", [S("z")], kv=[("hidden", "v"), ("title", "t"), ("lang", "en"), ("onclick", "f"), ("rel", "r"), ("k", "v")])], {}, "html4"),
    ("f07_heading_role_moves_to_a_section", [H(2, "h", [S("z")], cls=["c"], kv=[("role", "v"), ("k", "w")]), Pa(S("p"))], {}, "html"),
    ("f07_heading_epub_type_moves_to_a_section", [H(2, "h", [S("z")], kv=[("epub:type", "v")])], {}, "html4"),
    ("rh04_id_prefix_changes_internal_links", [Pa(Link(A, X, "#x"), Link(A, X, "u#x"), Link(A, X, "#"), Img(A, X, "#i"))], {"id_prefix": "p-"}, "html"),
    ("f13_html4_id_prefix_is_percent_encoded", [Pa(Link(A, X, "#x"), note("n"))], {"id_prefix": 'a"<'}, "html4"),
    ("rh05_heading_note_with_section_divs_and_numbers", [H(1, "a", [S("A"), note("n")]), Pa(S("x"))], {"section_divs": True, "number_sections": True}, "html"),
    ("rh06_div_without_attributes_is_not_a_section", [Div(A, H(1, "h", [S("T")]), Pa(S("p")))], {}, "html"),
    ("rh06_div_without_attributes_html4", [Div(A, H(1, "", [S("h")]))], {}, "html4"),
    ("rh07_section_notes_with_skipped_levels", [H(1, "", [S("h"), note("n")]), H(3, "", [S("h")]), H(2, "", [S("h")])], {"refloc": "section"}, "html"),
    ("rh07_section_notes_after_content", [Pa(S("a"), note("na")), H(1, "", [S("h"), note("nh")]), Pa(S("p"), note("np"))], {"refloc": "section"}, "html"),
    ("rh08_block_notes_after_a_heading_with_section_divs", [H(1, "", [S("h"), note("n")]), Pa(S("x"))], {"section_divs": True, "refloc": "block"}, "html"),
    ("f11_block_notes_after_a_heading_get_its_id", [H(1, "a", [S("h"), note("nh")]), Pa(S("p"))], {"refloc": "block"}, "html"),
    ("f11_section_notes_without_headings", [Pa(S("p"), note("np"))], {"refloc": "section"}, "html"),
    ("rh09_reference_obfuscation_keeps_structure", [Pa(Link(["id", ["class"], [["foo", "v"]]], [{"t": "Emph", "c": X}], "mailto:a@b", "title"), Link(A, [S("a"), SP, {"t": "Code", "c": [A, "c d"]}], "mailto:a@b"))], {"email": "references"}, "html"),
    ("f12_mailto_link_has_no_title", [Pa(Link(["i", ["c"], []], [S("me")], "mailto:me@x.y", "T"), Link(A, [S("me")], "MAILTO:me@x.y", "T"))], {}, "html"),
    ("rh10_span_stops_at_the_row_heads", [Table(TWO, [], [Body([Row(Cell([PL(S("A"))], cs=2))], rh=1)])], {}, "html"),
    ("rh10_span_stops_at_the_row_heads_after_a_cell", [Table([Spec(), Spec(), Spec()], [], [Body([Row(Cell([PL(S("A"))]), Cell([PL(S("B"))], cs=3))], rh=2)])], {}, "html"),
    ("rh11_empty_head_row_gives_no_head", [Table([Spec()], [Row()], [Body([Row(Cell([PL(S("A"))]))])])], {}, "html"),
    ("f10_blank_head_and_foot_are_dropped", [Table([Spec()], [Row(Cell([]))], [Body([Row(Cell([PL(S("b"))]))])], foot=[Row(Cell([]))])], {}, "html"),
    ("f10_head_cell_with_attributes_stays", [Table([Spec()], [Row(Cell([], attr=["ci", [], []]))], [Body([Row(Cell([PL(S("b"))]))])], foot=[Row(Cell([PL()]))])], {}, "html"),
    ("n05_empty_head_cell_with_alignment_stays", [Table([Spec()], [Row(Cell([], al="AlignCenter"))], [Body([Row(Cell([PL(S("A"))]))])])], {}, "html"),
    ("n05_empty_foot_cell_with_a_span_stays", [Table(TWO, [], [Body([Row(Cell([PL(S("A"))]), Cell([PL(S("B"))]))])], foot=[Row(Cell([], cs=2))])], {}, "html4"),
    ("n05_colgroup_before_a_foot", [{"t": "Table", "c": [A, [None, []], [[{"t": "AlignDefault"}, {"t": "ColWidth", "c": 0.5}]], [A, []], [], [A, [Row(Cell([PL(S("f"))]))]]]}], {}, "html"),
    ("n06_offsets_count_from_level_one", [H(2, "", [S("a")]), H(7, "", [S("b")]), H(0, "", [S("z")])], {"number_sections": True, "number_offset": [1]}, "html"),
    ("n06_offsets_and_level_zero_in_html4", [H(0, "", [S("z")]), H(1, "", [S("a")])], {"number_sections": True, "section_divs": True, "number_offset": [5]}, "html4"),
    ("n06_extreme_offsets", [H(2, "", [S("a")]), H(4, "", [S("b")])], {"number_sections": True, "number_offset": [9223372036854775807, -9223372036854775808]}, "html"),
    ("r2_04_html4_heading_id_pair_goes_away", [H(2, "k0", [S("x")], kv=[("id", "v")])], {}, "html4"),
    ("r2_05_alt_pair_replaces_the_description", [Pa(Img(["", [], [["alt", "v"]]], [S("x")], "u"), Img(["", [], [["style", "a"], ["style", "b"], ["width", "50%"]]], [S("x")], "u"))], {}, "html"),
    ("r2_06_dimension_spaces_and_rounding", [Pa(*[Img(["", [], [["width", w]]], [S("x")], "u") for w in ["5\tcm", "5\ncm", "0.000005cm", "1.234575cm", "9" * 400 + "cm"]])], {}, "html"),
    ("r2_07_html4_identifier_before_the_prefix", [Pa(Span(["1a", [], []], *X), Span(["c", [], []], *X)), Pa(Link(A, X, "#1a"), Link(A, X, "u"))], {"id_prefix": "p-"}, "html4"),
    ("n09_html4_prefix_with_a_digit", [Pa(Span(["c", [], []], *X), Span(["1c", [], []], *X))], {"id_prefix": "1"}, "html4"),
    ("r2_09_inner_link_uses_the_span_rules", [Pa(Link(A, [Link(["", ["mark"], []], X, "u"), Link(["i", ["csl-no-emph"], []], X, "u")], "u"))], {}, "html"),
    ("r2_10_notes_of_a_parent_with_an_empty_subsection", [H(1, "", [note("a")]), H(2, "", [S("child")]), H(1, "", [S("b")]), Pa(note("b"))], {"refloc": "section"}, "html"),
    ("r2_11_section_div_closes_the_section", [H(1, "", [S("a")]), Div(["d", ["c"], []], H(1, "", [S("b")])), H(0, "", [S("z")]), Div(["e", [], [["k", "v"]]], H(0, "", [S("y")]))], {"section_divs": True}, "html"),
    ("r2_12_merged_div_and_heading_pairs_do_not_warn", [Div(["d", ["c"], [["foo", "v"]]], H(1, "", [S("x")], kv=[("foo", "w")]))], {}, "html"),
    ("r2_13_inline_code_with_the_hook_arrow", [Pa({"t": "Code", "c": [A, "\u21a9"]})], {}, "html"),
    ("r2_14_notes_after_a_raw_block_with_a_newline", [Pa(note("n")), {"t": "RawBlock", "c": ["html", "x\n"]}], {}, "html"),
    ("r2_15_obfuscation_leaves_notes_alone", [Pa(Link(A, [note("n")], "mailto:a@b"), Link(A, [{"t": "LineBreak"}], "mailto:a@b"))], {"email": "references"}, "html"),
    ("r2_16_large_column_widths", [{"t": "Table", "c": [A, [None, []], [[{"t": "AlignDefault"}, {"t": "ColWidth", "c": 1e18}]], [A, []], [Body([Row(Cell([]))])], [A, []]]}], {}, "html"),
    ("n08_notes_inside_a_note", [Pa(S("a"), {"t": "Note", "c": [Pa(S("o"), note("i"), note("j"))]}, S("b"), note("after"))], {}, "html"),
    ("n09_html4_block_notes_after_a_heading", [H(1, "a", [S("h"), note("nh")]), Pa(S("p"))], {"refloc": "block"}, "html4"),
    ("r2_cell_alignment_replaces_a_style_pair", [Table([Spec("AlignCenter")], [], [Body([Row(Cell([], attr=["i", [], [["role", "v"], ["style", "s"]]]), ), Row(Cell([], attr=["", [], [["style", "s"]]], al="AlignDefault"))])])], {}, "html"),
    ("r2_note_ending_in_a_raw_block", [Pa(S("a"), {"t": "Note", "c": [{"t": "RawBlock", "c": ["html", "x\n"]}]})], {}, "html"),
    ("r2_csl_class_with_a_special_span", [Pa(Span(["i", ["smallcaps", "csl-no-emph"], [["lang", "en"]]], *X), Span(["i", ["mark", "csl-no-emph"], [["title", "t"]]], *X))], {}, "html"),
    ("i_html_10_code_block_start_tag", [{"t": "CodeBlock", "c": [["", ["x" * pad], [["k", "v"]]], code]} for pad in (40, 43) for code in ("ab cdefghijkl", "a &lt; b\n\n c")], {}, "html"),
    ("i_html_11_section_with_only_empty_blocks", [Div(["", ["c"], []], Pa(S("p")), H(2, "", [S("h")]), Pa()), H(2, "", [S("t")]), Pa()], {}, "html"),
    ("i_html_11_section_divs_with_only_empty_blocks", [H(1, "", [S("a")]), H(2, "", [S("h")]), Pa(), {"t": "RawBlock", "c": ["html", "x\n\n"]}], {"section_divs": True}, "html"),
    ("r3_notes_with_blocks_that_give_no_html", [Pa(S("a"), {"t": "Note", "c": [Pa(), PL(S("w"))]}, {"t": "Note", "c": [{"t": "RawBlock", "c": ["latex", "x"]}]}, {"t": "Note", "c": [{"t": "RawBlock", "c": ["html", ""]}]})], {}, "html4"),
    ("r3_number_span_space_with_wrap_preserve", [H(1, "", [S("a"), {"t": "Space"}, {"t": "LineBreak"}])], {"number_sections": True, "wrap": "preserve"}, "html"),
    ("r3_obfuscation_of_breaks_and_the_hook_arrow", [Pa(Link(A, [S("\u21a9"), {"t": "SoftBreak"}, S("x")], "mailto:a@b"))], {"email": "references", "wrap": "none"}, "html"),
    ("r3_note_with_a_line_break_before_a_paragraph", [Pa(S("a"), {"t": "Note", "c": [PL(S("x"), {"t": "LineBreak"}), Pa(S("y"))]})], {}, "html4"),
    ("r3_number_of_a_div_section_after_the_pairs", [Div(["i", [], [["lang", "10"], ["k", "10"]]], H(1, "", [S("x")]))], {"number_sections": True}, "html"),
    ("i_html_16_section_class_heading_in_a_div", [Div(A, H(1, "", [S("h")], cls=["section"])), Div(["d", ["e"], []], H(2, "x", [], cls=["section", "c"]), Pa(S("p")))], {}, "html"),
    ("i_html_16_section_class_heading_in_a_div_html4", [Div(A, H(1, "", [S("h")], cls=["section"]), H(2, "", [S("f")]))], {}, "html4"),
    ("i_html_16_section_class_heading_with_section_divs", [Pa(S("p")), H(1, "x", [S("h")], cls=["section"], kv=[("k", "v")]), Div(A, H(1, "", [S("h")], cls=["section"]))], {"section_divs": True}, "html"),
    ("i_html_19_notes_after_a_section_in_divs", [Div(A, Div(["1a", [], []], H(2, "", [S("x")]))), Pa(S("a"), note("n"))], {"refloc": "section", "section_divs": True}, "html"),
    ("i_html_19_block_notes_in_a_div_that_holds_a_section", [Div(A, Div(["d", [], []], H(2, "", [S("h"), note("hn")]), Pa(S("q"), note("qn"))))], {"refloc": "block"}, "html"),
    ("i_html_20_div_with_a_section_div_of_the_same_level", [Div(A, H(3, "", [S("a")]), Pa(S("p")), Div(A, H(3, "", [S("b")])))], {"number_sections": True, "number_offset": [5, 0, 1]}, "html"),
    ("i_html_20_div_with_a_section_div_without_numbers", [Div(A, H(3, "", [S("a")]), Div(A, H(3, "", [S("b")])))], {}, "html"),
    ("r3_04_section_notes_before_a_block_without_html", [H(1, "", [note("n")]), Pa()], {"refloc": "section"}, "html"),
    ("r3_04_section_notes_before_an_ignored_raw_block", [H(1, "", [note("n")]), {"t": "RawBlock", "c": ["latex", "x"]}, H(1, "", [S("b"), note("m")]), Pa({"t": "RawInline", "c": ["latex", "x"]})], {"refloc": "section"}, "html4"),
    ("r3_02_repeated_special_classes", [Pa(*[Span(["", c, []], S("x")) for c in [["mark", "mark"], ["kbd", "kbd"], ["smallcaps", "smallcaps"], ["underline", "mark", "underline"], ["a", "mark", "b", "b", "mark"]]]), Pa(Link(A, [Link(A, [Span(["", ["mark", "mark"], []], S("x"))], "v")], "u"))], {}, "html"),
    ("r3_02_repeated_special_classes_html4", [Pa(*[Span(["", c, []], S("x")) for c in [["mark", "mark"], ["underline", "kbd", "underline"]]])], {}, "html4"),
    ("r3_07_csl_style_order", [Pa(*[Span(["", list(c), []], S("x")) for c in __import__("itertools").permutations(["csl-no-emph", "csl-no-strong", "csl-no-smallcaps"])]), Pa(Span(["", ["csl-no-smallcaps", "csl-no-emph", "csl-no-emph"], []], S("x")), Span(["", ["csl-no-strong", "mark"], []], S("x")), Span(["", ["c", "csl-no-strong"], [["style", "color:red"]]], S("x")))], {}, "html"),
    ("r3_07_csl_block_classes_are_divs", [Pa(*[Span(["", [c], []], S("x")) for c in ["csl-block", "csl-left-margin", "csl-right-inline", "csl-indent", "csl-entry"]])], {}, "html"),
    ("r3_03_heading_removes_repeated_classes", [H(1, "a", [S("x")], ["c", "c", "d", "c"]), {"t": "BlockQuote", "c": [H(2, "b", [S("x")], ["c", "c"])]}, Div(["", ["e"], []], Pa(S("p")), H(2, "", [S("x")], ["c", "c"])), {"t": "Figure", "c": [A, [None, []], [H(3, "", [S("x")], ["c", "c"])]]}, Table([Spec()], [], [Body([Row(Cell([H(4, "", [S("x")], ["c", "c"])]))])])], {}, "html"),
    ("r3_03_heading_removes_repeated_classes_in_sections", [H(1, "a", [S("x")], ["c", "c", "unnumbered", "unnumbered"]), Div(["d", ["s"], []], H(2, "", [S("x")], ["c", "c"]))], {"section_divs": True, "number_sections": True}, "html"),
    ("r3_03_heading_removes_repeated_classes_html4", [H(1, "a", [S("x")], ["c", "c"])], {}, "html4"),
    ("r3_01_cell_alignment_merges_the_style", [Table([Spec()] + [Spec("AlignRight")] * 15, [Row(Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color: red"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color:red;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", ""]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "  "]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "text-align: left;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "margin:0;text-align:left;color:red; text-align: right"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "a;b"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color: red; bad"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color:red;;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "a:b:c"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "  :v"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x: ;y: 2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x:1;  ;y:2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x:1\n;\ny:2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "font-family: \"A B\"; x: url(a;b)"]]], al="AlignCenter"))], [Body([Row(Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color: red"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color:red;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", ""]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "  "]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "text-align: left;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "margin:0;text-align:left;color:red; text-align: right"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "a;b"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color: red; bad"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color:red;;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "a:b:c"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "  :v"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x: ;y: 2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x:1;  ;y:2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x:1\n;\ny:2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "font-family: \"A B\"; x: url(a;b)"]]], al="AlignCenter"))])], [Row(Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color: red"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color:red;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", ""]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "  "]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "text-align: left;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "margin:0;text-align:left;color:red; text-align: right"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "a;b"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color: red; bad"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "color:red;;"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "a:b:c"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "  :v"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x: ;y: 2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x:1;  ;y:2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "x:1\n;\ny:2"]]], al="AlignCenter"), Cell([PL(S("A"))], attr=["", [], [["k", "v"], ["style", "font-family: \"A B\"; x: url(a;b)"]]], al="AlignCenter"))]), Table([Spec("AlignRight"), Spec()], [], [Body([Row(Cell([PL(S("A"))], attr=["", [], [["style", "color: red"]]]), Cell([PL(S("B"))], attr=["", [], [["style", "color: red"]]]))])])], {}, "html"),
    ("r3_01_cell_alignment_keeps_the_style_html4", [Table([Spec("AlignRight")], [], [Body([Row(Cell([PL(S("A"))], attr=["", [], [["style", "color:red"]]], al="AlignCenter"))])])], {}, "html4"),
    ("r3_o01_div_section", [Div(["d", ["c"], []], H(1, "", [S("x")], kv=[["role", "r"], ["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {}, "html"),
    ("r3_o01_div_section_sections", [Div(["d", ["c"], []], H(1, "", [S("x")], kv=[["role", "r"], ["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {"section_divs": True}, "html"),
    ("r3_o01_div_section_html4", [Div(["d", ["c"], []], H(1, "", [S("x")], kv=[["role", "r"], ["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {}, "html4"),
    ("r3_o01_div_section_block", [Div(["d", ["c"], []], H(1, "", [S("x")], kv=[["role", "r"], ["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {"refloc": "block"}, "html"),
    ("r3_o01_div_section_section", [Div(["d", ["c"], []], H(1, "", [S("x")], kv=[["role", "r"], ["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {"refloc": "section", "id_prefix": "p-"}, "html"),
    ("r3_o02_div_section", [Div(["d", ["c"], []], H(1, "h", [S("x"), note("n")]), Pa(S("p"), note("m"))), H(1, "z", [S("z")])], {}, "html"),
    ("r3_o02_div_section_sections", [Div(["d", ["c"], []], H(1, "h", [S("x"), note("n")]), Pa(S("p"), note("m"))), H(1, "z", [S("z")])], {"section_divs": True}, "html"),
    ("r3_o02_div_section_html4", [Div(["d", ["c"], []], H(1, "h", [S("x"), note("n")]), Pa(S("p"), note("m"))), H(1, "z", [S("z")])], {}, "html4"),
    ("r3_o02_div_section_block", [Div(["d", ["c"], []], H(1, "h", [S("x"), note("n")]), Pa(S("p"), note("m"))), H(1, "z", [S("z")])], {"refloc": "block"}, "html"),
    ("r3_o02_div_section_section", [Div(["d", ["c"], []], H(1, "h", [S("x"), note("n")]), Pa(S("p"), note("m"))), H(1, "z", [S("z")])], {"refloc": "section", "id_prefix": "p-"}, "html"),
    ("r3_o03_div_section", [Div(["d", ["c"], [["k", "v"]]], H(1, "", [S("x")], ["c"], [["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {}, "html"),
    ("r3_o03_div_section_sections", [Div(["d", ["c"], [["k", "v"]]], H(1, "", [S("x")], ["c"], [["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {"section_divs": True}, "html"),
    ("r3_o03_div_section_html4", [Div(["d", ["c"], [["k", "v"]]], H(1, "", [S("x")], ["c"], [["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {}, "html4"),
    ("r3_o03_div_section_block", [Div(["d", ["c"], [["k", "v"]]], H(1, "", [S("x")], ["c"], [["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {"refloc": "block"}, "html"),
    ("r3_o03_div_section_section", [Div(["d", ["c"], [["k", "v"]]], H(1, "", [S("x")], ["c"], [["k", "v"]]), Pa(S("p"))), H(1, "z", [S("z")])], {"refloc": "section", "id_prefix": "p-"}, "html"),
    ("r3_i15_div_section", [Div(["", ["c"], [["k", "a"], ["k", "b"]]], H(1, "", [S("x")])), H(1, "z", [S("z")])], {}, "html"),
    ("r3_i15_div_section_sections", [Div(["", ["c"], [["k", "a"], ["k", "b"]]], H(1, "", [S("x")])), H(1, "z", [S("z")])], {"section_divs": True}, "html"),
    ("r3_i15_div_section_html4", [Div(["", ["c"], [["k", "a"], ["k", "b"]]], H(1, "", [S("x")])), H(1, "z", [S("z")])], {}, "html4"),
    ("r3_i15_div_section_block", [Div(["", ["c"], [["k", "a"], ["k", "b"]]], H(1, "", [S("x")])), H(1, "z", [S("z")])], {"refloc": "block"}, "html"),
    ("r3_i15_div_section_section", [Div(["", ["c"], [["k", "a"], ["k", "b"]]], H(1, "", [S("x")])), H(1, "z", [S("z")])], {"refloc": "section", "id_prefix": "p-"}, "html"),
    ("r3_i17_div_section", [Div(["i", [], [["role", "r"]]], H(1, "i", [S("x")], kv=[["role", "r"]])), H(1, "z", [S("z")])], {}, "html"),
    ("r3_i17_div_section_sections", [Div(["i", [], [["role", "r"]]], H(1, "i", [S("x")], kv=[["role", "r"]])), H(1, "z", [S("z")])], {"section_divs": True}, "html"),
    ("r3_i17_div_section_html4", [Div(["i", [], [["role", "r"]]], H(1, "i", [S("x")], kv=[["role", "r"]])), H(1, "z", [S("z")])], {}, "html4"),
    ("r3_i17_div_section_block", [Div(["i", [], [["role", "r"]]], H(1, "i", [S("x")], kv=[["role", "r"]])), H(1, "z", [S("z")])], {"refloc": "block"}, "html"),
    ("r3_i17_div_section_section", [Div(["i", [], [["role", "r"]]], H(1, "i", [S("x")], kv=[["role", "r"]])), H(1, "z", [S("z")])], {"refloc": "section", "id_prefix": "p-"}, "html"),
    ("i_html_15_repeated_div_key_and_heading_key", [Div(["i", [], [["k", "a"], ["k", "b"]]], H(1, "h", [S("x")], kv=[["k", "c"]])), Div(["", ["c"], [["k", "a"], ["k", "b"]]], H(1, "", [S("x")]))], {}, "html"),
    ("i_html_17_same_identifier_and_role", [Div(["i", ["c"], [["role", "r"]]], H(3, "i", [S("x")], kv=[["role", "r"]]))], {}, "html"),
    ("rh16_cell_alignment_before_pairs", [Table([Spec()], [], [Body([Row(Cell([PL(S("A"))], attr=["c", [], [["foo", "v"]]], al="AlignLeft"))])])], {}, "html"),
    ("rh17_table_with_only_a_caption", [Table([], [], [], caption=[PL(S("c"))])], {}, "html"),
    ("rh17_figure_with_only_a_caption", [{"t": "Figure", "c": [A, [None, [PL(S("c"))]], []]}], {}, "html"),
    ("rh12_kbd_and_nesting_of_special_classes", [Pa(Span(["", ["kbd"], []], *X), Span(["", ["mark", "underline"], []], *X), Span(["", ["underline", "mark"], []], *X), Span(["i", ["c", "underline", "d", "smallcaps"], [["k", "v"]]], *X))], {}, "html"),
    ("f03_special_classes_keep_the_attributes", [Pa(Span(["i", ["mark"], [["k", "v"]]], *X), Span(["i", ["smallcaps", "c"], [["k", "v"]]], *X), Span(["i", ["kbd", "c"], [["k", "v"]]], *X))], {}, "html"),
    ("f03_csl_classes_become_styles", [Pa(Span(["i", ["c", "csl-no-emph", "csl-no-strong", "csl-no-smallcaps"], [["style", "color:red"]]], *X))], {}, "html"),
    ("f04_link_inside_a_link_is_a_span", [Pa(Link(A, [Link(["li", ["c"], [["k", "v"]]], [SP, S("in")], "u2", "t2"), {"t": "Emph", "c": [Link(A, X, "u3")]}], "u"))], {}, "html"),
    ("f05_alt_of_a_description_without_text", [Pa(Img(A, [note("n")]), Img(A, [{"t": "Emph", "c": []}]), Img(A, [{"t": "LineBreak"}]), Img(A, []))], {}, "html"),
    ("rh13_image_dimensions", [Pa(*[Img(["", [], [["width", w]]], [S("a")], "x") for w in [".5em", "1000000000000000000000", "1.00000000001cm", "1pt", "3pc", "0.05%", "5.in", "10.7", "1.123456789in", "12345678%"]])], {}, "html"),
    ("f06_image_style_is_merged", [Pa(Img(["", [], [["width", "50%"], ["style", "color:red"], ["k", "v"], ["height", "20"]]], [S("a")], "i", "t"))], {}, "html"),
    ("rh14_combining_marks_have_no_width", [Pa(S("a" * 65), SP, S("᪰" * 5), SP, S("end"))], {}, "html"),
    ("rh15_paragraph_with_only_dropped_raw_content", [Pa({"t": "RawInline", "c": ["latex", "x"]}), Pa(S("y"))], {}, "html"),
    ("f08_raw_block_with_a_final_newline", [{"t": "RawBlock", "c": ["html", "x\n"]}, Pa(S("b"))], {}, "html"),
    ("f08_plain_space_and_line_break_in_items", [PL(SP), Pa(S("a")), {"t": "BulletList", "c": [[PL(S("a"), {"t": "LineBreak"}), {"t": "OrderedList", "c": [[1, {"t": "DefaultStyle"}, {"t": "DefaultDelim"}], [[PL(S("b"))]]]}]]}], {}, "html"),
    ("f09_div_with_the_section_class", [Div(["d", ["section"], []], H(2, "", [S("z")])), Div(["e", ["unnumbered"], []], H(1, "h", [S("z")], cls=["unnumbered"]))], {}, "html"),
    ("f13_level_in_a_note_in_emphasis", [H(1, "", [S("a")]), Pa({"t": "Emph", "c": [{"t": "Note", "c": [H0]}]})], {"number_sections": True}, "html"),
    ("f13_level_in_a_table_cell", [H(1, "", [S("a")]), Table([Spec()], [], [Body([Row(Cell([H0]))])])], {"number_sections": True}, "html"),
    ("f13_level_in_a_figure_and_lists", [H(1, "", [S("a")]), {"t": "Figure", "c": [A, [None, []], [H(2, "", [S("f")])]]}, {"t": "DefinitionList", "c": [[[S("t")], [[H0]]]]}], {"number_sections": True}, "html"),
    ("f13_level_in_a_list", [H(1, "", [S("a")]), {"t": "BulletList", "c": [[H0]]}], {"number_sections": True}, "html"),
    ("f13_numbers_count_from_the_smallest_level", [H(1, "", [S("a")]), H(0, "", [S("b")]), H(1, "", [S("c")]), {"t": "BlockQuote", "c": [H(-1, "", [S("q")])]}], {"number_sections": True}, "html"),
]


def rust_string(text):
    """A Rust string literal; invisible and format characters are escapes."""
    import unicodedata
    out = []
    for c in text:
        if c in '"\\':
            out.append("\\" + c)
        elif c == "\n":
            out.append("\\n")
        elif unicodedata.category(c) in ("Cf", "Cc", "Zs", "Mn", "Zl", "Zp") and c != " ":
            out.append("\\u{%x}" % ord(c))
        else:
            out.append(c)
    return '"' + "".join(out) + '"'


def wrap_tests():
    """The none and preserve cases of the matrix in `tools/differential.py`, for Para blocks."""
    import sys
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from differential import wrap_cases
    out = []
    for name, doc, fmt, opts in wrap_cases():
        mode, index, shape, block = (name.split("/") + ["", "", ""])[:4]
        if mode in ("none", "preserve") and block == "Para" and shape != "in-span":
            out.append((f"wrap_{mode}_{index}_{shape.replace('-', '_')}", doc["blocks"], {"wrap": mode}, fmt))
    return out


def main():
    out = [
        "//! Unit tests with expected texts and warnings from Pandoc 3.13.",
        "//! Generated by `tools/oracle_tests.py`. Do not edit.",
        "",
        "use super::*;",
        "",
        "fn check(doc: &str, options: &WriterOptions, version: Version, expected: &str, warnings: &[&str]) {",
        "    let doc = randoc_ast::json::from_json(doc).unwrap();",
        "    let mut context = Context::pure();",
        "    let output = write(&doc, options, &mut context, version).unwrap();",
        "    // The program adds the final newline of the oracle output.",
        "    assert_eq!(output, expected.strip_suffix('\\n').unwrap_or(expected));",
        "    let log: Vec<String> = context.log().iter().map(LogMessage::render).collect();",
        "    assert_eq!(log, warnings);",
        "}",
        "",
    ]
    for name, blocks, opts, fmt in OLD + REVIEW + wrap_tests():
        setup, args = [], []
        for key, value in opts.items():
            line, extra = option_code(key, value)
            setup.append(line)
            args += extra
        target = fmt + opts.get("ext", "")
        doc = json.dumps({"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": blocks}, ensure_ascii=True)
        r = subprocess.run([P, "-f", "json", "-t", target, *args], input=doc.encode(), capture_output=True, check=True)
        expected = r.stdout.decode()
        warnings = [line + "\n" for line in r.stderr.decode().splitlines() if line.startswith("[WARNING]")]
        version = "Version::Html4" if fmt == "html4" else "Version::Html5"
        out.append("#[test]")
        out.append(f"fn {name}() {{")
        mutable = "mut " if setup else ""
        out.append(f"    let {mutable}options = WriterOptions::default();")
        out += ["    " + line for line in setup]
        out.append(f'    let doc = r###"{doc}"###;')
        out.append(f"    let expected = {rust_string(expected)};")
        out.append("    let warnings: &[&str] = &[" + ", ".join(rust_string(w) for w in warnings) + "];")
        out.append(f"    check(doc, &options, {version}, expected, warnings);")
        out.append("}")
        out.append("")
    path = os.path.join(os.path.dirname(__file__), "..", "src", "option_tests.rs")
    with open(path, "w") as f:
        f.write("\n".join(out))
    print(len(OLD + REVIEW + wrap_tests()), "tests")


if __name__ == "__main__":
    main()
