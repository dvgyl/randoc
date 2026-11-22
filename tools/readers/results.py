"""Compare reader results and record the oracle result class."""

import json
from decimal import Decimal, DecimalException

from reference import Reference
from run import json_equal


def reject_constant(value):
    raise ValueError(f"{value} is not a JSON number")


def unique_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"repeated JSON key {key!r}")
        result[key] = value
    return result


def json_values_match(expected, actual):
    try:
        left = json.loads(expected, parse_constant=reject_constant,
                          parse_float=Decimal, object_pairs_hook=unique_keys)
        right = json.loads(actual, parse_constant=reject_constant,
                           parse_float=Decimal, object_pairs_hook=unique_keys)
        return json_equal(left, right)
    except (ValueError, DecimalException, RecursionError):
        return False


def differences(expected, actual):
    parts = []
    if expected.exit != actual.exit:
        parts.append("exit")
    if expected.stderr != actual.stderr:
        parts.append("stderr")
    if expected.files != actual.files:
        parts.append("files")
    equal_output = (json_values_match(expected.stdout, actual.stdout)
                    if expected.exit == actual.exit == 0 else expected.stdout == actual.stdout)
    if not equal_output:
        parts.append("stdout")
    return parts


def oracle_reference(first, second, saved=None):
    original = saved.first if saved is not None else None
    parts = set(saved.unstable_parts) if saved is not None else set()
    limits = []
    for number, observation in enumerate((first, second), start=1):
        if isinstance(observation, str):
            limits.append(f"oracle run {number}: {observation}")
        elif original is None:
            original = observation
        else:
            parts.update(differences(original, observation))
    if limits:
        return Reference("oracle_limit", original, unstable_parts=tuple(sorted(parts)), reason=". ".join(limits))
    if parts:
        return Reference("unstable", original, unstable_parts=tuple(sorted(parts)),
                         reason="the oracle results differ")
    return Reference("stable", original)


def verdict(identifier, fmt, expected, actual):
    row = {"id": identifier, "format": fmt, "status": expected.status}
    if expected.status == "oracle_limit":
        row["reason"] = expected.reason
        return row
    if expected.status == "unstable":
        row["unstable_parts"] = list(expected.unstable_parts)
        row["reason"] = expected.reason
    if isinstance(actual, str):
        row["reason_randoc"] = actual
        if expected.status == "stable":
            row["status"] = "no_result"
        else:
            row["unstable_parts_ok"] = False
        return row
    parts = differences(expected.first, actual)
    row["differences"] = parts
    if expected.status == "unstable":
        row["unstable_parts_ok"] = set(parts).issubset(expected.unstable_parts)
    else:
        row["status"] = "fail" if parts else "pass"
        if not parts and actual.stdout != expected.first.stdout:
            row["value_only"] = True
    return row
