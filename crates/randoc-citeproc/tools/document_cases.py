#!/usr/bin/env python3
"""Writes tests/replay/document.txt: whole documents through the citation
filter of the oracle (Pandoc 3.13). For each case: the identifier, the
style, the input document as Pandoc JSON (from the oracle's Markdown
reader) and the document after `--citeproc` (Pandoc JSON).

The styles, the locale and the references are new and written for this
test; none is copied. Each style holds the locale of
tests/replay/document-locale.xml, so that the built-in locale of the
oracle and the data-directory locale of Randoc give the same terms.

Usage (from the crate folder):
    python3 -I tools/document_cases.py "$RANDOC_ORACLE_PANDOC"
"""

import json
import os
import subprocess
import sys
import tempfile

LOCALE = (
    '<locale xmlns="http://purl.org/net/xbiblio/csl" version="1.0" xml:lang="en-US">'
    '<style-options punctuation-in-quote="true"/><terms>'
    '<term name="and">and</term><term name="and" form="symbol">&amp;</term>'
    '<term name="et-al">et al.</term><term name="ibid">ibid.</term>'
    '<term name="no date" form="short">n.d.</term><term name="bc">BC</term><term name="ad">AD</term>'
    '<term name="open-quote">“</term><term name="close-quote">”</term>'
    '<term name="open-inner-quote">‘</term><term name="close-inner-quote">’</term>'
    '<term name="page-range-delimiter">–</term>'
    '<term name="page"><single>page</single><multiple>pages</multiple></term>'
    '<term name="page" form="short"><single>p.</single><multiple>pp.</multiple></term>'
    '<term name="chapter"><single>chapter</single><multiple>chapters</multiple></term>'
    '<term name="chapter" form="short"><single>chap.</single><multiple>chaps.</multiple></term>'
    '<term name="section"><single>section</single><multiple>sections</multiple></term>'
    '<term name="section" form="short"><single>sec.</single><multiple>secs.</multiple></term>'
    '<term name="section" form="symbol"><single>§</single><multiple>§§</multiple></term>'
    '</terms></locale>'
)

REFS = [
    {"id": "s1", "type": "book", "author": [{"family": "Smith", "given": "John"}], "issued": {"date-parts": [[2000]]}, "title": "Alpha"},
    {"id": "s2", "type": "book", "author": [{"family": "Smith", "given": "Jane"}], "issued": {"date-parts": [[2000]]}, "title": "Beta"},
    {"id": "s3", "type": "book", "author": [{"family": "Smith", "given": "John"}], "issued": {"date-parts": [[2000]]}, "title": "Gamma"},
    {"id": "s4", "type": "book", "author": [{"family": "Smith", "given": "John"}, {"family": "Doe", "given": "Ann"}], "issued": {"date-parts": [[2001]]}, "title": "Delta"},
    {"id": "s5", "type": "book", "author": [{"family": "Smith", "given": "John"}, {"family": "Roe", "given": "Bob"}], "issued": {"date-parts": [[2001]]}, "title": "Epsilon"},
    {"id": "d1", "type": "article-journal", "author": [{"family": "de Vries", "given": "Hugo"}], "issued": {"date-parts": [[1999, 5, 3]]}, "title": "Zeta", "container-title": "Journal of Things", "DOI": "10.1/xyz"},
    {"id": "d2", "type": "book", "editor": [{"family": "Åhl", "given": "Cy"}], "issued": {"date-parts": [[-44]]}, "title": "Eta", "URL": "https://example.com/eta"},
    {"id": "d3", "type": "book", "author": [{"literal": "World Health Organization"}], "title": "Theta", "issued": {"date-parts": [[2010]]}},
    {"id": "d4", "type": "book", "title": "Iota"},
    {"id": "s6", "type": "book", "author": [{"family": "Smith", "given": "J."}], "issued": {"date-parts": [[2000]]}, "title": "Kappa"},
    {"id": "z", "type": "book", "author": [{"family": "Zeller", "given": "Ann"}], "issued": {"date-parts": [[2001]]}, "title": "Zed", "container-title": "Journal of Interesting Things"},
    {"id": "o", "type": "book", "author": [{"family": "Ödegaard", "given": "Bo"}], "issued": {"date-parts": [[2001]]}, "title": "Oh"},
    {"id": "a", "type": "book", "author": [{"family": "Ahl", "given": "Cy"}], "issued": {"date-parts": [[1999]]}, "title": "Ah"},
]

ABBREVIATIONS = {"default": {"container-title": {"Journal of Interesting Things": "J. Int. Things"}}}


def style(cls, citation, bibliography="", attrs=""):
    return (
        f'<style xmlns="http://purl.org/net/xbiblio/csl" class="{cls}" version="1.0"{attrs}>'
        + LOCALE + citation + bibliography + "</style>"
    )


AUTHOR = ('<macro name="author"><names variable="author"><name form="short" and="symbol" initialize-with=". "/>'
          '<substitute><names variable="editor"/><text variable="title" font-style="italic"/></substitute></names></macro>'
          '<macro name="year"><date variable="issued"><date-part name="year"/></date></macro>')
AD_BIB = ('<bibliography subsequent-author-substitute="———" hanging-indent="true">'
          '<sort><key macro="author"/><key variable="issued" sort="descending"/><key variable="title"/></sort>'
          '<layout suffix="."><group delimiter=". "><names variable="author"><name name-as-sort-order="all"/>'
          '<substitute><names variable="editor"/></substitute></names><date variable="issued"><date-part name="year"/></date>'
          '<text variable="title" font-style="italic"/><text variable="container-title"/><text variable="DOI" prefix="https://doi.org/"/>'
          '</group></layout></bibliography>')


def author_date(options):
    return style(
        "in-text",
        AUTHOR + f'<citation et-al-min="2" et-al-use-first="1" {options}><sort><key macro="author"/><key macro="year"/></sort>'
        '<layout prefix="(" suffix=")" delimiter="; "><group delimiter=" "><text macro="author"/><text macro="year"/>'
        '<group delimiter=" "><label variable="locator" form="short"/><text variable="locator"/></group></group>'
        '<choose><if disambiguate="true"><text value=" D"/></if></choose></layout></citation>',
        AD_BIB,
        ' demote-non-dropping-particle="sort-only"',
    )


FULL = 'disambiguate-add-names="true" disambiguate-add-givenname="true" disambiguate-add-year-suffix="true" collapse="year-suffix"'
STYLES = {
    "ad": author_date(FULL + ' givenname-disambiguation-rule="by-cite"'),
    "ad-all": author_date(FULL + ' givenname-disambiguation-rule="all-names"'),
    "ad-allini": author_date(FULL + ' givenname-disambiguation-rule="all-names-with-initials"'),
    "ad-prim": author_date(FULL + ' givenname-disambiguation-rule="primary-name"'),
    "ad-primini": author_date(FULL + ' givenname-disambiguation-rule="primary-name-with-initials"'),
    "ad-year": author_date(FULL.replace("year-suffix\"", "year\"") + ' givenname-disambiguation-rule="by-cite"'),
    "ad-ranged": author_date(FULL.replace("year-suffix\"", "year-suffix-ranged\"")),
    "ad-ys": author_date('disambiguate-add-year-suffix="true"'),
    "ad-none": author_date(''),
    "num": style(
        "in-text",
        '<citation collapse="citation-number"><sort><key variable="citation-number"/></sort>'
        '<layout prefix="[" suffix="]" delimiter=","><text variable="citation-number"/></layout></citation>',
        '<bibliography second-field-align="flush" entry-spacing="0" line-spacing="2"><layout>'
        '<text variable="citation-number" prefix="[" suffix="]"/><text variable="title" display="block"/>'
        '<text variable="container-title" form="short"/></layout></bibliography>',
    ),
    "num-desc": style(
        "in-text",
        '<citation><layout prefix="[" suffix="]" delimiter=","><text variable="citation-number"/></layout></citation>',
        '<bibliography><sort><key variable="citation-number" sort="descending"/></sort><layout>'
        '<text variable="citation-number" suffix=". "/><text variable="title"/></layout></bibliography>',
    ),
    "note": style(
        "note",
        '<citation near-note-distance="2"><layout delimiter="; " suffix="."><group delimiter=", ">'
        '<choose><if position="ibid-with-locator"><text term="ibid"/></if><else-if position="ibid"><text term="ibid"/></else-if>'
        '<else-if position="subsequent"><names variable="author"><name form="short"/></names>'
        '<text variable="first-reference-note-number" prefix="n. "/></else-if>'
        '<else><names variable="author"/><text variable="title" font-style="italic"/></else></choose>'
        '<text variable="locator"/></group></layout></citation>',
        '<bibliography><layout><text variable="title"/></layout></bibliography>',
    ),
    # A citation that ends with a period takes the place of a period after it.
    "dot": style(
        "in-text",
        '<citation><layout suffix="." delimiter="; "><group delimiter=" ">'
        '<names variable="author"><name form="short"/></names>'
        '<date variable="issued"><date-part name="year"/></date></group></layout></citation>',
        '<bibliography><layout><text variable="title"/></layout></bibliography>',
    ),
    "suffix-var": style(
        "in-text",
        '<citation disambiguate-add-year-suffix="true" collapse="year"><layout prefix="(" suffix=")" delimiter="; ">'
        '<group delimiter=" "><names variable="author"><name form="short"/></names>'
        '<date variable="issued"><date-part name="year"/></date></group></layout></citation>',
        '<bibliography><layout><date variable="issued"><date-part name="year"/></date><text variable="year-suffix"/></layout></bibliography>',
    ),
}

DOCS = [
    ("ad", "ad", "A [@s1; @s2; @s3]. B [@s4; @s5]. C [@d1; @d2; @d3; @d4; @s6]. D @s1 [p. 3]. E [@s3, 4].\n"),
    ("ad-all", "ad-all", None),
    ("ad-allini", "ad-allini", None),
    ("ad-prim", "ad-prim", None),
    ("ad-primini", "ad-primini", None),
    ("ad-year", "ad-year", "A [@s1; @s3, p. 4; @s6]. B [@z; @o; @a].\n"),
    ("ad-ranged", "ad-ranged", None),
    ("ad-ys", "ad-ys", None),
    ("ad-none", "ad-none", None),
    ("ad-links", "ad", "---\nlink-citations: true\n---\nA [@d1; @d2; @z].\n"),
    ("ad-nolinks", "ad", "---\nlink-bibliography: false\n---\nA [@d1; @d2].\n"),
    ("ad-lang", "ad", "---\nlang: de-DE\n---\nA [@o; @z; @a].\n"),
    ("ad-star", "ad", "---\nnocite: \"@*\"\n---\nA [@z].\n"),
    ("ad-nocite", "ad", "---\nnocite: \"@a, @d4\"\nreference-section-title: Works\n---\nA [@z].\n\n::: {#refs .x}\nBefore.\n:::\n\nAfter.\n"),
    ("ad-suppress", "ad", "---\nsuppress-bibliography: true\n---\nA [@z; @missing].\n"),
    ("ad-inline", "ad", "---\nreferences:\n- id: m1\n  type: book\n  title: Inline *ref*\n  author:\n  - family: Moe\n    given: Al\n  issued:\n    date-parts:\n    - [2005]\n- id: s1\n  type: book\n  title: Overridden\n  author:\n  - family: Smith\n    given: John\n---\nA [@m1; @s1].\n"),
    ("ad-locators", "ad", "A [@z, p. 33; @o, pp. 33-35 and *passim*; @a, chap. 2]. B [@z, 33, with notes]. C [@z, iv]. D [@z, {ii, A, D-Z}, rest]. E [@z, {}, 99 years]. F [@z, § 4]. G @z [foo]. H @z [(foo)]. I [see @z].\n"),
    ("ad-abbrev", "ad", "---\ncitation-abbreviations: ab.json\n---\nA [@z].\n"),
    ("num", "num", "A [@z; @o; @a]. B [@a; @s1; @s2; @s3; @d4].\n"),
    ("num-link", "num", "---\nlink-citations: true\nreference-section-title: Refs\n---\nA [@z; @o; @a].\n"),
    ("num-desc", "num-desc", "A [@z; @o; @a].\n"),
    ("note", "note", "---\nnocite: \"@a\"\n---\nOne [see @z]. Two @o. Three [@z]; four [@o, p. 3]: five [@o, p. 4]? Six [@o, p. 4]. Seven [@z].[^1] Then [ibid. @z] and [e.g., @o].\n\n[^1]: Note [@z] and @o says and [-@z].\n\n# Next {.reset-citation-positions}\n\nEight [@z]. Nine [@z].\n"),
    ("note-npa", "note", "---\nnotes-after-punctuation: false\n---\nOne [@z]. Two @o. Three [@z].\n"),
    ("suffix-var", "suffix-var", "A [@s1; @s3].\n"),
    # GPT 6.2 Sol r-b98611b-16bf516: the citations of the metadata come
    # first, in the order of the keys, without `nocite` (finding 7).
    ("meta-cite", "ad", "---\ntitle: \"T [@z]\"\nabstract: \"A [@o]\"\nzz: [\"Z [@a]\", {k: \"[@s1]\"}]\nnocite: \"@d4\"\n---\nText [@a] [@o].\n"),
    ("meta-cite-note", "note", "---\ntitle: \"T [@z].\"\nabstract: \"A [@o]\"\nnocite: \"@d4\"\n---\nText [@z]. More [@o].\n"),
    # Finding 9: a `nocite` key with no reference.
    ("nocite-missing", "ad", "---\nnocite: \"@missing, @z\"\n---\nA [@o].\n"),
    # Finding 11: each div `refs` gets the bibliography.
    ("refs-twice", "ad", "A [@z].\n\n::: {#refs}\n:::\n\nB.\n\n::: {#refs .x}\nIn.\n:::\n"),
    # Finding 10: a normal citation that opens a note is the text of the note.
    ("note-opens", "note", "A.[^1] B.[^2] C.[^3] D.[^4] E.^[[@z].] F.[^6] G.[^7] H.[^8] I.[^9] J.[^10]\n\n"
     "[^1]: [@z].\n\n[^2]: [@o]. More text.\n\n[^3]: [@a] and [@z].\n\n[^4]: @o says.\n\n"
     "[^6]: Para one.\n\n    [@o].\n\n[^7]: *[@a].*\n\n[^8]: [see @a], more.\n\n[^9]: [@z]? Yes.\n\n[^10]: [@o].x\n"),
    # Finding 10: a reset heading counts at the top level and in a top-level div.
    ("reset-div", "note", "# A\n\nA [@z].\n\n::: d\n# B {.reset-citation-positions}\n\nB [@z].\n:::\n\nC [@z].\n\n"
     "> # C {.reset-citation-positions}\n>\n> D [@z].\n\n- # D {.reset-citation-positions}\n\n  E [@z].\n\n"
     "::: a\n::: b\n# E {.reset-citation-positions}\n\nF [@z].\n:::\n:::\n\n"
     "# F {.reset-citation-positions}\n\n::: c\nG [@z].\n:::\n\nH [@z].\n\n"
     "# G {.reset-citation-positions}\n\n::: e\n# H {.reset-citation-positions}\n:::\n\nI [@z].\n\n"
     "# I {.reset-citation-positions}\n\n::: f\nfoo\n:::\n\nJ [@z].\n\n::: g\n# J {.reset-citation-positions}\n:::\n\nK [@z].\n"),
    ("dot", "dot", "Text [@z]. More [@o]? And.^[Note [@a]. x] End [@z].x\n"),
]

DEFAULT_DOC = "A [@s1; @s2; @s3]. B [@s4; @s5]. C [@d1; @d2; @d3; @d4; @s6]. D @s1 [p. 3]. E [@s3, 4]. F [@s3; @s1].\n"


def run(pandoc, args, stdin, cwd):
    result = subprocess.run([pandoc] + args, input=stdin.encode("utf-8"), capture_output=True, cwd=cwd)
    if result.returncode != 0:
        sys.exit(f"{args}: {result.stderr.decode()}")
    return result.stdout.decode("utf-8").strip()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    pandoc = sys.argv[1]
    with open("tests/replay/document-locale.xml", "w", encoding="utf-8") as f:
        f.write(LOCALE + "\n")
    with open("tests/replay/document-refs.json", "w", encoding="utf-8") as f:
        json.dump(REFS, f, ensure_ascii=False)
        f.write("\n")
    with open("tests/replay/document-abbreviations.json", "w", encoding="utf-8") as f:
        json.dump(ABBREVIATIONS, f)
        f.write("\n")
    lines = ["# Oracle results of the citation filter. Fields: ID, STYLE, INPUT (JSON), OUTPUT (JSON).",
             "# Written by tools/document_cases.py. The styles, the locale and the references are new; none is copied."]
    with tempfile.TemporaryDirectory() as tmp:
        with open(os.path.join(tmp, "refs.json"), "w", encoding="utf-8") as f:
            json.dump(REFS, f, ensure_ascii=False)
        with open(os.path.join(tmp, "ab.json"), "w", encoding="utf-8") as f:
            json.dump(ABBREVIATIONS, f)
        for case, style_name, doc in DOCS:
            text = STYLES[style_name]
            with open(os.path.join(tmp, "s.csl"), "w", encoding="utf-8") as f:
                f.write(text)
            doc = doc or DEFAULT_DOC
            if doc.startswith("---\n"):
                doc = "---\ncsl: s.csl\nbibliography: refs.json\n" + doc[4:]
            else:
                doc = "---\ncsl: s.csl\nbibliography: refs.json\n---\n" + doc
            source = run(pandoc, ["-f", "markdown", "-t", "json"], doc, tmp)
            output = run(pandoc, ["-f", "json", "-t", "json", "--citeproc"], source, tmp)
            lines.append("\t".join([case, text, source, output]))
    with open("tests/replay/document.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    print(f"tests/replay/document.txt: {len(DOCS)} cases")


if __name__ == "__main__":
    main()
