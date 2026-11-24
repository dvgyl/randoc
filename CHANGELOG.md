# Changelog

## 0.1.0 - 2026-11-25

First release of Randoc, a Rust document converter that targets Pandoc 3.13
compatibility.

- Convert Markdown, HTML, LaTeX, DOCX, EPUB, and other document formats.
- Apply Lua filters, render citations, and generate PDF with an external
  PDF engine.
- Run conversions from the command line, from Rust, or through the HTTP
  conversion server.

Some formats, options, and default data produce different results from
Pandoc 3.13. See the [README](README.md) and the
[known issues](docs/ISSUES.md) for details.
