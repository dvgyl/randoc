#!/usr/bin/env python3
"""Writes the case files of the conformance suite `csljson`.

The cases are the oracle matrix of the reader and the writer of the format
`csljson`. Each input is new and written for Randoc. Run this tool, then
`randoc-harness record` for the suite.

Usage: python3 -I crates/randoc-citeproc/tools/csljson_cases.py OUTPUT_FOLDER
"""

import json
import sys
from pathlib import Path


def item(**fields):
    return {"id": "k", **fields}


def items_json(*objects):
    return json.dumps(list(objects), ensure_ascii=False)


# Reader: one item for each value form. The output formats show the tree
# (`read`), the round trip (`e2e:csljson`) and the writer (`write:csljson`).
VALUES = {
    "type-and-id": [{"id": "a", "type": "book"}, {"id": 7}, {"title": "no id"}],
    "standard-text": [item(title="Plain title", publisher="P", abstract="A")],
    "standard-number": [item(title=12, publisher=-3)],
    "string-variables": [item(DOI="10.1/x'y", ISBN="12", URL="http://e.org/?a=1&b", PMID=5)],
    "number-forms": [item(volume=3, issue="4", page="12-15", edition="2nd", number=-2, section=1e3)],
    "unknown-variables": [item(zzz="q", zzy=12, zzb=True, zza=[1], custom={"a": 1})],
    "key-case": [{"id": "k", "Title": "Upper", "ZZZ": "z", "pmcid": "p", "Issued": "2001"}],
    "key-case-collision": [{"id": "k", "Title": "first", "title": "second"}],
    "legacy-keys": [item(journalAbbreviation="J. A."), item(shortTitle="Short")],
    "names-object": [item(author=[{"family": "Ampel", "given": "Anna", "dropping-particle": "van",
                                   "non-dropping-particle": "de", "suffix": "Jr.", "comma-suffix": True,
                                   "static-ordering": "true"}, {"literal": "ACME Group"}, {}])],
    "names-quotes": [item(author=[{"family": "O'Brien", "given": "\"Pat, Jr.\""},
                                  {"family": "\"Quoted Name\"", "suffix": "d'x"}])],
    "names-cheater": [item(editor=["Doe || John", "de la Fontaine || Jean", "van Berg || Anna, Jr.",
                                   "Solo", "d'Arc || Jeanne,! III", "\u24d0x Y || g", "\u00aax || g"])],
    "names-flags": [item(author=[{"family": "F", "comma-suffix": 1, "static-ordering": "yes"},
                                 {"family": "G", "comma-suffix": None}])],
    "dates-parts": [item(issued={"date-parts": [[2001, 2, 3]]}, accessed={"date-parts": [[2001], [2002, 5]]},
                         submitted={"date-parts": [["2001", "02", ""]]})],
    "dates-circa-season": [item(issued={"date-parts": [[2001]], "circa": True, "season": 2},
                                accessed={"date-parts": [[1999, 5]], "circa": "true"},
                                submitted={"date-parts": [[1999]], "circa": 1, "season": "x"})],
    "dates-literal": [item(issued={"literal": "Spring of 2001"},
                           accessed={"literal": "L", "date-parts": [[2001]], "circa": True})],
    "dates-raw": [item(issued={"raw": "2001-02-03/2004-05"}, accessed={"raw": "199u"},
                       submitted={"raw": "2001/open"}, **{"event-date": {"raw": "May 2001"},
                       "original-date": {"raw": "spring 1999"}, "available-date": {"raw": "a while ago"}})],
    "dates-raw-edtf": [item(issued="-0044", accessed="0000", submitted="2001~",
                            **{"event-date": "y12345", "original-date": "2001-02-03T10:00"})],
    "dates-years": [item(issued={"date-parts": [[-1]]}, accessed={"date-parts": [[-50]]},
                         submitted={"date-parts": [[12345]]}, **{"event-date": {"date-parts": [[5]]},
                         "original-date": {"date-parts": [[0, 5]]}, "available-date": {"date-parts": [[-10001]]}})],
    "dates-odd-parts": [item(issued={"date-parts": []}, accessed={"date-parts": [[2001], []]},
                             submitted={"date-parts": [[2001, 13, 40, 4]]}, **{"event-date": {}})],
    "note-fields": [item(note="original-date: 1990\nauthor: Doe || J.\nauthor: Roe\nrest of note"),
                    item(note="{:issued: 2001}{:volume: 7} more")],
    "duplicate-ids": [{"id": "x", "title": "A"}, {"id": "x", "title": "B"}],
    "duplicate-keys": '[{"id":"x","id":"y","title":"a","Title":"b"}]',
    "names-particles": [item(author=[{"family": "van der Berg", "given": "Anna Maria de"}],
                             editor=["-Smith || x", "van der || y", "d\u2019Arc || z", " || ", "a||b||c"])],
}

MARKUP = [
    "<i>it</i> <b>bo</b> <u>un</u> <sup>up</sup> <sub>dn</sub>",
    "<sc>a</sc> <span style=\"font-variant:small-caps;\">b</span> <span style=\"font-variant: small-caps; \" >c</span>",
    "<span class=\"nocase\">NASA</span> <span  class=\"nocase\" >x</span> <span class=\"nodecor\">d</span> <span class=\"baseline\">e</span>",
    "\"double 'single' double\" \u201ccurly\u201d \u2018c\u2019",
    "don't it's l'\u00e9t\u00e9 'quoted' x\"y\"z",
    "\" open space\" \"\" ''",
    "<i>open <b>never closed",
    "a < b > c & d &amp; e",
    "x ; y ? z ! \u00ab w \u00bb",
    "\u00b2 \u2122 a\u00b2 \u00aa",
    "  lead  trail  ",
    "line\nbreak\ttab\u00a0nbsp",
    "<i><i>nested</i></i> <b><b>x</b></b> <sc><sc>y</sc></sc>",
    "<i>a</i><i>b</i> <sc>c</sc><sc>d</sc> <b>e</b><b>f</b>",
    "<i></i><b> </b>",
    "<a href=\"x\">l</a> <span class=\"other\">o</span>",
    "x\u093f'y \u093f'y \u0301'z a\u0345'b \u24b6'c",
]

ERRORS = {
    "empty-input": "",
    "not-json": "nope",
    "object-without-items": "{\"id\":\"x\"}",
    "items-not-array": "{\"items\":5}",
    "items-form": "{\"items\":[{\"id\":\"x\"}],\"other\":1}",
    "item-in-items-not-object": "{\"items\":[5]}",
    "root-string": "\"x\"",
    "root-number": "5",
    "item-not-object": "[1]",
    "trailing-garbage": "[{\"id\":\"a\"}] x",
    "trailing-space": "[{\"id\":\"a\"}]  \n",
    "empty-list": "[]",
    "unclosed-array": "[{\"id\":",
    "names-not-array": items_json(item(author="Smith")),
    "name-not-object": items_json(item(author=[{"family": "x"}, 5])),
    "name-part-number": items_json(item(author=[{"family": 5}])),
    "name-flag-array": items_json(item(author=[{"family": "x", "comma-suffix": [1]}])),
    "date-number": items_json(item(issued=5)),
    "date-parts-number": items_json(item(issued={"date-parts": 5})),
    "date-parts-inner-number": items_json(item(issued={"date-parts": [5]})),
    "date-part-text": items_json(item(issued={"date-parts": [["x"]]}, volume=True)),
    "date-part-float": items_json(item(issued={"date-parts": [[2001.5]]})),
    "date-part-bool": items_json(item(issued={"date-parts": [[True]]})),
    "date-literal-number": items_json(item(issued={"literal": 5})),
    "date-raw-number": items_json(item(issued={"raw": 5, "date-parts": [[2001]]})),
    "first-error-in-key-order": items_json(item(volume=True, author=5)),
    "standard-bool": items_json(item(title=True)),
    "standard-null": items_json(item(title=None)),
    "standard-array": items_json(item(title=["x"])),
    "standard-float": items_json(item(title=1.5)),
    "number-float": items_json(item(volume=3.5)),
    "id-float": items_json({"id": 1.5}),
    "id-huge": "[{\"id\":1e30}]",
    "type-number": items_json({"id": "a", "type": 5}),
    "note-number": items_json(item(note=5)),
    "unknown-float": items_json(item(**{"a b": 5.5})),
    "unknown-key-quote": items_json(item(**{"x'y": True})),
}

# Writer: metadata of each form in a native document.
def native_doc(fields):
    return "Pandoc (Meta {unMeta = fromList [(\"references\"," + fields + ")]}) []\n"


def ref(*pairs):
    return "MetaMap (fromList [" + ",".join(f"(\"{k}\",{v})" for k, v in pairs) + "])"


S = lambda t: "MetaString " + json.dumps(t, ensure_ascii=False)
I = lambda t: "MetaInlines [Str " + json.dumps(t, ensure_ascii=False) + "]"

WRITER = {
    "no-references": "Pandoc (Meta {unMeta = fromList []}) []\n",
    "references-not-list": native_doc(S("x")),
    "references-map": native_doc(ref(("id", S("a")))),
    "list-with-non-maps": native_doc("MetaList [" + S("x") + "," + ref(("id", S("a"))) + ",MetaBool True]"),
    "value-forms": native_doc("MetaList [" + ref(
        ("id", I("a")), ("type", "MetaInlines [Emph [Str \"book\"]]"),
        ("DOI", S("a'b <i>x</i>")), ("title", "MetaInlines [Str \"x\",Space,Emph [Str \"e\"],Space,Strong [Str \"s\"],SoftBreak,SmallCaps [Str \"c\"],LineBreak,Quoted SingleQuote [Str \"q\",Space,Quoted DoubleQuote [Str \"i\"]]]"),
        ("publisher", "MetaBool True"), ("abstract", "MetaList [" + S("x") + "]"),
        ("keyword", "MetaMap (fromList [(\"k\"," + S("v") + ")])"), ("volume", S("3")),
        ("page", "MetaInlines [Str \"12\\8211\\&15\"]"), ("container-title", S(""))) + "]"),
    "inline-forms": native_doc("MetaList [" + ref(("id", S("a")), ("title",
        "MetaInlines [Str \"x\",Note [Para [Str \"n\"]],Space,Cite [Citation {citationId = \"k\", citationPrefix = [], citationSuffix = [], citationMode = NormalCitation, citationNoteNum = 0, citationHash = 0}] [Str \"[@k]\"],Space,Math DisplayMath \"y\",Space,Math InlineMath \"z\",Code (\"\",[],[]) \"c\",RawInline (Format \"html\") \"<b>r</b>\",Span (\"\",[\"csl-quoted\"],[]) [Str \"cq\"],Span (\"\",[\"nodecoration\"],[]) [Str \"nd\"],Span (\"\",[\"nocase\"],[]) [Str \"NC\"],Strikeout [Str \"s\"],Underline [Str \"u\"],Superscript [Str \"p\"],Subscript [Str \"b\"],Link (\"\",[],[]) [Str \"L\"] (\"u\",\"\"),Image (\"\",[],[]) [Str \"I\"] (\"p\",\"\"),Str \"\\178\"]")) + "]"),
    "block-forms": native_doc("MetaList [" + ref(("id", S("a")), ("abstract",
        "MetaBlocks [Para [Str \"p1\"],Plain [Str \"p2\"],BulletList [[Plain [Str \"i1\"]],[Para [Str \"i2\"],Para [Str \"i2b\"]]],CodeBlock (\"\",[],[]) \"cb\",BlockQuote [Para [Str \"bq\"]],HorizontalRule,Header 1 (\"\",[],[]) [Str \"h\"],Div (\"\",[],[]) [Para [Str \"d1\"],Para [Str \"d2\"]],RawBlock (Format \"html\") \"<p>raw</p>\",LineBlock [[Str \"l1\"],[Str \"l2\"]]]"),
        ("note", "MetaBlocks [Para [Str \"only\"]]")) + "]"),
    "names": native_doc("MetaList [" + ref(("id", S("a")),
        ("author", "MetaList [" + ref(("family", "MetaInlines [Emph [Str \"F\"],Space,Str \"G\"]"), ("given", S("a'b")),
                                      ("comma-suffix", S("yes")), ("static-ordering", "MetaBool True"),
                                      ("literal", "MetaList [" + S("l") + "]"), ("zzz", S("q"))) +
         ",MetaBool True,MetaBlocks [Para [Str \"bb\"]]," + S("Doe, J.") + "]"),
        ("editor", S("Solo")), ("translator", ref(("family", S("T")))), ("illustrator", "MetaList []")) + "]"),
    "dates": native_doc("MetaList [" + ref(("id", S("a")),
        ("issued", I("2001-02-03/2004")), ("submitted", S("May 2001")), ("original-date", S("199u")),
        ("accessed", "MetaMap (fromList [(\"date-parts\",MetaList [MetaList [" + I("2001") + "," + S("02") + "," + S("x") + "]]),(\"season\"," + I("x") + "),(\"circa\",MetaBool True),(\"literal\",MetaInlines [Emph [Str \"lit\"]])])"),
        ("event-date", "MetaMap (fromList [(\"year\"," + S("2001") + "),(\"month\"," + S("x") + ")])"),
        ("available-date", "MetaMap (fromList [(\"raw\"," + S("2001-02") + "),(\"literal\"," + S("z") + ")])"),
        ("container", "MetaMap (fromList [(\"date-parts\",MetaList [" + S("2001") + "])])")) + "]"),
    "dates-more": native_doc("MetaList [" + ref(("id", S("a")),
        ("issued", "MetaMap (fromList [(\"date-parts\",MetaList [MetaList [" + S("2001") + "]]),(\"season\"," + S("2") + "),(\"circa\"," + S("true") + ")])"),
        ("submitted", "MetaList [" + S("2001") + "]"), ("accessed", "MetaBool True"),
        ("original-date", S("")), ("event-date", S("~")), ("available-date", S("-0049/y12345"))) + "]"),
    "key-forms": native_doc("MetaList [" + ref(("id", S("a")), ("Title", S("T1")), ("title", S("T2")),
        ("Doi", S("d")), ("pmcid", S("p")), ("ZZZ", S("z")), ("Volume", S("3"))) + "]"),
    "variable-kinds": native_doc("MetaList [" + ref(("id", S("a")), *[(k, S("2001")) for k in
        "author chair collection-editor composer compiler container-author contributor curator director editor "
        "editor-translator editorial-director executive-producer guest host illustrator interviewer narrator "
        "original-author organizer performer producer recipient reviewed-author script-writer series-creator "
        "translator accessed available-date container event-date issued original-date submitted".split()]) + "]"),
    "escapes": native_doc("MetaList [" + ref(("id", S("a\"b\\c")), ("title", S("t\u0001\u001f\t\n\r/<>&\u2028"))) + "]"),
}


def toml_string(text):
    return json.dumps(text, ensure_ascii=False)


def write_cases(path, header, defaults, cases):
    lines = [f"# {header}"] + defaults + [""]
    for case_id, text in cases:
        lines += ["", "[[case]]", f"id = {toml_string(case_id)}", f"input = {toml_string(text)}"]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main():
    out = Path(sys.argv[1])
    write_cases(out / "values.toml", "The reader: each value form of each variable type.",
                ['from = "csljson"', 'to = ["csljson", "native"]'],
                [(k, v if isinstance(v, str) else items_json(*v)) for k, v in VALUES.items()])
    write_cases(out / "markup.toml", "The markup inside CSL JSON strings: tags, quotes, escapes and text that is not markup.",
                ['from = "csljson"', 'to = ["csljson"]'],
                [(f"markup-{n:02}", items_json(item(title=t))) for n, t in enumerate(MARKUP)])
    write_cases(out / "errors.toml", "The forms of the input and the error texts of bad input.",
                ['from = "csljson"', 'to = ["csljson"]'], list(ERRORS.items()))
    write_cases(out / "writer.toml", "The writer: references metadata of each form.",
                ['from = "native"', 'to = ["csljson"]'], list(WRITER.items()))


main()
