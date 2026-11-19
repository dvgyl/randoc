"""Collect the inputs for each reader format."""

import hashlib
import json
import os
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / "tools/pivot"), str(ROOT / "tools/corpus")]

import corpuslib
from fetch import download
from reference import cache_key, read_reference
from writers import Unit as WriterUnit
from writers import references


@dataclass(frozen=True)
class Input:
    id: str
    data: bytes


def reader_formats(root):
    return json.loads((root / "spec/inventory/input-formats.json").read_bytes())


def harness_inputs(root, formats):
    """Keep one case input per distinct byte sequence in each format."""
    known = {fmt: {} for fmt in formats}
    fixtures = root / "conformance/fixtures"
    for path in sorted((root / "conformance/cases").rglob("*.toml")):
        if path.name.startswith("._"):
            continue
        case_file = tomllib.loads(path.read_text(encoding="utf-8"))
        for case in case_file.get("case", []):
            fmt = case.get("from", case_file.get("from"))
            if "cmd" in case or fmt is None:
                continue
            fmt = fmt.split("+", 1)[0].split("-", 1)[0]
            if fmt not in known:
                continue
            data = case["input"].encode("utf-8") if "input" in case else b""
            if "input_file" in case:
                name = Path(case["input_file"])
                if name.is_absolute() or ".." in name.parts or not name.parts or "input" in case:
                    raise ValueError(f"{path}: invalid input_file in case {case['id']}")
                data = (fixtures / name).read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            known[fmt][digest] = Input(f"reader/{fmt}/harness/{digest}", data)
    return {fmt: list(inputs.values()) for fmt, inputs in known.items()}


def document_data(document, folder, *, fetch=False):
    path = folder / document.file_name
    if path.is_file():
        with path.open("rb") as stream:
            data = stream.read(document.size + 1)
    elif fetch:
        data = download(document.url)
    else:
        raise FileNotFoundError(f"missing corpus document {path}")
    if len(data) != document.size or corpuslib.sha256_of(data) != document.sha256:
        raise ValueError(f"{path}: the document differs from its pinned size or SHA-256")
    if fetch and not path.is_file():
        folder.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=folder, prefix=".download-", delete=False) as stream:
            temporary = Path(stream.name)
            try:
                stream.write(data)
                stream.close()
                os.replace(temporary, path)
            finally:
                temporary.unlink(missing_ok=True)
    return data


def document_inputs(documents, folder, *, fetch=False):
    for document in documents:
        unit_id = f"reader/{document.format}/corpus/{document.name}"
        yield Input(unit_id, document_data(document, folder, fetch=fetch))


def pivot_inputs(fmt, trees, database, version, oracle, pool, rule, selected):
    """Read the original standard output of each successful oracle write."""
    command = ["-f", "json", "-t", fmt]
    units = []
    for identifier, data in trees:
        unit_id = f"reader/{fmt}/pivot/{identifier}"
        if selected is None or selected in unit_id:
            units.append(WriterUnit(unit_id, data, cache_key(version, command, data)))
    if not units:
        return
    references(database, units, oracle, command, pool, rule)
    for unit in units:
        reference = read_reference(database, unit.key)
        if reference is None:
            raise ValueError(f"{unit.name}: the oracle writer result is missing")
        if reference.first is not None and reference.first.exit == 0:
            yield Input(unit.name, reference.first.stdout)
