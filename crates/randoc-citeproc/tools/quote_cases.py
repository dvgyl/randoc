"""Writes tests/replay/quotes-{cases.txt,refs.json}: quote terms of a style
locale x texts with quotation marks x layouts (review citeproc-R3b-05: the
text of a style is parsed with the quotes of the locale). Each case line has
the locale, the item, the layout attributes and the layout content.
Usage (from the crate folder): python3 tools/quote_cases.py, then
pandoc lua tools/quotes.lua SCRATCH_DIR"""
import json
from xml.sax.saxutils import escape, quoteattr

REFS = [
    {"id": "q1", "type": "book", "title": "Ways “to” swim"},
    {"id": "q2", "type": "book", "title": "Ways «to» swim"},
    {"id": "q3", "type": "book", "title": "Ways „to“ swim"},
]

# (outer open, outer close, inner open, inner close); None: no terms.
QUOTE_SETS = [
    None,
    ("„", "“", "‚", "‘"),
    ("«", "»", "‹", "›"),
    ("「", "」", "『", "』"),
    ("»", "«", "›", "‹"),
    ("‚‚", "‘‘", "«", "»"),
]

TEXTS = [
    "„x“", "«x»", "“x”", "‘x’", "‹x›",
    "「x」", "»x«", "\"x\"", "'x'", "„a ‚b‘ c“",
    "« x »", "«x» y", "„“", "x„y“z",
    "‚‚x‘‘", "«x ‹y›»",
]

LAYOUTS = [
    ("q1", "", '<text value={v}/>'),
    ("q1", "", '<text value={v} suffix="."/>'),
    ("q1", "", '<text value="z" prefix={v} suffix=","/>'),
    ("q1", "", '<text term="qt"/>'),
    ("q1", 'delimiter=", "', '<text value={v}/><text value="w"/>'),
]

TITLES = [
    ("q1", "", '<text variable="title"/>'),
    ("q2", "", '<text variable="title"/>'),
    ("q3", "", '<text variable="title" quotes="true"/>'),
]


def locale(quotes, piq, text):
    terms = f'<term name="qt">{escape(text)}</term>'
    if quotes is not None:
        names = ["open-quote", "close-quote", "open-inner-quote", "close-inner-quote"]
        terms += "".join(f'<term name="{n}">{escape(q)}</term>' for n, q in zip(names, quotes))
    return (f'<locale><style-options punctuation-in-quote="{piq}"/>'
            f"<terms>{terms}</terms></locale>")


def main():
    lines = []
    for quotes in QUOTE_SETS:
        for piq in ["true", "false"]:
            for text in TEXTS:
                for item, attrs, layout in LAYOUTS:
                    lines.append("\t".join([locale(quotes, piq, text), item, attrs,
                                            layout.format(v=quoteattr(text))]))
            for item, attrs, layout in TITLES:
                lines.append("\t".join([locale(quotes, piq, "t"), item, attrs, layout]))
    with open("tests/replay/quotes-cases.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    with open("tests/replay/quotes-refs.json", "w", encoding="utf-8") as f:
        f.write("[\n" + ",\n".join(json.dumps(r, ensure_ascii=False) for r in REFS) + "\n]\n")


main()
