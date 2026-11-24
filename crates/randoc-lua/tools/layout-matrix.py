#!/usr/bin/env python3
"""Write the layout and template replay matrix."""

from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]
setup = r"""local L=P.layout; local a,b=L.literal'alpha',L.literal'beta'
local two=a/b; local spaced=a..L.space..b"""

unary = ("before_non_blank", "bold", "italic", "underlined", "strikeout", "braces", "brackets",
         "parens", "quotes", "double_quotes", "chomp", "flush", "height", "is_empty", "min_offset",
         "offset", "nestle", "nowrap")
parameters = {
    "literal": ["", "{}", "123", "'a\\nb'"],
    "after_break": ["", "{}", "'>'"],
    "blanklines": ["", "1.5", "-2", "'2'"],
    "fg": ["", "a", "a, {}", "a, 'invalid'", "a, 'red'"],
    "bg": ["", "a", "a, {}", "a, 'invalid'", "a, 'blue'"],
    "hang": ["", "two", "two, {}", "two, 4", "two, 4, {}", "two, 4, '- '"],
    "inside": ["", "a", "a, {}", "a, '<'", "a, '<', {}", "a, '<', '>'"],
    "concat": ["", "a", "{}", "{a,{}}", "{1,2}", "{a,b}, {}", "{a,b}, ', '"],
    "prefixed": ["", "two", "two, {}", "a//b, '> '"],
    "real_length": ["", "a", "'日本a'", "123"],
    "render": ["", "{}", "spaced", "spaced, 0", "spaced, -3", "spaced, 1.5",
               "spaced, 'bad'", "spaced, 4, {}", "spaced, 4, 'bad'", "spaced, 4, 'plain'",
               "L.bold(a), nil, 'ansi'", "L.fg(a, 'red'), nil, 'ansi'"],
    "update_column": ["", "a", "a, {}", "a, 7", "two, 7", "a, 0", "a, -7"],
    "vfill": ["", "{}", "'|'"],
}

matrix["write_matrix"]("layout", {
    "unary": (setup, [
        *[probe for name in unary for probe in calls("L." + name, ["", "{}", "a", "''", "3"])],
        ("after break", "return (a/L.after_break('>')..b):render()"),
        ("before nonblank", "return (a..L.before_non_blank('>')..b):render()"),
        ("before blank", "return (a..L.before_non_blank('>')/b):render()"),
        ("chomp", "return L.chomp(a..L.blankline)..b"),
        ("fill rows", "return L.vfill('|')..two"),
    ]),
    "parameters": (setup, [
        probe for name, args in parameters.items() for probe in calls("L." + name, args)
    ]),
    "geometry": (setup, [
        *[probe for name in ("cblock", "lblock", "rblock", "nest")
          for probe in calls("L." + name, ["", "{}", "two", "two, {}", "two, -1", "two, '2'", "two, 8"])],
        ("flush nested", "return (a/L.flush(b)):nest(2)"),
        ("nowrap", "return spaced:nowrap():render(5)"),
        ("blankline count", "return a..L.blanklines(3)..b"),
        ("nested prefix", "return two:prefixed('> '):nest(1)"),
    ]),
    "typed": (setup, [
        ("constants", "return L.empty,L.space,L.cr,L.blankline"),
        ("constant identity", "return rawequal(L.space,L.space),type(rawget(L,'space'))"),
        ("userdata", "return type(a),P.utils.type(a),getmetatable(a),keys(a),a.unknown"),
        ("assign", "a.extra=1"),
        ("concat operands", "return 1 .. a .. 'x'"),
        ("concat error", "return a..{}"),
        ("plus", "return a+b,a+L.empty+b"),
        ("div", "return a/b,a/L.empty/b"),
        ("idiv", "return a//b,a//L.empty//b"),
        ("comparison", "return a==L.literal'alpha',a==b,a=='alpha'"),
        ("less", "return a<b"),
        ("length", "return #a"),
        ("template Doc", "local d=P.template.apply('Hello $x$',{x=a}); return d,d:braces(),P.utils.type(d),P.template.apply('$d$',{d=d})"),
    ]),
})
