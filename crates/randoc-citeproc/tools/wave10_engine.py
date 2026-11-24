#!/usr/bin/env python3
"""Record synthetic Wave 10 citation filter replays with Pandoc 3.13.

Source env/env.sh, then run python3 -B crates/randoc-citeproc/tools/wave10_engine.py.
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile

from document_cases import style

CRATE = Path(__file__).resolve().parents[1]
REFS = [
    {"id": "a", "type": "book", "title": "river maps", "author": [{"family": "Reed"}]},
    {"id": "q", "type": "book", "title": "river maps."},
    {"id": "j", "type": "book", "title": "other maps"},
    {"id": "l", "type": "book", "URL": "https://example.org/maps."},
    {"id": "u", "type": "book", "URL": "https://example.org/maps"},
]
TITLE = '<text variable="title"/>'
NAMES = '<names variable="author"><name/></names>'
BIB = '<bibliography><layout>' + TITLE + '</layout></bibliography>'
RIS = ("TY  - BOOK\nID  - r\nTI  - RIS record\nER  - \n"
       "TY  - BOOK\nID  - s\nTI  - Second RIS record\nER  - \n")
BIBLATEX = "@book{c, author={Alex Reed}, title={Survey of {ACM} maps}, year={2031}}"


def note(content, suffix="", bib=""):
    return style("note", f'<citation><layout suffix="{suffix}">{content}</layout></citation>', bib)


def cases():
    yield ("C01-softbreak", note(NAMES + TITLE, "."), """A [@a].
B

C [@a],
D

E [@a];
F

G [@a]
H

"Title" [@a].
I

J [@a]!Next
K
""", [], {})
    yield ("C03-author-in-text", note(f'<group delimiter=", ">{NAMES}{TITLE}</group>', "."),
           "A @a,\nthen [@j].\n\nB @a.\nNext\n\nC @a; then.\n"
           "\nD @a [chap. 3], [@j].\n\nE @q, next.\n\nF @a?! Next.",
           [], {})
    yield ("C03-move-disabled", note(NAMES + TITLE, "."),
           "A @a,\nthen [@j].", [], {"notes-after-punctuation": False})

    for name, content, suffix, key in [
        ("sole-period", '<text value="."/>', "", "a"),
        ("text", TITLE, ".", "a"),
        ("after-emphasis", '<text variable="title" font-style="italic"/>', ".", "a"),
        ("inside-emphasis", '<text variable="title" font-style="italic" suffix="."/>', "", "q"),
        ("after-quote", '<text variable="title" quotes="true"/>', ".", "a"),
        ("inside-quote", '<text variable="title" quotes="true"/>', "", "q"),
        ("after-link", '<text variable="URL"/>', ".", "u"),
        ("inside-link", '<text variable="URL"/>', "", "l"),
        ("inside-span", '<text variable="title" display="block" suffix="."/>', "", "q"),
        ("only-period-in-span", '<text value="." display="block"/>', "", "a"),
    ]:
        markdown = f"Text.^[See [@{key}].]\n\nText.^[[@{key}].]\n\nText.^[See @{key}.]"
        yield f"C02-{name}", note(content, suffix), markdown, [], {}

    for name, before in [
        ("same-after-period", "Earlier [@j]. "),
        ("same-after-comma", "Earlier [@j], "),
        ("same-no-punctuation", "Earlier [@j] "),
        ("next-paragraph", "Earlier [@j].\n\n"),
        ("adjacent-note", "Earlier [@j]."),
        ("earlier-text-no-cite", "Earlier. "),
        ("author-in-text-punctuation", "Earlier @j. "),
    ]:
        yield (f"C02-{name}", note('<text variable="title" font-style="italic"/>', "."),
               before + "Text.^[See [@a].]", [], {})

    for name, md, meta in [
        ("earlier-note", "Text.^[See [@a].] Later [@j].", {}),
        ("earlier-note-no-punctuation", "Text.^[See [@a].] Later [@j]", {}),
        ("earlier-note-separated", "Text.^[See [@a].]\n\nLater [@j].", {}),
        ("disabled", "Earlier [@j]. Text.^[See [@a].]", {"notes-after-punctuation": False}),
        ("before-nested-format", "Earlier [@j]. *Text*.^[See [@a].]", {}),
    ]:
        yield f"C02-{name}", note('<text variable="title" quotes="true"/>', "."), md, [], meta

    for name, md in [
        ("two-notes", "A [@a], [@j]."),
        ("two-textual", "A @a, @a."),
        ("punctuation-before-second-note", "A @a, [@j]"),
        ("no-punctuation-between", "A @a [@j]."),
        ("separate-words", "A @a, then [@j]."),
        ("adjacent", "A [@a],[@j]."),
        ("softbreak-between", "A [@a],\n[@j]."),
        ("third-note", "A [@a], [@j], [@a]."),
    ]:
        yield f"C03-{name}", note(f'<group delimiter=", ">{NAMES}{TITLE}</group>', "."), md, [], {}

    for name, extra in [("alone", ""), ("right", '<text value="End" display="right-inline"/>'),
                        ("plain", '<text value="End"/>')]:
        bib = '<bibliography><layout><text value="Start" display="left-margin"/>' + extra + '</layout></bibliography>'
        yield f"C04-{name}", style("in-text", '<citation><layout>' + TITLE + '</layout></citation>', bib), "[@a]", [], {}

    st = style("in-text", '<citation><layout><text variable="citation-number"/></layout></citation>', BIB)
    for name, md, files, meta in [
        ("alone", "A sentence.", ["recs.ris"], {}),
        ("mixed", "[@j]", ["recs.ris", "other.json"], {}),
        ("reverse", "[@j]", ["other.json", "recs.ris"], {}),
        ("explicit-nocite", "[@j]", ["recs.ris", "other.json"], {"nocite": "[@j]"}),
        ("explicit-empty", "[@j]", ["recs.ris", "other.json"], {"nocite": ""}),
        ("cited-ris", "[@s]", ["recs.ris", "other.json"], {}),
        ("suppress-bibliography", "[@s]", ["recs.ris"], {"suppress-bibliography": True}),
    ]:
        yield f"C05-{name}", st, md, files, meta

    yield ("C06-biblatex", style("in-text", '<citation><layout>' + TITLE + '</layout></citation>',
                                '<bibliography><layout><text variable="title" text-case="title"/></layout></bibliography>'),
           "[@c]", ["protected.bib"], {})
    yield ("C06-csljson", style("in-text", '<citation><layout>' + TITLE + '</layout></citation>',
                               '<bibliography><layout><text variable="title" text-case="title"/></layout></bibliography>'),
           "[@p]", ["protected.json"], {})


def record(folder):
    def run(args, data=""):
        result = subprocess.run([os.environ["RANDOC_ORACLE_PANDOC"], *args],
                                input=data, text=True, cwd=folder, capture_output=True, check=True)
        return json.loads(result.stdout)

    def compact(value):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))

    external = {"recs.ris": ("ris", RIS), "protected.bib": ("biblatex", BIBLATEX)}
    files = {
        "refs.json": compact(REFS),
        "other.json": compact([{"id": "unused", "title": "Unused JSON record"}]),
        "protected.json": compact([{"id": "p", "title": 'm<span class="nocase">ACM</span> maps'}]),
        **{name: contents for name, (_, contents) in external.items()},
    }
    for name, contents in files.items():
        (folder / name).write_text(contents, encoding="utf-8")
    external_meta = {name: run(["-f", fmt, "-t", "json", "-s", name])["meta"]["references"]
                     for name, (fmt, _) in external.items()}
    rows = ["# ID, synthetic style, input document, host files, reader metadata, oracle document."]
    for name, generated_style, md, filenames, meta in cases():
        (folder / "s.csl").write_text(generated_style, encoding="utf-8")
        refs = ["refs.json", *filenames]
        md = "---\n" + "\n".join([
            "csl: s.csl", "bibliography: " + compact(refs),
            *(f"{key}: {compact(value)}" for key, value in meta.items()),
        ]) + "\n---\n\n" + md
        src = run(["-f", "markdown", "-t", "json"], md)
        expected = run(["-f", "json", "-t", "json", "--citeproc"], compact(src))
        rows.append("\t".join([name, generated_style, compact(src),
                               compact({f: {"t": "MetaString", "c": files[f]} for f in refs}),
                               compact({f: external_meta[f] for f in refs if f in external_meta}),
                               compact(expected)]))
    return "\n".join(rows) + "\n"


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as tmp:
        replay = record(Path(tmp))
    (CRATE / "tests/replay/wave10-engine.txt").write_text(replay, encoding="utf-8")
