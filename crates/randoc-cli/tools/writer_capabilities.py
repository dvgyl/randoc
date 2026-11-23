#!/usr/bin/env python3
"""Records standalone output for each registered writer.

Usage: writer_capabilities.py PANDOC OUT.json

Each writer runs with no option, -s, --template, and both options. Each
command reads a titled document and an empty document. The default
templates of these writers are present in randoc-data. For OpenDocument,
record the document body without the default styles. For EPUB, record the
first chapter in the archive. Each oracle process has memory and time limits.
"""

import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from zipfile import ZipFile


EPUB_FORMATS = {"epub", "epub2", "epub3"}
FORMATS = sorted([
    "beamer", "commonmark", "commonmark_x", "csljson", "dokuwiki", "fodt", "gfm",
    "html", "html4", "html5", "jira", "json", "latex", "markdown", "markdown_github",
    "markdown_mmd", "markdown_phpextra", "markdown_strict", "markua", "mediawiki",
    "native", "odt", "opendocument", "org", "plain", "rst", "rtf",
    "biblatex", "bibtex", "docbook", "docbook4", "docbook5", "fb2", "jats",
    "jats_archiving", "jats_articleauthoring", "jats_publishing", "tei",
] + list(EPUB_FORMATS))
ODF_FORMATS = {"fodt", "odt", "opendocument"}
REQUESTS = [
    ("none", []),
    ("standalone", ["-s"]),
    ("template", ["--template=t"]),
    ("both", ["-s", "--template=t"]),
]
DOCUMENTS = [
    ("titled", {
        "pandoc-api-version": [1, 23, 1, 2],
        "meta": {"title": {"t": "MetaInlines", "c": [{"t": "Str", "c": "Title"}]}},
        "blocks": [{"t": "Para", "c": [{"t": "Str", "c": "Body"}]}],
    }),
    ("empty", {"pandoc-api-version": [1, 23, 1, 2], "meta": {}, "blocks": []}),
]
TEMPLATE = "marker:$title$|$body$"
ODF_TEMPLATE = (
    '<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"'
    ' xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3">'
    '<office:body><office:text><text:p>$title$</text:p>$body$'
    '</office:text></office:body></office:document>'
)


def document_body(writer, data):
    if writer == "odt":
        with ZipFile(io.BytesIO(data)) as archive:
            data = archive.read("content.xml")
    if writer in EPUB_FORMATS:
        with ZipFile(io.BytesIO(data)) as archive:
            data = archive.read("EPUB/text/ch001.xhtml")
    text = data.decode("utf-8")
    if writer in ODF_FORMATS and "<office:body>" in text:
        return text.split("<office:body>", 1)[1].split("</office:body>", 1)[0]
    return text


def main():
    oracle, destination = sys.argv[1:]
    with tempfile.TemporaryDirectory(prefix="randoc-writers-") as folder:
        records = record(oracle, Path(folder))
    Path(destination).write_text(json.dumps({
        "template": TEMPLATE, "odf_template": ODF_TEMPLATE, "records": records,
    }, indent=1) + "\n", encoding="utf-8")
    print(f"Recorded {len(records)} commands for {len(FORMATS)} writers.", flush=True)


def record(oracle, root):
    (root / "t").write_text(TEMPLATE, encoding="utf-8")
    (root / "t-odf").write_text(ODF_TEMPLATE, encoding="utf-8")
    home = root / "home"
    home.mkdir(exist_ok=True)
    environment = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(home / "share"),
                       LC_ALL="C", LANG="C")
    records = []
    for writer in FORMATS:
        for request, options in REQUESTS:
            if writer in ODF_FORMATS:
                options = ["--template=t-odf" if arg == "--template=t" else arg for arg in options]
            args = ["-f", "json", "-t", writer] + options
            for name, document in DOCUMENTS:
                stdin = json.dumps(document, separators=(",", ":"))
                result = subprocess.run(
                    ["limit-run", "1500", "60", oracle] + args,
                    input=stdin.encode(), cwd=root, env=environment,
                    capture_output=True, timeout=65, check=False,
                )
                if result.returncode != 0:
                    sys.exit(f"oracle did not complete {args}: {result.stderr!r}")
                records.append({
                    "writer": writer, "request": request, "document": name,
                    "args": args, "stdin": stdin,
                    "exit": result.returncode,
                    "stdout": document_body(writer, result.stdout),
                    "stderr": result.stderr.decode("utf-8"),
                })
        print(f"Recorded {writer}: {len(records)} commands.", flush=True)
    return records


if __name__ == "__main__":
    main()
