"""The corpus manifest: reading, checking and writing it."""

import dataclasses
import hashlib
import re
import tomllib
from pathlib import Path

MAX_DOCUMENT_BYTES = 2_000_000
MAX_TOTAL_BYTES = 300_000_000
USER_AGENT = "randoc-corpus/1.0 (document corpus fetcher; Python urllib)"
MANIFEST = Path(__file__).with_name("manifest.toml")

# The file extension of a corpus document for each Pandoc input format.
FORMAT_EXTENSIONS = {
    "asciidoc": "adoc",
    "biblatex": "bib",
    "bibtex": "bib",
    "commonmark": "md",
    "commonmark_x": "md",
    "creole": "creole",
    "csljson": "json",
    "csv": "csv",
    "djot": "dj",
    "docbook": "xml",
    "docx": "docx",
    "dokuwiki": "txt",
    "epub": "epub",
    "fb2": "fb2",
    "gfm": "md",
    "html": "html",
    "ipynb": "ipynb",
    "jats": "xml",
    "latex": "tex",
    "man": "1",
    "markdown": "md",
    "mdoc": "1",
    "mediawiki": "wiki",
    "muse": "muse",
    "odt": "odt",
    "opml": "opml",
    "org": "org",
    "pod": "pod",
    "pptx": "pptx",
    "ris": "ris",
    "rst": "rst",
    "rtf": "rtf",
    "t2t": "t2t",
    "textile": "textile",
    "tsv": "tsv",
    "typst": "typ",
    "xlsx": "xlsx",
}

NAME_PATTERN = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
FIELDS = ("name", "format", "url", "license", "source", "sha256", "size")
# An optional field: the time limit of one conversion of this document in seconds.
OPTIONAL_FIELDS = ("timeout",)
TIMEOUT_RULE = "the timeout must be a whole number of seconds greater than 0"


class ManifestError(Exception):
    """The manifest has a defect."""


@dataclasses.dataclass(frozen=True)
class Document:
    """One corpus document as the manifest lists it."""

    name: str
    format: str
    url: str
    license: str
    source: str
    sha256: str
    size: int
    timeout: int = 0

    @property
    def file_name(self):
        """The name of the document file in the corpus directory."""
        return f"{self.name}.{FORMAT_EXTENSIONS[self.format]}"


def sha256_of(data):
    """Returns the SHA-256 hash of `data` as hexadecimal text."""
    return hashlib.sha256(data).hexdigest()


def parse_manifest(text, require_hashes=True):
    """Parses the manifest text and returns the list of documents.

    With `require_hashes=False`, a document can have an empty `sha256` and a
    `size` of 0. `fetch.py --pin` fills these fields.
    """
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise ManifestError(f"manifest: {error}") from error
    documents = []
    for index, entry in enumerate(data.get("document", [])):
        where = f"document {index + 1}"
        missing = [field for field in FIELDS if field not in entry]
        if missing:
            raise ManifestError(f"{where}: missing {', '.join(missing)}")
        extra = sorted(set(entry) - set(FIELDS) - set(OPTIONAL_FIELDS))
        if extra:
            raise ManifestError(f"{where}: unknown field {', '.join(extra)}")
        if "timeout" in entry and (type(entry["timeout"]) is not int or entry["timeout"] <= 0):
            raise ManifestError(f"{where}: {TIMEOUT_RULE}")
        known = [field for field in FIELDS + OPTIONAL_FIELDS if field in entry]
        documents.append(Document(**{field: entry[field] for field in known}))
    check(documents, require_hashes)
    return documents


def check(documents, require_hashes=True):
    """Raises `ManifestError` for the first rule that `documents` break."""
    names = set()
    total = 0
    for document in documents:
        where = f"document '{document.name}'"
        if not NAME_PATTERN.match(document.name):
            raise ManifestError(f"{where}: the name must be lowercase words joined by '-'")
        if document.name in names:
            raise ManifestError(f"{where}: the name is not unique")
        names.add(document.name)
        if document.format not in FORMAT_EXTENSIONS:
            raise ManifestError(f"{where}: unknown format '{document.format}'")
        if not document.url.startswith("https://"):
            raise ManifestError(f"{where}: the URL must use https")
        if not document.license.strip() or not document.source.strip():
            raise ManifestError(f"{where}: the license and the source must not be empty")
        pinned = bool(SHA256_PATTERN.match(document.sha256)) and document.size > 0
        if not pinned and (require_hashes or document.sha256 or document.size):
            raise ManifestError(f"{where}: sha256 and size must both be set")
        # A timeout of 0 means that the entry has no timeout field.
        if type(document.timeout) is not int or document.timeout < 0:
            raise ManifestError(f"{where}: {TIMEOUT_RULE}")
        if document.size > MAX_DOCUMENT_BYTES:
            raise ManifestError(f"{where}: {document.size} bytes is more than {MAX_DOCUMENT_BYTES}")
        total += document.size
    if total > MAX_TOTAL_BYTES:
        raise ManifestError(f"the corpus has {total} bytes, more than {MAX_TOTAL_BYTES}")


def load_manifest(path=MANIFEST, require_hashes=True):
    """Reads and parses the manifest file at `path`."""
    return parse_manifest(Path(path).read_text(encoding="utf-8"), require_hashes)


def _toml_string(value):
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def format_manifest(documents, header=""):
    """Returns the manifest text for `documents`. `parse_manifest` reads it back."""
    lines = [line.rstrip() for line in header.splitlines()]
    for document in documents:
        lines.append("")
        lines.append("[[document]]")
        for field in FIELDS + OPTIONAL_FIELDS:
            value = getattr(document, field)
            if field in OPTIONAL_FIELDS and not value:
                continue
            rendered = str(value) if isinstance(value, int) else _toml_string(value)
            lines.append(f"{field} = {rendered}")
    return "\n".join(lines).lstrip("\n") + "\n"


def source_table(documents):
    """Returns the Markdown table of the sources: licences, formats and document counts."""
    groups = {}
    for document in documents:
        source = document.source.split("@")[0]
        licenses, formats, count = groups.get(source, (set(), set(), 0))
        groups[source] = (licenses | {document.license}, formats | {document.format}, count + 1)
    lines = ["| Source | Licence | Formats | Documents |", "| --- | --- | --- | --- |"]
    for source, (licenses, formats, count) in sorted(groups.items(), key=lambda g: g[0].lower()):
        lines.append(f"| {source} | {'; '.join(sorted(licenses))} | {', '.join(sorted(formats))} | {count} |")
    return "\n".join(lines)


def count_table(documents):
    """Returns the Markdown table of the document count and size for each format."""
    counts = {}
    for document in documents:
        number, size = counts.get(document.format, (0, 0))
        counts[document.format] = (number + 1, size + document.size)
    lines = ["| Format | Documents | Bytes |", "| --- | --- | --- |"]
    for name, (number, size) in sorted(counts.items()):
        lines.append(f"| {name} | {number} | {size} |")
    total = sum(document.size for document in documents)
    lines.append(f"| all | {len(documents)} | {total} |")
    return "\n".join(lines)


def manifest_header(text):
    """Returns the comment lines at the start of a manifest text."""
    header = []
    for line in text.splitlines():
        if not line.startswith("#"):
            break
        header.append(line)
    return "\n".join(header)


if __name__ == "__main__":
    # Prints the two tables of docs/CORPUS.md.
    corpus_documents = load_manifest()
    print(source_table(corpus_documents))
    print()
    print(count_table(corpus_documents))
