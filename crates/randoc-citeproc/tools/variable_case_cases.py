"""Writes tests/replay/case-{cases.txt,refs.json,locale.xml}: variable names
in other letter cases (review Codex citeproc-R3b-02), dates with `circa` only
(R3b-03) and the case of letters that are new in Unicode 16.0 (R3b-08).
Usage (from the crate folder): python3 tools/variable_case_cases.py, then
pandoc lua tools/eval.lua SCRATCH_DIR case"""
import json

REFS = [
    {"id": "v1", "type": "book", "title": "TiꟋtle ɤx", "page": "12-19", "DOI": "10.1/x",
     "URL": "example.org/u", "PMID": "77", "volume": "3", "number-of-volumes": "2",
     "issued": {"date-parts": [[2001]]}},
    {"id": "v2", "type": "article", "title": "\U00010d50\U00010d70 aᲉ", "page": "7",
     "issued": {"circa": True}},
    {"id": "v3", "type": "chapter", "issued": {"circa": True, "season": 2}},
    {"id": "v4", "type": "report", "issued": {"date-parts": [[1999]], "circa": True}},
]

VARIABLES = ["ID", "Id", "TYPE", "Page", "PAGE-FIRST", "doi", "url", "Url", "pmid", "TITLE",
             "Volume", "LOCATOR", "Year-Suffix", "Citation-Label"]

LAYOUTS = []
for v in VARIABLES:
    LAYOUTS.append(f'<text variable="{v}"/>')
    LAYOUTS.append(f'<choose><if variable="{v}"><text value="y"/></if><else><text value="n"/></else></choose>')
LAYOUTS += [
    '<text variable="TITLE" form="short"/>',
    '<number variable="VOLUME"/>',
    '<label variable="Number-Of-Volumes"/>',
    '<choose><if is-numeric="PAGE"><text value="y"/></if><else><text value="n"/></else></choose>',
    '<choose><if is-uncertain-date="ISSUED"><text value="y"/></if><else><text value="n"/></else></choose>',
    '<choose><if variable="issued"><text value="y"/></if><else><text value="n"/></else></choose>',
    '<choose><if is-uncertain-date="issued"><text value="y"/></if><else><text value="n"/></else></choose>',
    '<group delimiter=" "><text value="g"/><text variable="issued"/></group>',
    '<group><text value="g"/><date variable="issued"><date-part name="year"/></date></group>',
    '<text variable="title" text-case="lowercase"/>',
    '<text variable="title" text-case="uppercase"/>',
    '<text variable="title" text-case="capitalize-all"/>',
    '<text variable="title" text-case="title"/>',
    '<text variable="title" text-case="sentence"/>',
]


def main():
    lines = [f"{r['id']}\t\t{layout}" for r in REFS for layout in LAYOUTS]
    with open("tests/replay/case-cases.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    with open("tests/replay/case-refs.json", "w", encoding="utf-8") as f:
        f.write("[\n" + ",\n".join(json.dumps(r, ensure_ascii=False) for r in REFS) + "\n]\n")
    with open("tests/replay/case-locale.xml", "w", encoding="utf-8") as f:
        f.write('<locale><terms><term name="number-of-volumes"><single>vol.</single>'
                '<multiple>vols.</multiple></term><term name="volume">volume</term><term name="page-range-delimiter">\u2013</term></terms></locale>\n')


main()
