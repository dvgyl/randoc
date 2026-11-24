#!/usr/bin/env python3
"""Move closed issues to the closed table and sort each issue table."""

import argparse
from collections import Counter
import difflib
from pathlib import Path
import re
import sys

DEFAULT_ISSUES = Path(__file__).resolve().parents[1] / "docs" / "ISSUES.md"
OPEN = "## Open divergences"
CLOSED = "## Closed rows"
ISSUE_ID = re.compile(r"([A-Z][A-Z0-9-]*?)(\d+)")
CELL_TOKEN = re.compile(r"\\.|(?P<ticks>`+)(?!`).*?(?<!`)(?P=ticks)(?!`)|\|")


def cells(line):
    """Read table cells, including cells with escaped pipes or code spans."""
    pipes = [token.start() for token in CELL_TOKEN.finditer(line) if token[0] == "|"]
    return [line[start + 1:end].strip() for start, end in zip(pipes, pipes[1:])]


def row_key(row):
    """Split an ID into its area and numeric suffix."""
    issue_id = cells(row)[0]
    match = ISSUE_ID.fullmatch(issue_id)
    if match is None:
        raise ValueError(f"Invalid issue ID: {issue_id}")
    return match[1].rstrip("-"), int(match[2])


def tables(sections):
    """Find each ID table and its row range."""
    for section in sections:
        end = 0
        while end < len(section):
            header = cells(section[end])
            if not header or header[0] != "ID":
                end += 1
                continue
            start = end + 2
            if start > len(section) or not re.fullmatch(r"[|: -]+", section[start - 1]):
                raise ValueError("Missing Markdown table separator")
            end = start
            rows = []
            while end < len(section):
                if section[end].startswith("|"):
                    row_key(section[end])
                    rows.append(section[end])
                elif section[end].strip():
                    break
                end += 1
            # Keep the blank lines after the table.
            while end > start and not section[end - 1].strip():
                end -= 1
            yield section, start, end, rows
            end = max(end, start)


def tidy(text):
    """Return the new document and check that every original row remains."""
    sections = [[]]
    for line in text.splitlines():
        if line.startswith("## "):
            sections.append([])
        sections[-1].append(line)
    original = list(tables(sections))
    before = Counter(row for _, _, _, rows in original for row in rows)
    ids = Counter(cells(row)[0] for _, _, _, rows in original for row in rows)
    duplicates = sorted(issue_id for issue_id, count in ids.items() if count > 1)
    if duplicates:
        raise ValueError(f"Duplicate issue IDs: {', '.join(duplicates)}")

    open_tables = [table for table in original if table[0][0] == OPEN]
    closed_tables = [table for table in original if table[0][0] == CLOSED]
    if len(open_tables) != 1 or len(closed_tables) > 1:
        raise ValueError("Expected one open table and at most one closed table")
    open_section, start, _, open_rows = open_tables[0]
    header = open_section[start - 2:start]
    if cells(header[0])[-1] != "State":
        raise ValueError("The last column in the open table must be State")
    moved = {row for row in open_rows if cells(row)[-1].startswith("Closed")}
    open_rows[:] = [row for row in open_rows if row not in moved]

    if closed_tables:
        closed_section, start, _, closed_rows = closed_tables[0]
        if cells(closed_section[start - 2]) != cells(header[0]):
            raise ValueError("The open and closed tables must have the same columns")
        closed_rows.extend(moved)
    elif moved:
        if sections[-1] and sections[-1][-1]:
            sections[-1].append("")
        sections.append([CLOSED, "", *header, *sorted(moved, key=row_key)])

    for section, start, end, rows in reversed(original):
        section[start:end] = sorted(rows, key=row_key)
    after = Counter(row for _, _, _, rows in tables(sections) for row in rows)
    if after != before:
        raise ValueError("The issue rows changed during sorting")
    return "\n".join(line for section in sections for line in section) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=DEFAULT_ISSUES)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="exit with 1 if changes are necessary")
    mode.add_argument("--dry-run", action="store_true", help="show changes without writing them")
    args = parser.parse_args(argv)
    try:
        old = args.path.read_text(encoding="utf-8")
        new = tidy(old)
        if args.check:
            return int(new != old)
        if args.dry_run:
            sys.stdout.writelines(difflib.unified_diff(
                old.splitlines(keepends=True), new.splitlines(keepends=True),
                fromfile=str(args.path), tofile=f"{args.path} (sorted)",
            ))
        elif new != old:
            args.path.write_text(new, encoding="utf-8")
    except (OSError, ValueError) as error:
        print(f"issues_tidy: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
