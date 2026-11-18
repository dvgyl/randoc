"""Record the result of a conversion in a new work folder."""

import io
import os
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "crates/randoc-harness/src"))
import snapshot

ZIP_FORMATS = {"docx", "odt", "pptx", "epub", "epub2", "epub3", "chunkedhtml"}
MAX_OUTPUT_BYTES = 512 << 20
LIMIT_RUN = ROOT / "tools/autobuild/bin/limit-run"
LAUNCH = '''
ulimit -t 60 && ulimit -f 1048576 && PERL_BADLANG=0 exec /usr/bin/perl -e '
    my $program = shift @ARGV;
    delete @ENV{qw(PWD OLDPWD SHLVL _ PERL_BADLANG __CF_USER_TEXT_ENCODING)};
    exec {$program} "pandoc", @ARGV;
    kill "KILL", $$;
' -- "$@"
kill -KILL "$$"
'''


@dataclass(frozen=True)
class Observation:
    exit: int
    stdout: bytes
    stderr: bytes
    files: dict


class NoResult(Exception):
    """The program did not produce a complete result."""


def run(binary, args, data, *, oracle=False):
    try:
        program = Path(binary).resolve(strict=True)
    except OSError as error:
        raise NoResult(f"cannot read the program {binary}: {error}") from error
    with tempfile.TemporaryDirectory(prefix="randoc-pivot-") as folder, \
         tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        env = {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(Path(folder) / ".home"),
            "LANG": "en_US.UTF-8",
            "LC_ALL": "en_US.UTF-8",
            "TZ": "UTC",
            "SOURCE_DATE_EPOCH": "1700000000",
            "GHCRTS": "-M3g",
            "RANDOC_DATA_DIR": str(Path(os.environ.get("RANDOC_GPT", ROOT.parent)) / "datapack"),
        }
        command = [sys.executable, str(LIMIT_RUN), "3000", "600", "/bin/sh", "-c",
                   LAUNCH, "pivot", str(program), *args]
        result = subprocess.run(command, input=data, stdout=stdout, stderr=stderr, cwd=folder, env=env)
        stderr.seek(0)
        if result.returncode == 125:
            reason = stderr.read(4096).decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"the limit tool refused to start {program}: {reason}")
        if result.returncode < 0 or 128 <= result.returncode < 192 or (oracle and result.returncode == 251):
            raise NoResult(f"{program.name} ended with exit code {result.returncode}")
        stdout.seek(0)
        output = stdout.read(MAX_OUTPUT_BYTES + 1)
        error_output = stderr.read(MAX_OUTPUT_BYTES + 1)
        if max(len(output), len(error_output)) > MAX_OUTPUT_BYTES:
            raise NoResult(f"{program.name} exceeded the output limit")
        try:
            files = snapshot.snapshot(folder, set()) if any(Path(folder).iterdir()) else {}
        except (OSError, ValueError) as error:
            raise NoResult(f"cannot capture the files that {program.name} wrote: {error}") from error
    return Observation(result.returncode, output, error_output, files)


def zip_content(data):
    """Return the ZIP content under N1 and the first-entry check."""
    try:
        return snapshot.digest_zip(io.BytesIO(data), snapshot.ByteBudget())
    except (ValueError, RuntimeError, NotImplementedError, zipfile.BadZipFile):
        return None


def differences(fmt, expected, actual):
    """Return the parts that differ under the writer comparison rules."""
    parts = []
    if expected.exit != actual.exit:
        parts.append("exit")
    if expected.stderr != actual.stderr:
        parts.append("stderr")
    if expected.files != actual.files:
        parts.append("files")
    same_output = expected.stdout == actual.stdout
    if fmt in ZIP_FORMATS and expected.exit == actual.exit == 0:
        a, b = zip_content(expected.stdout), zip_content(actual.stdout)
        same_output = (a is not None and b is not None and a["sha256"] == b["sha256"]
                       and (not a["first_stored_mimetype"] or b["first_stored_mimetype"]))
    if not same_output:
        parts.append("stdout")
    return parts
