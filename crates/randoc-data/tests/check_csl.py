#!/usr/bin/env python3
"""Check the embedded CSL data and optionally replay Pandoc probes."""

import argparse
import csv
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import unicodedata
from xml.etree import ElementTree as ET

CRATE = Path(__file__).resolve().parents[1]
ROOT = CRATE.parents[1]
DATA = CRATE / "data/csl"
NS = "{http://purl.org/net/xbiblio/csl}"
XML_LANG = "{http://www.w3.org/XML/1998/namespace}lang"
FULL = {"de-DE", "en-GB", "en-US", "es-ES", "fr-FR", "it-IT", "zh-CN"}
FORMS = ("long", "short")
TERM_TYPES = frozenset("""
accessed act ad advance-online-publication album and anonymous appendix article
article-journal article-locator article-magazine article-newspaper at audio-recording
author bc bce bill book broadcast by canon ce chair chapter chapter-number
circa citation-number cited classic close-inner-quote close-quote collection
collection-editor collection-number colon column comma compiler composer
container-author contributor curator dataset director document edition editorial-director
editor editor-translator editortranslator elocation entry entry-dictionary entry-encyclopedia equation
et-al event executive-producer figure film first-reference-note-number folio
forthcoming from graphic guest hearing henceforth host ibid illustrator in internet
interview interviewer issue legal_case legislation letter line loc-cit manuscript
map motion_picture musical_score narrator no-place no-publisher note number
number-of-pages number-of-volumes on online op-cit open-inner-quote open-quote opus
organizer original-author original-work-published page page-first page-range-delimiter
pamphlet paper-conference paragraph part patent performance performer periodical
personal-communication personal_communication podcast podcast-episode post post-weblog preprint printing
producer radio-broadcast radio-series radio-series-episode recipient reference
regulation report retrieved review review-book review-of reviewed-author rule scale
scene script-writer section semicolon series-creator software song special-issue
special-section speech standard sub-verbo supplement table television-broadcast
television-series television-series-episode thesis timestamp title-locator
translator treaty verse version video volume webpage working-paper
""".split()) | {
    "and others", "available at", "in press", "no date", "presented at",
}


def rows(path):
    with path.open(encoding="utf-8", newline="") as source:
        return list(csv.DictReader(source, delimiter="\t", quoting=csv.QUOTE_NONE))


def generated_data(folder):
    """Export the Rust API output to a temporary data directory."""
    source = folder / "export.rs"
    source.write_text(
        f'#[path = {json.dumps(str(CRATE / "src/csl.rs"))}] mod csl;\n'
        """use std::{fs, io, path::Path};
fn main() -> io::Result<()> {
    let folder = Path::new("data/csl/locales");
    fs::create_dir_all(folder)?;
    fs::write("data/default.csl", csl::DEFAULT_STYLE)?;
    for tag in csl::locale_tags() {
        let xml = csl::locale_xml(tag).ok_or_else(|| io::Error::other(*tag))?;
        let labels = csl::page_labels(tag).ok_or_else(|| io::Error::other(*tag))?;
        assert_eq!(labels.len(), 4);
        fs::write(folder.join(format!("locales-{tag}.xml")), xml)?;
    }
    Ok(())
}
""", encoding="utf-8",
    )
    subprocess.run(["rustc", "--edition=2024", str(source), "-o", str(folder / "export")],
                   check=True, cwd=folder)
    subprocess.run([folder / "export"], check=True, cwd=folder)
    return folder / "data"


def check_data(data):
    terms = rows(DATA / "terms.tsv")
    locale_rows = rows(DATA / "locales.tsv")
    inventory = TERM_TYPES | {f"month-{n:02}" for n in range(1, 13)} | {
        f"season-{n:02}" for n in range(1, 5)
    } | {f"long-ordinal-{n:02}" for n in range(1, 11)} | {"ordinal"}
    for row in terms + locale_rows:
        assert all(unicodedata.is_normalized("NFC", value) for value in row.values()), row
    for row in terms:
        name = row["name"]
        assert name in inventory or re.fullmatch(r"ordinal-[0-9]{2}", name), row
    source_tags = {row["tag"] for row in locale_rows}
    files = sorted((data / "csl/locales").glob("locales-*.xml"))
    assert {path.stem.removeprefix("locales-") for path in files} == source_tags
    for path in files:
        locale = ET.parse(path).getroot()
        tag = locale.attrib[XML_LANG]
        assert tag == path.stem.removeprefix("locales-"), path
        assert locale.tag == NS + "locale" and locale.attrib["version"] == "1.0"
        available = {term.attrib["name"] for term in locale.findall(f"{NS}terms/{NS}term")}
        assert inventory <= available if tag in FULL else available == {"page"}, (
            tag, sorted(inventory - available)
        )
        dates = locale.findall(f"{NS}date")
        assert {date.attrib["form"] for date in dates} == (
            {"text", "numeric"} if tag in FULL else {"numeric"}
        ), tag
        for date in dates:
            assert {p.attrib["name"] for p in date} == {"day", "month", "year"}, tag
    path = data / "default.csl"
    style = ET.parse(path).getroot()
    assert style.tag == NS + "style"
    info = style.find(NS + "info")
    assert info.findtext(NS + "title") == "Randoc Author–Date"
    assert info.findtext(NS + "id") == "urn:randoc:style:author-date"
    rights = info.find(NS + "rights")
    assert rights.attrib == {"license": "https://spdx.org/licenses/MIT.html"}
    assert rights.text == "MIT OR Apache-2.0"
    assert "by-sa" not in path.read_text(encoding="utf-8").lower()
    print(f"Validated XML and Unicode NFC for {len(files)} locales and {len(terms)} terms.")


def run(program, args, stdin, folder):
    return subprocess.run([program, *args], input=stdin, text=True, cwd=folder,
                          capture_output=True, timeout=30)


def result_cells(html):
    """Read the citations, bibliography and layout from the HTML fragment."""
    citations = []
    bibliography = []
    layout = ""
    starts = {"span": [], "div": []}
    for match in re.finditer(r"<(/?)(span|div)\b([^>]*)>", html):
        closing, tag, attributes = match.groups()
        if not closing:
            starts[tag].append((match.end(), attributes))
            continue
        offset, attributes = starts[tag].pop()
        content = html[offset:match.start()].strip()
        if tag == "span" and 'class="citation"' in attributes:
            citations.append((offset, content))
        if tag == "div" and 'class="csl-entry"' in attributes:
            ref = re.search(r'id="ref-([^"]*)"', attributes)[1]
            bibliography.append([ref, content])
        if tag == "div" and 'id="refs"' in attributes:
            layout = attributes.strip()
    return [content for _, content in sorted(citations)], bibliography, layout


def replay(data, selected, verbose):

    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    randoc = ROOT / "target/debug/randoc"
    probes = rows(ROOT / "docs/csl/probe-log.tsv")
    if selected:
        probes = [row for row in probes if re.fullmatch(selected, row["id"])]
    probes = [row for row in probes if row["id"].startswith(("M", "S", "L"))]
    with tempfile.TemporaryDirectory(dir=data.parent) as scratch:
        folder = Path(scratch)
        styles = folder / "page-labels.csl"
        labels = [
            f'<text term="page" form="{form}" plural="{plural}"/>'
            for form in FORMS for plural in ("false", "true")
        ]
        styles.write_text(
            '<?xml version="1.0"?>\n'
            f'<style xmlns="{NS[1:-1]}" version="1.0" class="in-text">'
            '<info><title>Page probe</title><id>urn:randoc:test:page</id></info>'
            '<citation><layout delimiter=";">'
            f'<group delimiter="|">{"".join(labels)}</group>'
            '</layout></citation></style>',
            encoding="utf-8",
        )

        def case(probe):
            ident = probe["id"]
            inputs = json.loads(probe["input_json"])
            observed = json.loads(probe["observed_json"])
            args = shlex.split(probe["command"])[2:]
            if ident.startswith("L"):
                args = [arg for arg in args if not arg.startswith("--csl=")]
                args.append(f"--csl={styles}")
                expected = "|".join(term["value"] for term in observed["terms"][:4])
            else:
                expected = (observed["citations"], observed["bibliography"], observed["layout"],
                            observed["stderr"], observed.get("headings", []))
            with tempfile.TemporaryDirectory(dir=folder) as scratch_case:
                cwd = Path(scratch_case)
                if "refs.json" in inputs:
                    (cwd / "refs.json").write_text(json.dumps(inputs["refs.json"]), encoding="utf-8")
                std_input = inputs["stdin"]
                actual = []
                for program, extra in (
                    (oracle, [] if ident.startswith("L") else [f"--csl={data / 'default.csl'}"]),
                    (randoc, [f"--data-dir={data}"]),
                ):
                    result = run(program, [*args, *extra], std_input, cwd)
                    if result.returncode:
                        actual.append((result.returncode, result.stderr))
                        continue
                    cites, bib, layout = result_cells(result.stdout)
                    if ident.startswith("L"):
                        actual.append("|".join(cites))
                    else:
                        headings = re.findall(r"<h[1-6][^>]*>.*?</h[1-6]>", result.stdout)
                        actual.append((cites, bib, layout, result.stderr, headings))
            return ident, expected, actual

        with ThreadPoolExecutor(max_workers=3) as pool:
            results = list(pool.map(case, probes))
    groups = (
        ("Page locale probes", r"L-.*"),
        ("Synthetic upstream scenarios", r"S(?:0[1-9]|[1-3][0-9]|4[0-6])"),
        ("Other style probes", r"M.*|S(?:4[7-9]|5[0-8])"),
    )
    for label, pattern in groups:
        group = [item for item in results if re.fullmatch(pattern, item[0])]
        if not group:
            continue
        for i, program in enumerate(("Pandoc", "Randoc")):
            failed = [ident for ident, expected, actual in group if expected != actual[i]]
            print(f"{program} {label}: {len(group) - len(failed)}/{len(group)}")
            if failed:
                print(" ".join(failed))
    if verbose:
        for ident, expected, actual in results:
            if actual != [expected, expected]:
                print(ident, "\n expected", expected, "\n pandoc ", actual[0],
                      "\n randoc ", actual[1])


def upstream(data, verbose):
    sys.path.insert(0, str(ROOT / "tools/pandoc-testsuite"))
    from suite import code_blocks, parse_command_block

    ids = re.findall(r"^\| `([^|`]+\.md#[0-9]+)`", (
        ROOT / "docs/csl/author-date-behaviour.md"
    ).read_text(encoding="utf-8"), re.M)
    assert len(ids) == 46
    suite = Path(os.environ["RANDOC_PANDOC_SRC"]) / "test"
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    randoc = ROOT / "target/debug/randoc"

    def case(ident):
        name, number = ident.split("#")
        source = (suite / "command" / name).read_text(encoding="utf-8")
        command, stdin, _ = parse_command_block(code_blocks(source)[int(number) - 1])
        program, *args = shlex.split(command)
        # Pass simple commands as arguments. Never open external style files.
        assert program == "pandoc" and not any(
            arg.startswith(("--csl", "--citation-style")) or arg in ("|", ">", ";")
            for arg in args
        ), (ident, command)
        reference = run(oracle, args, stdin, suite)
        styled = run(oracle, [*args, f"--csl={data / 'default.csl'}"], stdin, suite)
        actual = run(randoc, [*args, f"--data-dir={data}"], stdin, suite)
        def outcome(result):
            return result.returncode, result.stdout, result.stderr

        return ident, outcome(reference), outcome(styled), outcome(actual)

    with ThreadPoolExecutor(max_workers=3) as pool:
        results = list(pool.map(case, ids))
    for index, program in enumerate(("Pandoc", "Randoc"), start=2):
        failed = [row for row in results if row[1] != row[index]]
        print(f"{program} upstream commands: {len(results) - len(failed)}/{len(results)}")
        for row in failed:
            print(row[0], row[1:] if verbose else "")


def upstream_locales(data, verbose):
    """Replay upstream commands that need a locale for their supplied style."""
    sys.path.insert(0, str(ROOT / "tools/pandoc-testsuite"))
    from suite import code_blocks, parse_command_block

    ids = set(re.findall(r"^\| `([^|`]+\.md#[0-9]+)`", (
        ROOT / "docs/csl/author-date-behaviour.md"
    ).read_text(encoding="utf-8"), re.M))
    suite = Path(os.environ["RANDOC_PANDOC_SRC"]) / "test"
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    randoc = ROOT / "target/debug/randoc"
    candidates = []
    for source in sorted((suite / "command").glob("*.md")):
        if source.name.startswith("._"):
            continue
        for number, block in enumerate(code_blocks(source.read_text(encoding="utf-8")), 1):
            ident = f"{source.name}#{number}"
            parsed = parse_command_block(block)
            if not parsed or ident in ids:
                continue
            command, stdin, _ = parsed
            if re.search(r"--citeproc\b|--csl\b|(?:^| )-C(?: |$)", command):
                program, *args = shlex.split(command)
                assert program == "pandoc" and not any(x in args for x in (";", "|", ">"))
                candidates.append((ident, args, stdin))

    empty = data.parent / "empty"
    empty.mkdir()

    def case(candidate):
        ident, args, stdin = candidate
        baseline = run(randoc, [*args, f"--data-dir={empty}"], stdin, suite)
        if "cannot find the CSL locale" not in baseline.stderr:
            return None
        expected = run(oracle, args, stdin, suite)
        actual = run(randoc, [*args, f"--data-dir={data}"], stdin, suite)
        left = expected.returncode, expected.stdout, expected.stderr
        right = actual.returncode, actual.stdout, actual.stderr
        return ident, left, right

    with ThreadPoolExecutor(max_workers=3) as pool:
        results = [entry for entry in pool.map(case, candidates) if entry is not None]
    assert len(results) == 60, f"The binary without embedded CSL data found {len(results)} cases."
    failed = [entry for entry in results if entry[1] != entry[2]]
    print(f"Randoc upstream locale commands: {len(results) - len(failed)}/{len(results)}")
    for ident, expected, actual in failed:
        print(ident, (expected, actual) if verbose else "")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--replay", action="store_true")
    parser.add_argument("--upstream", action="store_true")
    parser.add_argument(
        "--upstream-locales", action="store_true",
        help="Replay commands with a Randoc binary that requires locale files.",
    )
    parser.add_argument("--select", help="Regular expression for probe IDs")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--schema", type=Path, help="CSL 1.0.2 Relax NG schema in XML syntax")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as folder:
        data = generated_data(Path(folder))
        check_data(data)
        if args.schema:
            files = [data / "default.csl", *sorted((data / "csl/locales").glob("locales-*.xml"))]
            subprocess.run(["xmllint", "--noout", "--relaxng", args.schema, *files], check=True)
        if args.replay:
            replay(data, args.select, args.verbose)
        if args.upstream:
            upstream(data, args.verbose)
        if args.upstream_locales:
            upstream_locales(data, args.verbose)
