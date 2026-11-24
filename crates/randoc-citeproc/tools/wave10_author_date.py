#!/usr/bin/env python3
"""Generate the Wave 10 specification and record the synthetic Pandoc probes.

Source env/env.sh, then run python3 -B crates/randoc-citeproc/tools/wave10_author_date.py.
"""

from concurrent.futures import ThreadPoolExecutor
from datetime import date
import html
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import tempfile

DOCS = Path(__file__).resolve().parents[3] / "docs/csl"
TYPES = """book chapter article-journal article-magazine article-newspaper paper-conference
report thesis webpage post-weblog motion_picture dataset software manuscript legal_case
patent entry-encyclopedia article article-with-genre""".split()
NAMES = [{"given": g, "family": f} for g, f in [
    ("Ada J.", "Reed"), ("Bo", "Lake"), ("Cy", "Baker"), ("Di", "Moor"), ("Eli", "Ford"),
    ("Flo", "Green"), ("Gil", "Banks"), ("Hal", "Clark"), ("Ira", "Dale"),
    ("Jo", "Elm"), ("Kai", "Snow"),
]]
EDITOR = [{"given": "Eve", "family": "Holt"}]
TRANSLATOR = [{"given": "Tam", "family": "Vale"}]


def issued(*parts):
    return {"date-parts": parts}


def item(key="x", kind="book", **fields):
    return {"id": key, "type": kind, "title": "the map of a river: a field guide",
            "author": NAMES[:1], "issued": issued([2031, 9, 24]), **fields}


VARIANTS = {
    **{("1 author" if n == 1 else f"{n} authors"): {"author": NAMES[:n]} for n in (1, 2, 3, 4, 11)},
    "Editor only": {"author": None, "editor": EDITOR},
    "Translator": {"translator": TRANSLATOR},
    "Translator only": {"author": None, "translator": TRANSLATOR},
    "No author": {"author": None, "director": None},
    "No date": {"issued": None},
    "Date range": {"issued": issued([2030, 12, 29], [2031, 1, 2])},
    "Season": {"issued": {"date-parts": [[2031]], "season": 2}},
    "Status without date": {"issued": None, "status": "forthcoming"},
    "Status with date": {"status": "in press"},
    "Original date": {"original-date": issued([1913])},
    "Edition": {"edition": 3},
    "Number of volumes": {"number-of-volumes": 4},
    "Collection": {"collection-title": "Survey series", "collection-number": 2},
    "DOI": {"DOI": "10.1234/river"},
    "URL and accessed": {"URL": "https://example.org/maps", "accessed": issued([2032, 1, 4])},
    "Short container title": {"container-title-short": "Survey Q."},
}


def baseline(kind):
    common = {"publisher": "Map House", "publisher-place": "North Port"}
    serial = {"container-title": "the survey quarterly", "volume": 12,
              "issue": 3, "page": "101-129"}
    fields = {
        "book": common,
        "chapter": {**common, "container-title": "the atlas of river lands", "page": "101-129"},
        "article-journal": serial,
        "article-magazine": serial,
        "article-newspaper": {**serial, "section": "Science", "edition": "Evening"},
        "paper-conference": {**common, "container-title": "proceedings of the survey congress",
                             "event-title": "River Congress", "event-place": "East Port",
                             "page": "101-129"},
        "report": {**common, "number": "R-12", "institution": "Survey Institute"},
        "thesis": {**common, "genre": "PhD thesis"},
        "webpage": {"container-title": "River Survey Online"},
        "post-weblog": {"container-title": "River Survey Online", "genre": "Field blog"},
        "motion_picture": {**common, "director": [{"given": "Dee", "family": "Wood"}],
                           "medium": "DVD", "genre": "documentary"},
        "dataset": {**common, "version": "2.1", "number": "D-12"},
        "software": {**common, "version": "2.1", "medium": "software"},
        "manuscript": {**common, "archive": "Map Archive", "archive_location": "Box 3"},
        "legal_case": {"authority": "River Court", "container-title": "River Reports",
                       "volume": 12, "page": "101", "number": "31-5"},
        "patent": {**common, "number": "2031-12", "authority": "Patent Office"},
        "entry-encyclopedia": {**common, "container-title": "encyclopedia of rivers",
                               "volume": 12, "page": "101-129"},
        "article": {**common, **serial},
        "article-with-genre": {**common, **serial, "genre": "Preprint"},
    }
    return item(kind="article" if kind == "article-with-genre" else kind, **fields[kind])


def standalone_cases():
    a = item("a")
    b = item("b", "article-journal", author=NAMES[1:2], title="banks and streams",
             **{"container-title": "river studies", "volume": 3, "page": "125-139"})
    alone = "[@a]"
    locators = "[@a, 33]\n\n[@a, pp. 101-129]\n\n[@a, 2:33]\n\n[@a, chap. 3]\n\n[@a, sec. 3]\n\n[@a, para. 4]\n\n[@a, fig. 5]"
    cluster = "@a\n\n@a [p. 33]\n\n@a [p.33; see also @b]\n\n[see @a p. 33]\n\n[see @a, p. 33]\n\n[see @a, p. 33 and *nearby*]\n\n[@b; @a]\n\n[see @b; @a]\n\n[-@a]"
    variants = [
        ("01", "6783.md#1", "Published proceedings with no volume", [baseline("paper-conference")], "Probe[@x].", {}),
        ("02", "6873.md#1", "Missing reference and quoted suffix with brackets", [], "[@missing, „A […] B“]", {}),
        ("03", "6890.md#1", "Textual citations in body and note. Three authors, edition, title language and short title",
         [{**a, "author": NAMES[:3], "edition": 3, "language": "de-DE", "title-short": "Flusskarten",
           "title": "karten und flüsse: ein atlas", "publisher": "Map House", "publisher-place": "North Port"}],
         "@a\n\nBody.^[@a]", {}),
        ("04", "6951.md#1", "No author and no date. Mixed with an authored work",
         [a, {**b, "type": "book", "author": None, "issued": None}], "[@b; @a]", {}),
        ("05", "7042.md#1", "Cite all. Partial dates, name particles, literal East Asian names, DOI and journal fields",
         [{**b, "issued": issued([2031, 9])}, item("p", "article-journal", author=[
             {"given": "Jan", "family": "Meer", "non-dropping-particle": "van"},
             {"given": "Lea", "family": "Noor", "dropping-particle": "de"}]),
          item("e", author=[{"literal": "青木 葵"}, {"literal": "中村 雪"}]),
          item("d", "article-journal", DOI="10.1234/maps", author=[NAMES[0], {"literal": "others"}])],
         "", {"nocite": "[@*]"}),
        ("06", "7219.md#1", "Suppress author. Journal series before volume", [{**b, "collection-title": "New series"}], "-@b", {}),
        ("07", "7321.md#1", "Math in a citation suffix", [b], "[@b, $x^2+y^2=z^2$]", {}),
        ("08", "7323.md#1", "Chapter label variants", [a], "@a [chap. 6]\n\n@a [chapter 6]\n\n@a [Chap. 6]\n\n@a [Chapter 6]", {}),
        ("09", "7324.md#1", "Textual suffix with comma, leading space or neither", [a], "@a [, and others]\n\n@a [ and others]\n\n@a [and others]", {}),
        ("10", "7329.md#2", "Textual, suppress-author, prefixes, suffixes and clusters", [a, b], cluster, {}),
        ("11", "7329.md#3", "Same citation forms with a different upstream writer mode", [a, b], cluster, {}),
        ("12", "7376.md#1", "Uncited inline reference", [a], "A sentence.", {}),
        ("13", "7826.md#5", "Suppress bibliography with notes-after-punctuation", [a, b], "Probe [@b; @a], next.", {"suppress-bibliography": True, "notes-after-punctuation": True}),
        ("14", "8354.md#1", "Cite all and link a DOI. Reference heading", [{**b, "DOI": "10.1234/maps"}], "## References", {"nocite": "[@*]"}),
        ("15", "9017.md#1", "Default type in bibliography", [{k:v for k,v in a.items() if k != "type"}], "# Probe\n\n[@a].", {}),
        ("16", "9021.md#1", "Reference section title metadata", [a], "[@a].", {"reference-section-title": "Sources"}),
        ("17", "9904.md#1", "Textual and parenthetical citations with prefix and locator", [a],
         "@a [p. 17]\n\n[see @a]\n\n[see @a, p. 17]", {}),
        ("18", "10185.md#2", "Explicit references div and no date", [{**a, "issued": None}], "[@a].\n\n# Sources\n\n::: {#refs}\n:::", {}),
    ]
    for n, filename, lang in (("19", "citeproc-87.md#1", "it-IT"),):
        variants.append((n, filename, "No date and nested quotation in suffix",
                         [{**a, "issued": None, "author": [{"literal": "Survey Office"}]}],
                         '[@a 50: «Note: "inner"»].', {"lang": lang, "suppress-bibliography": True}))
    variants.extend([
        ("20", "7", "Magazine date range within a month", [item("a", "article-magazine", issued=issued([2031, 9, 24], [2031, 9, 26]))], "@a", {}),
        ("21", "14", "Book, volume with its own title, and chapter in that volume", [
            a, item("v", **{"container-title": a["title"], "volume": 1, "title": "the upper valley"}),
            item("c", "chapter", **{"container-title": a["title"], "volume": 1, "volume-title": "the upper valley",
                                     "title": "river towns", "page": "31-48", "publisher": "Map House"})],
         "[@a, 1:12]. [@v, 12]. [@c, 12].", {}),
        ("22", "25", "Subtitle case and missing journal", [item("a", "article-journal")], alone, {}),
        ("23", "38", "Three authors with repeated family name", [{**b, "author": [NAMES[0], {"given": "Cal", "family": "Reed"}, NAMES[2]]}], "@b", {}),
        ("24", "47", "Sort single author, editor substitute and coauthors. Repeat author substitution",
         [a, item("d", author=None, editor=NAMES[:1]), item("b", author=NAMES[:2]), item("c", author=[NAMES[0], NAMES[2], NAMES[3]])],
         "[@a]. [@b]. [@d]. [@c].", {}),
        ("25", "51", "Year ranges in journals", [{**b, "issued": issued([2030], [2031])}, a], "@b; @a", {}),
        ("26", "65", "Literal date, series and total volumes", [{**a, "issued": {"literal": "2030_2034"},
           "language": "de", "number-of-volumes": 5, "collection-title": "Kartenreihe",
           "collection-number": "2.5", "publisher": "Map House", "title": "karten der flüsse"}], alone, {}),
        ("27", "70", "Non-English titles, two editors, chapter series and journal page ranges",
         [item("a", "chapter", language="fr", editor=EDITOR + NAMES[1:2],
               **{"container-title": "cartes des vallées", "page": "155--174", "publisher": "Map House",
                  "collection-title": "Études du fleuve", "collection-number": 50, "title": "les cartes anciennes"}),
          {**b, "language": "la", "page": "163--193"}], "[@b; @a]", {}),
        ("28", "76", "Same author and year. Subtitle initial articles affect sort", [
            item(str(i), "article-journal", title=f"maps and rivers: {sub}")
            for i, sub in enumerate(["survey notes", "the survey notes", "a survey guide", "an atlas"])],
         "@0, @1, @2, @3", {}),
        ("29", "87", "Web and newspaper dates. Same month, different months and different years", [
            item(str(i), kind, issued=issued(*p), URL="https://example.org/w",
                 **{"container-title": "Survey Online"})
            for i, (kind, p) in enumerate([("webpage", [[2031, 9, 24]]), ("webpage", [[2031, 9, 24], [2031, 10, 25]]),
              ("webpage", [[2031, 9], [2031, 10]]), ("webpage", [[2031, 12, 31], [2032, 1, 1]]),
              ("article-newspaper", [[2031, 9, 24], [2031, 10, 25]]), ("article-newspaper", [[2031, 9], [2031, 10]])])],
         "\n\n".join(f"@{i}" for i in range(6)), {}),
        ("30", "118", "Director when there is no author", [{**baseline("motion_picture"), "author": None}], "[@x]", {}),
        ("31", "136", "Anonymous Italian title and publisher place alone",
         [{**a, "author": None, "publisher-place": "Torino", "language": "it-IT", "title": "la carta di un fiume"}], "@a", {}),
        ("32", "175", "Magazine month-only range and pages",
         [item("a", "article-magazine", issued=issued([2031, 1], [2031, 2]),
               **{"container-title": "Survey Quarterly", "page": "33-44"})], alone, {}),
        ("33", "250", "Author-in-text citation links and no date", [{**a, "issued": None}], "@a", {"link-citations": True}),
        ("34", "301", "Anonymous default-type titles with punctuation and emphasis",
         [item("a", author=None, issued=None, title="maps for N.R. Wood (on rivers)"),
          item("b", author=None, issued=None, title="<i>Trial:</i> a survey: an abridgement")], "@a; @b", {}),
        ("35", "307", "French document, adjacent citation and punctuation", [{**b, "title": "La carte du fleuve"}], "Bonjour[@b] !", {"lang": "fr-FR"}),
        ("36", "320a", "Sort names with particles, apostrophes and transliteration marks", [
            item(str(i), author=[{"family": name, "given": "Jan", "non-dropping-particle": "al-"}])
            for i, name in enumerate(["ʾNūr", "ʿNūr", "'Nūr", "‘Nūr", "’Nūr", "Nuss", "Nyman"])],
         "[" + "; ".join(f"@{i}" for i in range(7)) + "]", {}),
        ("37", "325", "Disambiguate two undated works with the same author and no title",
         [item("a", issued=None, title=None), item("b", issued=None, title=None)], "[@a; @b]", {}),
        ("38", "356", "A citation to an alternate ID produces a missing-reference warning", [{**a, "other-ids": ["alias", "second"]}], "[@alias]", {}),
        ("39", "371", "Status without a date after dated works of the same author",
         [item("a", issued=None, status="in press", title="maps of lakes"), item("b", title="maps of cities")], "[@b; @a]", {}),
        ("40", "401", "Same author and year with different original years, suppress-author cluster",
         [item("a", "chapter", **{"original-date": issued([2003])}), item("b", "chapter", **{"original-date": issued([2000])})],
         "Reed [-@a; @b] wrote.", {}),
        ("41", "408", "One author versus two with the same family name",
         [a, item("b", author=[NAMES[0], {"given": "Bea", "family": "Reed"}], title="maps of lakes")], "[@b; @a]", {}),
        ("42", "416", "Dated newspaper entries of the same author, sorting and year collapse",
         [item("a", "article-newspaper", issued=issued([2031, 12, 13])),
          item("b", "article-newspaper", issued=issued([2027, 12, 12], [2027, 12, 13])),
          item("c", "article-newspaper", issued=issued([2028]))], "[@a; @b; @c]\n\n# References {-}", {}),
        ("43", "chicago-author-date", "Missing references, linked citation modes, notes, varied locators, markup and suffixes",
         [a,b], "[@missing]\n\n@missing\n\n" + cluster + "\n\n" + locators
         + "\n\nBody.^[See @a [p. 12] and [-@b].]\n\n[@a pp. 33, 35-37, and later]\n\n[*see* @a p. **32**]",
         {"link-citations": True}),
        ("44", "no-author", "Anonymous articles in the same and different containers",
         [item(str(i), "article-magazine", author=None, title=f"map {i}",
               **{"container-title": "Survey Notes" if i < 4 else "River News"}) for i in range(6)],
         "@0 [p. 3], @1, @2, @3, @4, @5", {}),
        ("45", "number-of-volumes", "Book with total volumes and a publisher", [{**a, "number-of-volumes": 2, "publisher": "Map House", "publisher-place": "North Port"}], "@a", {}),
        ("46", "unicode-collation.md#3", "Cite all with French backward accent collation",
         [item(str(i), author=[{"literal": n}], issued=None, title=None)
          for i,n in enumerate(["cote", "côté", "coté", "côte"])], "", {"nocite": "[@*]", "lang": "fr-FR-u-kb-true"}),
        ("47", "extra", "Locator types and ranges", [a], locators, {}),
        ("48", "extra", "Year suffix, collapse, reversed cluster and same family name across years",
         [a, item("b", title="an atlas of lakes"), item("c", author=[{"family": "Reed", "given": "Bea"}], issued=issued([2032])),
          item("d", author=[{"family": "Reed", "given": "Cai"}])], "[@a; @b]\n\n[@b; @a]\n\n[@a; @c; @d]", {}),
        ("49", "extra", "Citations retain input order across authors. Bibliography sorts authors",
         [a,b], "[@a; @b]\n\n[@b; @a]", {}),
        ("50", "extra", "DOI versus URL, undated web access, and an explicit year suffix",
         [item("a", "webpage", issued=None, URL="https://example.org/maps", accessed=issued([2032,1,4])),
          item("d", "article-journal", URL="https://example.org/ignored", DOI="10.1234/river",
               **{"year-suffix": "z"})], "[@a; @d]", {}),
        ("51", "extra", "Unpublished conference paper, with and without an event date",
         [item("a", "paper-conference", **{"event-title": "River Congress", "event-place": "North Port",
                                          "event-date": issued([2032,4,4])}),
          item("b", "paper-conference", **{"event-title": "Survey Congress", "event-place": "East Port"})], "[@a; @b]", {}),
        ("52", "extra", "Short titles substitute for missing names. Long identical titles with different short forms",
         [item("a", author=None, **{"title-short": "River Map"}), item("b", "article-journal", author=None, **{"title-short": "The Atlas"})], "@a\n\n[@b]", {}),
        ("53", "extra", "Same author and year with locators. The name collapses, and both years and locators stay",
         [a, item("b", title="an atlas of lakes")], "[@a, 10; @b, 12]\n\n[@b; @a]", {}),
        ("54", "extra", "Status with issued year and separate access date without URL",
         [item("a", status="submitted"), item("b", "webpage", issued=None, accessed=issued([2032,1,4]))], "[@a; @b]", {}),
        ("55", "extra", "Repeated authors sorted by year and title. Single and double editor or translator roles",
         [item("a", issued=issued([2030]), editor=EDITOR + NAMES[2:3], translator=TRANSLATOR),
          item("b", issued=issued([2031]), author=None, editor=EDITOR + NAMES[2:3], translator=EDITOR + NAMES[2:3])], "[@a; @b]", {}),
        ("56", "extra", "Undated French reference and citation", [item("a", issued=None)], alone, {"lang": "fr-FR"}),
        ("57", "extra", "Explicit volume, event details and number of volumes in a chapter",
         [item("a", "chapter", volume=2, editor=EDITOR, **{"container-title": "river anthology", "volume-title": "the coast",
                 "number-of-volumes": 5, "collection-title": "Atlases", "collection-number": 3, "page": "101-129"})], alone, {}),
        ("58", "extra", "Two metadata records with the same ID in a reference list", [a, {**a, "title": "new title"}], alone, {}),
    ])
    for n, origin, description, refs, md, meta in variants:
        if origin not in ("extra",) and ".md" not in origin:
            origin = f"pandoc-citeproc-{origin}.md#1"
        yield f"S{n}", origin, description, refs, md, meta


def compact(data):
    return json.dumps(data, ensure_ascii=False, separators=(",", ":"))


def run(folder, args, command_input):
    r = subprocess.run([os.environ["RANDOC_ORACLE_PANDOC"], *args], input=command_input,
                       text=True, cwd=folder, capture_output=True, check=True)
    return r.stdout, r.stderr


def command(args):
    return 'GHCRTS=-M3g "$RANDOC_ORACLE_PANDOC" ' + shlex.join(args)


def md_table(text):
    return text.replace("|", "&#124;").replace("\n", "<br>")


def code_cell(value):
    return "<code>" + md_table(html.escape(value)) + "</code>"


def elements(output, tag, match_attributes):
    """Return the attributes and body of each matching balanced HTML element."""
    tokens = re.finditer(rf"<(/?){tag}\b([^>]*)>", output)
    stack = []
    found = []
    for token in tokens:
        closing, attributes = token.groups()
        if not closing:
            stack.append((token.end(), attributes))
        else:
            offset, opened = stack.pop()
            if match_attributes(opened):
                found.append((offset, opened, output[offset:token.start()].strip()))
    return [(attrs, body) for _, attrs, body in sorted(found)]


def result_cells(output):
    citations = elements(output, "span", lambda attr: 'class="citation"' in attr)
    bibliography = elements(output, "div", lambda attr: 'class="csl-entry"' in attr)
    layout = elements(output, "div", lambda attr: 'id="refs"' in attr)
    return (
        [body for _,body in citations],
        [(re.search(r'id="ref-([^"]*)"', attr)[1], body) for attr,body in bibliography],
        layout[0][0].strip() if layout else "",
    )


def cite_probe(folder, key, refs, md, meta):
    refs = [{k:v for k,v in ref.items() if v is not None} for ref in refs]
    args = ["--citeproc", "-f", "markdown", "-t", "html", "--wrap=none"]
    data = {}
    if key == "S38":
        meta = {**meta, "references": refs}
    else:
        data["refs.json"] = refs
        args.append("--bibliography=refs.json")
        (folder / "refs.json").write_text(compact(refs), encoding="utf-8")
    stdin = "---\n" + "\n".join(f"{k}: {compact(v)}" for k,v in meta.items()) + "\n---\n\n" + md if meta else md
    output, stderr = run(folder, args, stdin)
    citations, bibliography, layout = result_cells(output)
    observed = {"citations": citations, "bibliography": bibliography, "layout": layout, "stderr": stderr}
    headings = re.findall(r"<h[1-6][^>]*>.*?</h[1-6]>", output)
    if headings:
        observed["headings"] = headings
    return log(key, args, {"stdin": stdin, **data}, observed), observed


def log(key, args, data, observed):
    return "\t".join([key, str(date.today()), "3.13", command(args), compact(data), compact(observed), "observed"])


def display(obs):
    cites = "<br>".join(md_table(c) for c in obs["citations"]) or "—"
    bib = "<br>".join(f"`{key}` {md_table(value)}" for key, value in obs["bibliography"]) or "—"
    return cites, bib


LOCALES = """af-ZA ar bal-PK bg-BG brh-PK ca-AD cs-CZ cy-GB da-DK de-AT de-CH de-DE el-GR
en-GB en-US es-CL es-ES es-MX et-EE eu fa-IR fi-FI fr-CA fr-FR gl-ES he-IL hi-IN hr-HR hu-HU
id-ID is-IS it-IT ja-JP km-KH ko-KR la lij-IT lt-LT lv-LV mn-MN ms-MY nb-NO nl-NL nn-NO
pa-PK pl-PL pt-BR pt-PT ro-RO ru-RU sk-SK sl-SI sr-RS sv-SE th-TH tr-TR uk-UA vi-VN zh-CN zh-TW""".split()
KEY_LANGS = "en-US en-GB de-DE fr-FR it-IT zh-CN es-ES".split()
LEGACY_TAGS = """en en-GB x-foo de de-AT de-CH de-1996 fr fr-CA es it nl ja pt pt-PT pt-BR pl sv da nb fi cs tr ru sr zh-CN zh-TW xx""".split()
PAGE_TERMS = [("page", form, plural) for form in ("long", "short") for plural in (False,True)]
KEY_TERMS = [(name, form, False) for name,form in [
    ("and", "long"), ("et-al", "long"), ("no date", "short"), ("in press", "long"),
    ("forthcoming", "long"), ("reference", "short"), ("chapter", "short"),
    ("section", "short"), ("editor", "short"), ("translator", "short"),
    ("editortranslator", "short"), ("edition", "short"), ("volume", "short"),
    ("issue", "long"), ("accessed", "long"), ("month-06", "short"), ("month-09", "short"),
    ("season-02", "long"), ("ad", "long"), ("open-quote", "long"),
    ("close-quote", "long"), ("open-inner-quote", "long"), ("close-inner-quote", "long"),
]] + [("volume", "short", True), ("editor", "short", True), ("translator", "short", True)]


def page_probe(folder, lang):
    rows = []
    terms = PAGE_TERMS + (KEY_TERMS if lang in KEY_LANGS else [])
    parts = []
    for i,(term, form, plural) in enumerate(terms):
        parts.append(f'<group prefix="[{i}=" suffix="]" delimiter=""><text variable="title"/><text term="{term}" form="{form}" plural="{str(plural).lower()}"/></group>')
    style = ('<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">'
             '<citation><layout>' + "".join(parts) + '</layout></citation></style>')
    (folder / "probe.csl").write_text(style, encoding="utf-8")
    refs = [{"id": "x", "type": "book", "title": "T"}]
    (folder / "refs.json").write_text(compact(refs), encoding="utf-8")
    args = ["--citeproc", "-f", "markdown", "-t", "html", "--wrap=none",
            "--bibliography=refs.json", "--csl=probe.csl", f"-Mlang={lang}"]
    out, err = run(folder, args, "[@x]")
    citations = result_cells(out)[0]
    values = {int(i): html.unescape(v) for i,v in re.findall(r"\[(\d+)=T([^]]*)\]", "".join(citations))}
    if len(values) != len(terms):
        raise ValueError((lang, out))
    results = [{"term": term, "form": form, "plural": plural, "value": values[i]}
               for i,(term,form,plural) in enumerate(terms)]
    rows.append(log(f"L-{lang}", args, {"stdin": "[@x]", "refs.json": refs,
                                        "synthetic_style_terms": terms}, {"terms": results, "stderr": err}))
    candidates = [label + " 33" for label in dict.fromkeys(values[i] for i in range(4)) if label.strip()]
    rows.append(biblatex_probe(folder, f"P-{lang}", lang, candidates))
    return rows


def biblatex_probe(folder, key, lang, candidates):
    args = ["-f", "markdown", "-t", "latex", "--biblatex", "--wrap=none"]
    if lang:
        args.append(f"-Mlang={lang}")
    stdin = "\n\n".join(f"[@x, {candidate}]" for candidate in candidates)
    out, err = run(folder, args, stdin)
    citations = [line for line in out.splitlines() if "\\autocite" in line]
    if len(citations) != len(candidates):
        raise ValueError((lang, out))
    return log(key, args, {"stdin": stdin}, {"results": [
        {"suffix": candidate, "cut": line == "\\autocite[33]{x}", "latex": line}
        for candidate,line in zip(candidates,citations,strict=True)
    ], "stderr": err})


def main():
    version = subprocess.check_output([os.environ["RANDOC_ORACLE_PANDOC"], "--version"], text=True)
    if version.splitlines()[0] != "pandoc 3.13" or os.environ.get("GHCRTS") != "-M3g":
        raise SystemExit("Source env/env.sh and use the Pandoc 3.13 oracle.")
    DOCS.mkdir(parents=True, exist_ok=True)

    def matrix(task):
        kind, name, fields, key = task
        with tempfile.TemporaryDirectory() as tmp:
            row, obs = cite_probe(Path(tmp), key, [{**baseline(kind), **fields}], "[@x]", {})
        return row, f"| {name} | " + " | ".join(display(obs)) + f" | `{key}` |"

    def scenario(task):
        key, origin, description, refs, md, meta = task
        with tempfile.TemporaryDirectory() as tmp:
            row, obs = cite_probe(Path(tmp), key, refs, md, meta)
        citation_input = "<br>" + code_cell(md) if md else ""
        return (row, f"| `{origin}` | {description}{citation_input} | "
                + " | ".join(display(obs)) + f" | `{key}` |")

    def locale(lang):
        with tempfile.TemporaryDirectory() as tmp:
            return page_probe(Path(tmp), lang)

    def legacy(lang):
        candidates = ["p. 33", "pp. 33", "page 33", "S. 33", "Seite 33", "s. 33",
                      "P. 33", "PAGE 33", "p.33", "pp 33"]
        with tempfile.TemporaryDirectory() as tmp:
            return biblatex_probe(Path(tmp), f"P-9275-{lang or 'unset'}", lang, candidates)

    results = []
    spec = [INTRO, "", "### Changes to the baseline", "",
            "A null value removes a field. The listed author count gives the number of names",
            "in the baseline order.", "",
            "| Input change | Fields |", "| --- | --- |"]
    spec.extend(f"| {name} | {code_cell(compact(fields))} |"
                for name,fields in VARIANTS.items() if not name.endswith(("author", "authors")))
    spec.extend(["", "The author order is " + code_cell(compact(NAMES)) + ".", "",
                 "## Type and field matrix"])
    langs = [*KEY_LANGS, *(lang for lang in LOCALES if lang not in KEY_LANGS)]
    header = "| Input change | Citation | Bibliography entry | Source |\n| --- | --- | --- | --- |"
    with ThreadPoolExecutor(max_workers=4) as pool:
        for rows in pool.map(locale, langs):
            results.extend(rows)
        results.extend(pool.map(legacy, ["", *LEGACY_TAGS]))
        tasks = [(kind, name, fields, f"M{t:02d}.{v:02d}")
                 for t, (kind) in enumerate(TYPES, 1)
                 for v, (name, fields) in enumerate(VARIANTS.items(), 1)]
        for i, (row, table) in enumerate(pool.map(matrix,tasks)):
            if i % len(VARIANTS) == 0:
                kind = tasks[i][0]
                other = {k:v for k,v in baseline(kind).items()
                         if k not in ("id", "type", "title", "author", "issued")}
                spec.extend(["", f"### `{kind}`", "",
                             "Additional baseline fields " + code_cell(compact(other)) + ".",
                             "", header])
            results.append(row)
            spec.append(table)
        spec.extend(["", "## Citation clusters and default-style scenarios", "", SCENARIOS,
                     "", "| Scenario | Synthetic input | Citation output in document order | Bibliography in sort order | Source |",
                     "| --- | --- | --- | --- | --- |"])
        for row, table in pool.map(scenario, standalone_cases()):
            results.append(row)
            spec.append(table)
    spec.extend(["", "## Exact scenario references", "",
                 "Each block gives the Citation Style Language (CSL) JSON references and",
                 "document metadata. Use the citation text in the scenario table.", "",
                 "<details><summary>Show scenario inputs</summary>", ""])
    for key, _, _, refs, _, meta in standalone_cases():
        data = {"references": [{k:v for k,v in ref.items() if v is not None} for ref in refs],
                "metadata": meta}
        spec.extend([f"### {key}", "", "```json", compact(data), "```", ""])
    spec.append("</details>")
    (DOCS / "author-date-behaviour.md").write_text("\n".join(spec) + "\n", encoding="utf-8")
    (DOCS / "probe-log.tsv").write_text(
        "id\tdate\toracle_version\tcommand\tinput_json\tobserved_json\tsource\n"
        + "\n".join(results) + "\n", encoding="utf-8")


INTRO = """# Pandoc 3.13 default author-date behaviour

The tables give observed output from `--citeproc` with no style argument.
All references and citation text are synthetic. Source IDs identify rows in
[the probe log](probe-log.tsv). Each log row gives the command, exact inputs,
observed citation content, bibliography order, layout attributes and standard error
where applicable. Run the commands with the environment from `env/env.sh`.

Italics and links show the formatting Pandoc emitted in HTML. Each citation cell
shows the body of the citation element. A dash means that the result has no
citation or no bibliography entry. Each bibliography cell gives the reference
ID and the contents of its entry. Line breaks in a cell separate results.

## How to use the matrix

Each row is an independent document with one reference and one citation,
`[@x]`. The first row of each table gives the baseline. Other rows change
only the fields in their input description. The source ID gives the full
Citation Style Language (CSL) JSON record. The type headings give the additional
baseline fields, including fields that Pandoc ignored for that type.

The base title is `the map of a river: a field guide`. The first author is
Ada J. Reed. The full author lists are in the log. The base publication date
is 24 September 2031. `article-with-genre` is type `article` with
`genre: Preprint`.

The tables record output for field combinations with one reference. Use the
scenario tables for effects that need more than one reference, such as sorting
and year suffixes. Apply output punctuation and links to the complete group
of fields shown in each row.

## Rules observed across probes

| Area | Observed behaviour | Source |
| --- | --- | --- |
| Citation names | Print one family name or join two family names with “and”. With three or more authors, print the first family name and “et al.”. For a book with no author, try the editor, then the translator, then the title. | `M01.01`–`M01.09`, `S52` |
| Year and status | Insert a space between the name and a numeric year. Insert a comma before the undated term or a status that replaces the year. Put the original year in brackets in a citation and in parentheses in the bibliography. | `M01.10`–`M01.15`, `S39`, `S40` |
| Locators | Insert a comma before the locator. Print page numbers without a label. Use the short term for non-page labels, such as “chap.”. Shorten page ranges such as 101–129 to 101–29. | `S08`, `S47` |
| Clusters | Keep citation order. Separate different authors with a semicolon and space. Group matching consecutive authors. Keep each year and locator when locators are present. | `S48`, `S49`, `S53` |
| Disambiguation | Assign suffix letters by bibliography order. Keep those letters when the citation order changes. Include initials when same-family names need disambiguation. Add a hyphen before the suffix on an undated term. | `S28`, `S37`, `S48`, `S53` |
| Bibliography names | Invert the first personal name. Use given-name-first order for the other names. Print all four names in a four-author book. Print three names and “et al.” in an eleven-author book. Show the editor or translator role if that name takes the place of the author. | `M01.04`–`M01.08` |
| Titles | Use title case for English titles in these probes. Italicize book titles. Put chapter and journal article titles in quotation marks. Preserve case in the tested German and French titles. | `M01.01`, `M02.01`, `M03.01`, `S03`, `S26`, `S27` |
| Publication | When both fields are present, print the publisher for the tested book and chapter. An anonymous book with only a publisher place prints that place. A date after a serial title depends on the type and the issue or volume fields. | `M01.01`, `M02.01`, `M03.01`, `M05.01`, `S31`, `S49` |
| Links and access | Prefer a DOI link when both DOI and URL are present. A dated web page with a URL omits the access date. An undated web page with a URL prints “Accessed” before the date. | `M09.20`, `S50` |
| Bibliography layout | The references list has a hanging indent. The output has no explicit entry-spacing or line-spacing attribute. A references title or a supplied references div can come from the document. | `M01.01`, `S16`, `S18` |

## Matrix inputs"""

SCENARIOS = """The first 46 rows use new reference data for the 46 upstream command scenarios
listed in the Wave 10 blueprint. The scenario ID identifies the upstream case.
The probes use Markdown input and HTML output to make the style result visible
in a common form. For cases whose original input or output uses another format,
the row covers the citation or bibliography feature at the citation filter
boundary. Additional rows test interactions used by the default style.

The log also records selected rendered locale terms under `L-<tag>`. The
`P-<tag>` rows test the four observed page term forms as candidate labels in
`--biblatex` output. The `P-9275-<tag>` rows test the same candidate matrix
across language tags, including missing and invalid tags. A cut means the
result has `33` as the full citation postnote. The term probes use a new
minimal style that renders only the terms listed in the input."""

if __name__ == "__main__":
    main()
