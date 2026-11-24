"""Print the MediaWiki highlight languages accepted by the Pandoc oracle."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys


def main():
    """Probe all candidates in one conversion."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pygments", action="store_true",
                        help="Add aliases from an installed copy of Pygments.")
    parser.add_argument("--verify", type=Path,
                        help="Compare the same input with this Randoc executable.")
    parser.add_argument("candidates", type=Path, nargs="*")
    args = parser.parse_args()

    oracle = os.environ["RANDOC_ORACLE_PANDOC"]
    env = {**os.environ, "GHCRTS": "-M3g"}

    def run(program, arguments, source=None):
        return subprocess.check_output(
            [program, *arguments], input=source, env=env, text=True
        )

    candidates = set(run(oracle, ["--list-highlight-languages"]).splitlines())
    current = Path(__file__).resolve().parents[1] / "src/mediawiki-highlight-languages.txt"
    candidates.update(current.read_text().split())
    candidates.update("cs js md mjs ts rs py rb sh txt yml zsh text htm ini vue svelte".split())
    for path in args.candidates:
        candidates.update(path.read_text().split())
    if args.pygments:
        from pygments.lexers import get_all_lexers

        for _, aliases, _, _ in get_all_lexers(plugins=False):
            candidates.update(aliases)
    names = sorted(name for name in candidates if re.fullmatch(r"[\w.+-]+", name))
    source = "\n".join(f"\x60\x60\x60{{.{name}}}\n<&>\n\x60\x60\x60\n" for name in names)
    ast = run(oracle, ["-f", "markdown", "-t", "json"], source)
    options = ["-f", "json", "-t", "mediawiki"]
    wiki = run(oracle, options, ast)
    if args.verify:
        actual = run(args.verify.resolve(), options, ast)
        if actual != wiki:
            raise SystemExit(f"MediaWiki output differs for the {len(names)} candidates.")
        print(f"All {len(names)} MediaWiki candidates match.", file=sys.stderr)
    print("\n".join(sorted(set(re.findall(r'<syntaxhighlight lang="([^"]+)">', wiki)))))


if __name__ == "__main__":
    main()
