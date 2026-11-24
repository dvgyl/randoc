#!/usr/bin/env python3
"""Write the image metadata replay matrix."""

import struct
import zlib
from pathlib import Path
from runpy import run_path

matrix = run_path(str(Path(__file__).with_name("wave8b_matrix.py")))
calls = matrix["calls"]
lua_string = matrix["lua_string"]


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


header = chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 1, 8, 0, 0, 0, 0))
pixels = chunk(b"IDAT", zlib.compress(bytes([0, 128, 255]))) + chunk(b"IEND", b"")
png = b"\x89PNG\r\n\x1a\n" + header + pixels
png_dpi = b"\x89PNG\r\n\x1a\n" + header + chunk(b"pHYs", struct.pack(">IIB", 11811, 3780, 1)) + pixels
gif = bytes.fromhex("47494638396101000100800000000000ffffff21f90400000000002c00000000010001000002024401003b")
formats = {
    "png": png,
    "gif": gif,
    "jpeg": bytes.fromhex("ffd8ffe000104a46494600010101007800960000ffc0000b080002000301011100ffd9"),
    "svg": b'<svg width="1in" height="2cm"/>',
    "svg_rounding": b'<svg width="1in" height="2.54cm"/>',
    "svgbox": b'<svg viewBox="0 0 10 20"/>',
    "pdf": b"%PDF-1.4\n1 0 obj\n<< /MediaBox [0 0 34 56] >>\nendobj\n",
    "eps": b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 1 2 32 46\n",
    "emf": struct.pack("<II", 1, 88) + bytes(32) + b" EMF" + bytes(44),
    "tiff": b"II*\x00" + bytes(8),
    "webp": b"RIFF\x04\x00\x00\x00WEBP",
    "bmp": b"BM" + bytes(24),
}

setup = "local I=P.image\n" + "\n".join(
    f"local {name}={lua_string(data)}" for name, data in {**formats, "pngdpi": png_dpi}.items()
)
matrix["write_matrix"]("image", {
    "formats": (setup, [
        *calls("I.format", ["", "nil", "{}", "123", "''", *formats]),
    ]),
    "sizes": (setup, [
        *calls("I.size", ["", "nil", "{}", "''", "png", "pngdpi", "gif", "jpeg", "svg", "svg_rounding", "svgbox",
                         "svg, {dpi=144}", "svg, {dpi=0}", "svg, {dpi='bad'}", "svg, false",
                         "svg, {unknown=true}", "pngdpi, {dpi=144}", "eps", "pdf", "webp"]),
    ]),
}, known=[("sizes", "I.size(svg_rounding)")])
