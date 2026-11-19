"""Provide original command inputs for the error code probes."""

from dataclasses import dataclass
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread


CODES = (
    1, 3, 4, 5, 6, 21, 22, 23, 24, 25, 31, 43, 44, 47, 61, 62,
    63, 64, 66, 67, 83, 84, 89, 91, 92, 93, 94, 95, 97, 98, 99,
)
TEXT = b"Check the seal.\n"


@dataclass(frozen=True)
class Command:
    args: tuple[str, ...]
    stdin: bytes = b""
    files: tuple[tuple[str, bytes], ...] = ()
    executable: str | None = None
    local_temp: bool = False
    env: tuple[tuple[str, str], ...] = ()


def convert(*options, source="markdown", target="html", stdin=TEXT, files=()):
    return Command(("-f", source, "-t", target, *options), stdin, files)


def pdf(engine, script=None):
    files = ((engine, script),) if script is not None else ()
    return Command(("-f", "markdown", "-o", "check.pdf", f"--pdf-engine=./{engine}"),
                   TEXT, files, engine if files else None, local_temp=True)


def http(url):
    return Command(("-f", "html", "-t", "plain", url),
                   env=(("SSL_CERT_FILE", "/etc/ssl/cert.pem"),))


class CharsetHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/check":
            self.send_error(404)
            return
        data = b"<p>Check the seal.</p>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=seal")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, _format, *_args):
        return


@contextmanager
def charset_server():
    """Serve one fixed response on the loopback interface."""
    with ThreadingHTTPServer(("127.0.0.1", 0), CharsetHandler) as server:
        thread = Thread(target=server.serve_forever)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}/check"
        finally:
            server.shutdown()
            thread.join()


TRIGGERS = {
    1: {"missing-input": Command(("missing-seal.md",))},
    3: {"input-encoding-warning": convert("--fail-if-warnings", stdin=b"Seal:\xff")},
    4: {"docx-to-pdf": convert("-o", "check.pdf", target="docx")},
    5: {"unclosed-template": convert("--template=check.html", files=(("check.html", b"$if(seal)$Check"),))},
    6: {"invalid-columns": Command(("--columns=bad",))},
    21: {"unknown-reader": Command(("-f", "absent-reader"),)},
    22: {"unknown-writer": Command(("-t", "absent-writer"),)},
    23: {"unsupported-extension": convert(source="markdown+absent_extension")},
    24: {"invalid-style": convert("--citeproc", "--csl=check.csl", stdin=b"See [@seal].",
                                  files=(("check.csl", b"<style>"),))},
    25: {"invalid-bibliography": convert("--citeproc", "--bibliography=check.json",
                                         stdin=b"See [@seal].", files=(("check.json", b"{!"),))},
    31: {"invalid-epub-directory": convert("--epub-subdirectory=../check", target="epub")},
    43: {
        "failed-tex-engine": pdf("pdflatex", b"#!/bin/sh\nprintf '! No output.\\n'\nexit 1\n"),
        "failed-html-engine": pdf("wkhtmltopdf", b"#!/bin/sh\nprintf 'No output.\\n' >&2\nexit 1\n"),
    },
    44: {"unresolved-xml-entity": convert(source="docbook", stdin=(
        b'<!DOCTYPE article [<!ENTITY seal SYSTEM "missing.xml">]>'
        b"<article><para>&seal;</para></article>"
    ))},
    47: {"missing-pdf-engine": pdf("missing/pdflatex")},
    61: {"unavailable-http-address": http("http://127.0.0.1:0/check")},
    62: {},
    63: {"invalid-docx-container": convert(source="docx", stdin=b"Check the seal.")},
    64: {"invalid-json": convert(source="json", stdin=b"{")},
    66: {
        "missing-tex-pdf": pdf("pdflatex", b"#!/bin/sh\nexit 0\n"),
        "missing-html-pdf": pdf("wkhtmltopdf", b"#!/bin/sh\nexit 0\n"),
    },
    67: {"invalid-syntax-definition": convert("--syntax-definition=check.xml",
                                               files=(("check.xml", b"<language>"),))},
    83: {"failed-lua-filter": convert("--lua-filter=check.lua",
                                      files=(("check.lua", b"error('Cannot check the seal')"),))},
    84: {"failed-lua-script": Command(("lua", "check.lua"),
                                      files=(("check.lua", b"error('Cannot check the seal')"),), local_temp=True)},
    89: {},
    91: {"recursive-macro": convert(source="latex", stdin=br"\newcommand{\checkseal}{\checkseal} \checkseal")},
    92: {
        "invalid-utf8-template": convert("--template=check.html", files=(("check.html", b"Seal:\xff"),)),
        "invalid-utf8-metadata": convert("--metadata-file=check.yaml", files=(("check.yaml", b"seal: \xff"),)),
    },
    93: {"invalid-notebook": convert(source="ipynb", stdin=b"{")},
    94: {},
    95: {"nul-in-text": convert(stdin=b"Check\x00the seal.")},
    97: {"missing-data-file": Command(("--print-default-data-file=missing-seal",))},
    98: {"missing-metadata": convert("--metadata-file=missing-seal.yaml")},
    99: {"missing-include": convert("--include-in-header=missing-seal.html")},
}

UNCONFIRMED = {
    24: "A malformed citation style returned exit code 4.",
    62: "No command input is known that produces this internal error.",
    66: "A TeX engine that wrote no PDF returned exit code 43.",
    89: "The pinned oracle has Lua support.",
    95: "The reader accepted text with a NUL byte.",
}
