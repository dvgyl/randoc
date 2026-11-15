#!/usr/bin/env python3
"""Record the HTML closer matrix with the Pandoc oracle.

Each case combines a Markdown HTML container, the three HTML extensions,
an inline token, a closer position, balanced delimiters and nesting.
The record command runs bounded batches through limit-run.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
SCRATCH = Path(os.environ["TMPDIR"]) / "markdown-reader-s6"

# These names are the non-void, non-verbatim entries of block::BLOCK_TAGS.
CONTAINERS = """address applet article aside audio blockquote body button canvas
caption center colgroup dd del details dir div dl dt fieldset figcaption figure
footer form frameset h1 h2 h3 h4 h5 h6 head header hgroup html iframe ins isindex li
main map menu nav noframes noscript object ol output p progress section summary svg
table tbody td tfoot th thead title tr ul video span""".split()

# One container of each kind that the reader treats in its own way: a native
# div, a native span, a block that interrupts a paragraph, one that does not,
# a paragraph, a list item, a table and SVG.
DEFAULT = "div span section ins p li table svg"


def cases(containers):
    result = []
    for name in containers:
        closer = f"</{name}>"
        for markdown in [False, True]:
            for divs in [False, True]:
                for spans in [False, True]:
                    fmt = "markdown+tex_math_single_backslash+tex_math_double_backslash"
                    for ext, enabled in [("markdown_in_html_blocks", markdown),
                                         ("native_divs", divs), ("native_spans", spans)]:
                        fmt += ("+" if enabled else "-") + ext
                    flags = f"{int(markdown)}{int(divs)}{int(spans)}"
                    for position, gap in [("same", " "), ("later", "\n"), ("blank", "\n\n")]:
                        payload = f"a{gap}{closer}{gap}b"
                        forms = {
                            "code1": ("`" + payload, "`"),
                            "code2": ("``" + payload, "``"),
                            "code3": ("q ```" + payload, "```"),
                            "code3-at-line-start": ("```" + payload, "```"),
                            "dollar": ("$" + payload, "$"),
                            "double-dollar": ("$$" + payload, "$$"),
                            "backslash-paren": ("\\(" + payload, "\\)"),
                            "backslash-bracket": ("\\[" + payload, "\\]"),
                            "raw-inline": ("`" + payload, "`{=html}"),
                            "comment": ("<!--" + payload, "-->"),
                            "quoted-html": ('<i title="' + payload, '">q</i>'),
                            "destination": ("[q](" + payload, ")"),
                            "title": ('[q](u "' + payload, '")'),
                            "quoted-markdown": ('[q]{title="' + payload, '"}'),
                        }
                        for token, (start, end) in forms.items():
                            for balanced in [False, True]:
                                for nesting in [1, 2]:
                                    text = (f"<{name}>\n" * nesting + start
                                            + (end if balanced else "") + "\n"
                                            + f"</{name}>\n" * nesting)
                                    ident = f"hc-{name}-{flags}-{token}-{position}-{int(balanced)}-{nesting}"
                                    result.append({"id": ident, "from": fmt, "input": text})
    return result


def record(containers, output, batch_size):
    SCRATCH.mkdir(parents=True, exist_ok=True)
    work = cases(containers)
    records = []
    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    for offset in range(0, len(work), batch_size):
        batch = work[offset:offset + batch_size]
        inputs = SCRATCH / "html-closer-cases.json"
        results = SCRATCH / "html-closer-oracle.json"
        inputs.write_text(json.dumps(batch, ensure_ascii=False), encoding="utf-8")
        subprocess.run(["limit-run", "1500", "60", oracle, "lua",
                        str(HERE / "oracle.lua"), str(inputs), str(results)], check=True)
        docs = json.loads(results.read_text(encoding="utf-8"))
        records.extend({**case, "doc": docs[case["id"]]} for case in batch)
        print(f"recorded {len(records)}/{len(work)}", flush=True)
    target = Path(output)
    target.write_text("[\n" + ",\n".join(json.dumps(row, ensure_ascii=False)
                                              for row in records) + "\n]\n", encoding="utf-8")
    print(f"saved {len(records)} cases to {target}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output")
    parser.add_argument("--containers", default=DEFAULT.replace(" ", ","),
                        help="a comma-separated list, or all")
    parser.add_argument("--batch-size", type=int, default=1000)
    args = parser.parse_args()
    names = CONTAINERS if args.containers == "all" else args.containers.split(",")
    if not names or any(name not in CONTAINERS for name in names):
        parser.error("unknown container")
    if not 1 <= args.batch_size <= 2000:
        parser.error("batch size must be from 1 to 2000")
    record(names, args.output, args.batch_size)


if __name__ == "__main__":
    main()
