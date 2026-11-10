"""Makes conformance/cases/templates/language.toml from tests/oracle.json.

Run from the repository root: python3 crates/randoc-templates/tests/make_conformance.py
Each oracle case becomes a command-line case that renders main.txt with the
plain writer and fixed -V values.
"""
import json

SOURCE = "crates/randoc-templates/tests/oracle.json"
TARGET = "conformance/cases/templates/language.toml"
VARIABLES = [
    "x=hello", "y=world", "e=", "list=a", "list=b", "list=c",
    "ml=line1\nline2", "nl=ends\n", "num=7", "big=1994", "t",
    "cases=ΣΑΣ ß ﬃ İ ǆ Ǆ ΌΣΟΣ",
]


def toml(value):
    return json.dumps(value, ensure_ascii=False)


def main():
    data = json.load(open(SOURCE, encoding="utf-8"))
    lines = [
        "# Template language cases: each case renders main.txt with the plain writer.",
        "# Made by crates/randoc-templates/tests/make_conformance.py from tests/oracle.json.",
        "",
    ]
    for case in data["cases"]:
        args = ["-f", "markdown", "-t", "plain", "--template=main.txt"]
        for variable in VARIABLES:
            args += ["-V", variable]
        if case.get("width"):
            args.append("--columns=%d" % case["width"])
        files = dict(data["partials"])
        files["main.txt"] = case["template"]
        lines += [
            "[[case]]",
            "id = %s" % toml(case["id"]),
            "cmd = [%s]" % ", ".join(toml(a) for a in args),
            'input = ""',
            "files = { %s }" % ", ".join(
                "%s = %s" % (toml(k), toml(v)) for k, v in sorted(files.items())
            ),
            "",
        ]
    open(TARGET, "w", encoding="utf-8").write("\n".join(lines))


main()
