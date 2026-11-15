"""Writes tests/replay/display-{cases.txt,refs.json,locale.xml}: a layout
suffix and a last part with a display (review Opus citeproc-R3b-03).
Usage (from the crate folder): python3 tools/display_cases.py, then
pandoc lua tools/eval.lua SCRATCH_DIR display"""
import json

REFS = [{"id": "d1", "type": "book", "title": "T", "publisher": "P"},
        {"id": "d2", "type": "book", "title": "Long; title", "volume": "4"}]
DISPLAYS = ["block", "left-margin", "right-inline", "indent"]
SUFFIXES = ['suffix="."', 'suffix=". "', 'suffix=";"', 'delimiter=", " suffix="."']


def main():
    lines = []
    for r in REFS:
        for d in DISPLAYS:
            for suffix in SUFFIXES:
                for layout in [
                    f'<text value="x"/><text variable="title" display="{d}"/>',
                    f'<text variable="title" display="{d}"/>',
                    f'<group display="{d}"><text value="a"/><text variable="title"/></group>',
                    f'<text value="x"/><group><text variable="title" display="{d}" suffix=";"/></group>',
                    f'<text variable="title" display="{d}"/><text value="y"/>',
                ]:
                    lines.append(f"{r['id']}\t{suffix}\t{layout}")
    with open("tests/replay/display-cases.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    with open("tests/replay/display-refs.json", "w", encoding="utf-8") as f:
        f.write("[\n" + ",\n".join(json.dumps(r) for r in REFS) + "\n]\n")
    with open("tests/replay/display-locale.xml", "w", encoding="utf-8") as f:
        f.write('<locale><terms><term name="page-range-delimiter">\u2013</term></terms></locale>\n')


main()
