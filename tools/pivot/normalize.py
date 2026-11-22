"""Normalize only the notebook cell identifiers that change between oracle runs."""

import json
import re
import tomllib

from generate import ROOT


RULE_ID = "pivot-ipynb-cell-id"
SCOPE = {"metric": "fmt.write.ipynb", "part": "stdout", "field": "cells[*].id"}


def notebook_rule():
    file = ROOT / "spec/normalizations.toml"
    rows = tomllib.loads(file.read_text(encoding="utf-8"))["normalization"]
    matches = [row for row in rows if row["id"] == RULE_ID]
    if len(matches) != 1 or matches[0]["kind"] != "N5" or matches[0]["scope"] != SCOPE:
        raise ValueError(f"{file} needs one N5 rule {RULE_ID} with the notebook cell scope")
    return re.compile(matches[0]["pattern"]), matches[0]["replacement"]


def notebook_ids(data):
    try:
        book = json.loads(data)
    except (ValueError, RecursionError):
        return None
    if not isinstance(book, dict) or not isinstance(book.get("cells"), list):
        return None
    cells = book["cells"]
    if not all(isinstance(cell, dict) for cell in cells):
        return None
    return [cell.get("id") for cell in cells]


def is_uuid(value, pattern):
    return isinstance(value, str) and pattern.fullmatch(value) is not None


def notebook_positions(first, second, rule):
    a, b = notebook_ids(first), notebook_ids(second)
    if a is None or b is None or len(a) != len(b):
        return ()
    pattern, _ = rule
    return tuple(index for index, (old, new) in enumerate(zip(a, b))
                 if old != new and is_uuid(old, pattern) and is_uuid(new, pattern))


def notebook_output(data, positions, rule):
    if not positions:
        return data
    identifiers = notebook_ids(data)
    if identifiers is None or max(positions) >= len(identifiers):
        return None
    pattern, replacement = rule
    changes = {}
    for number, index in enumerate(positions, start=1):
        candidate = identifiers[index]
        if not is_uuid(candidate, pattern):
            return None
        key = candidate.encode("ascii")
        if key in changes:
            return None
        changes[key] = replacement.replace("n", str(number)).encode("ascii")

    quoted_uuid = re.compile(rb'"(' + pattern.pattern.encode("ascii") + rb')"')
    seen = set()
    duplicates = set()

    def replace(match):
        value = match[1]
        if value not in changes:
            return match[0]
        if value in seen:
            duplicates.add(value)
        seen.add(value)
        return b'"' + changes[value] + b'"'

    result = quoted_uuid.sub(replace, data)
    return result if not duplicates and seen == set(changes) else None
