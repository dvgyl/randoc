#!/usr/bin/env python3
"""Downloads the corpus documents to $RANDOC_CORPUS and checks each hash.

The program sends one request at a time, so no server gets parallel requests.
It skips a document when its file exists with the correct hash.

With --pin, the program also fills an empty sha256 and size in the manifest
from the downloaded data. A document that cannot be downloaded stays
unpinned, and the program reports it.
"""

import argparse
import dataclasses
import http.client
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import corpuslib

DELAY_SECONDS = 0.5


class HttpsOnlyRedirects(urllib.request.HTTPRedirectHandler):
    """Follows a redirect only to an https URL."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != "https":
            raise urllib.error.HTTPError(newurl, code, f"redirect to a non-https URL: {newurl}", headers, fp)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


OPENER = urllib.request.build_opener(HttpsOnlyRedirects)


DOWNLOAD_SECONDS = 300


def download(url, timeout=10, total=DOWNLOAD_SECONDS):
    """Returns the bytes at `url`.

    `timeout` limits each network operation and `total` limits the whole
    download, so that a slow server cannot hold the program. Each read
    returns the bytes that are available (`read1`), so the program checks the
    total limit after each wait of at most `timeout`. Raises `ValueError` for
    a document that is too large and `TimeoutError` at the total limit.
    """
    deadline = time.monotonic() + total
    request = urllib.request.Request(url, headers={"User-Agent": corpuslib.USER_AGENT})
    data = bytearray()
    with OPENER.open(request, timeout=max(0.001, min(timeout, total))) as response:
        if time.monotonic() > deadline:
            raise TimeoutError(f"no complete download after {total} s")
        while chunk := response.read1(65536):
            data += chunk
            if len(data) > corpuslib.MAX_DOCUMENT_BYTES:
                raise ValueError(f"more than {corpuslib.MAX_DOCUMENT_BYTES} bytes")
            if time.monotonic() > deadline:
                raise TimeoutError(f"no complete download after {total} s")
    return bytes(data)


def write_atomic(path, data):
    """Writes `data` to `path` through a temporary file, so that a partial file never remains."""
    temporary = path.with_name(path.name + ".part")
    temporary.write_bytes(data)
    os.replace(temporary, path)


def fetch_one(document, corpus, get=download, pin=False, budget=corpuslib.MAX_TOTAL_BYTES):
    """Makes sure that the corpus holds `document`.

    Returns a pair: the status text and the document, with the hash and the
    size filled when `pin` is true and the manifest had no hash. `budget` is
    the number of bytes that a newly pinned document can add to the corpus.
    """
    path = corpus / document.file_name
    unpinned = not document.sha256
    if not unpinned and path.exists():
        if corpuslib.sha256_of(path.read_bytes()) == document.sha256:
            return "present", document
    data = get(document.url)
    digest = corpuslib.sha256_of(data)
    if unpinned:
        if not pin:
            return "unpinned: run with --pin", document
        if len(data) > budget:
            return f"over the total limit: {len(data)} bytes, {budget} left", document
        document = dataclasses.replace(document, sha256=digest, size=len(data))
    elif digest != document.sha256 or len(data) != document.size:
        # An old file with other content must not stay, or a run could use it.
        path.unlink(missing_ok=True)
        if digest != document.sha256:
            return f"hash mismatch: got {digest}", document
        return f"size mismatch: got {len(data)} bytes", document
    write_atomic(path, data)
    return "downloaded", document


def main(argv=None, get=download, delay=DELAY_SECONDS):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", type=Path, default=corpuslib.MANIFEST)
    parser.add_argument("--corpus", type=Path, default=os.environ.get("RANDOC_CORPUS"),
                        help="the corpus directory (default: $RANDOC_CORPUS)")
    parser.add_argument("--pin", action="store_true",
                        help="fill an empty sha256 and size in the manifest")
    parser.add_argument("--only", action="append", default=[],
                        help="fetch only the documents whose name contains this text")
    args = parser.parse_args(argv)
    if args.corpus is None:
        parser.error("set RANDOC_CORPUS or use --corpus")

    text = args.manifest.read_text(encoding="utf-8")
    try:
        documents = corpuslib.parse_manifest(text, require_hashes=not args.pin)
    except corpuslib.ManifestError as error:
        print(error, file=sys.stderr)
        return 2
    args.corpus.mkdir(parents=True, exist_ok=True)

    failures = 0
    budget = corpuslib.MAX_TOTAL_BYTES - sum(document.size for document in documents)
    last_request = {}
    result = []
    for document in documents:
        if args.only and not any(part in document.name for part in args.only):
            result.append(document)
            continue
        host = urllib.parse.urlsplit(document.url).hostname
        wait = last_request.get(host, 0.0) + delay - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        try:
            unpinned = not document.sha256
            status, document = fetch_one(document, args.corpus, get, args.pin, budget)
            if unpinned and document.sha256:
                budget -= document.size
        except (OSError, ValueError, http.client.HTTPException) as error:
            status = f"error: {error}"
        if status != "present":
            last_request[host] = time.monotonic()
        if status not in ("present", "downloaded"):
            failures += 1
        print(f"{document.name}: {status}")
        result.append(document)

    if args.pin:
        try:
            corpuslib.check(result, require_hashes=False)
        except corpuslib.ManifestError as error:
            print(f"the manifest is not written: {error}", file=sys.stderr)
            return 1
        pinned = corpuslib.format_manifest(result, corpuslib.manifest_header(text))
        args.manifest.write_text(pinned, encoding="utf-8")
    print(f"{len(documents)} documents, {failures} failures")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
