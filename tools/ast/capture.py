"""Capture a conversion with the limits of plan/SPEC.md section 3.9."""

import importlib.machinery
import importlib.util
import os
import selectors
import subprocess
import tempfile
import time
from pathlib import Path

from observe import LAUNCH, LIMIT_RUN, MAX_OUTPUT_BYTES, ROOT, NoResult, Observation, snapshot

WALL_SECONDS = 600
MEMORY_BYTES = 3000 * 1024 * 1024
CHUNK_BYTES = 65536


def limit_tool():
    loader = importlib.machinery.SourceFileLoader("ast_limits", str(LIMIT_RUN))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


class Capture:
    def __init__(self):
        self.limits = limit_tool()
        self.processes = self.limits.Processes()
        self.processes.footprint(os.getpid())
        self.processes.members(os.getpid())

    def run(self, binary, args, data, oracle, *, cwd=None, env=None, wall_seconds=WALL_SECONDS):
        try:
            program = Path(binary).resolve(strict=True)
        except OSError as error:
            raise NoResult(f"cannot read the program {binary}: {error}") from error
        with tempfile.TemporaryDirectory(prefix="randoc-ast-") as folder:
            if env is None:
                env = {
                    "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                    "HOME": str(Path(folder) / ".home"),
                    "LANG": "en_US.UTF-8",
                    "LC_ALL": "en_US.UTF-8",
                    "TZ": "UTC",
                    "SOURCE_DATE_EPOCH": "1700000000",
                    "GHCRTS": "-M3g",
                    "RANDOC_DATA_DIR": os.environ.get("RANDOC_DATA_DIR")
                    or str(Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "datapack"),
                }
            command = ["/bin/sh", "-c", LAUNCH, "ast", str(program), *args]
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, cwd=cwd or folder, env=env,
                                  start_new_session=True) as process:
                try:
                    stdout, stderr = self.exchange(process, data, wall_seconds)
                finally:
                    self.stop(process)
            code = process.returncode
            if code < 0 or 128 <= code < 192 or (oracle and code == 251):
                raise NoResult(f"{program.name} ended with exit code {code}")
            try:
                files = snapshot.snapshot(folder, set()) if cwd is None and any(Path(folder).iterdir()) else {}
            except (OSError, ValueError) as error:
                raise NoResult(f"cannot capture the files that {program.name} wrote: {error}") from error
        return Observation(code, stdout, stderr, files)

    def stop(self, process):
        try:
            members = self.processes.members(process.pid) if process.poll() is None else set()
        except OSError:
            members = set()
        self.limits.kill_all(process.pid, members)

    def exchange(self, process, data, wall_seconds):
        deadline = time.monotonic() + wall_seconds
        check_after = 0.0
        offset = 0
        buffers = {process.stdout: bytearray(), process.stderr: bytearray()}
        with selectors.DefaultSelector() as selector:
            for stream in buffers:
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ)
            if data:
                os.set_blocking(process.stdin.fileno(), False)
                selector.register(process.stdin, selectors.EVENT_WRITE)
            else:
                process.stdin.close()
            while selector.get_map() or process.poll() is None:
                now = time.monotonic()
                if now >= deadline:
                    raise NoResult("the conversion reached its wall time limit")
                if now >= check_after:
                    if process.poll() is None:
                        try:
                            members = self.processes.members(process.pid)
                            used = sum(self.processes.footprint(pid) for pid in members)
                        except OSError as error:
                            raise NoResult(f"cannot read process memory: {error}") from error
                        if used > MEMORY_BYTES:
                            raise NoResult("the conversion reached its memory limit")
                    else:
                        self.limits.kill_all(process.pid, set())
                    check_after = now + self.limits.POLL_SECONDS
                timeout = min(deadline, check_after) - now
                if not selector.get_map():
                    try:
                        process.wait(timeout=timeout)
                    except subprocess.TimeoutExpired:
                        pass
                    continue
                for key, event in selector.select(timeout):
                    stream = key.fileobj
                    if event == selectors.EVENT_WRITE:
                        try:
                            offset += os.write(key.fd, data[offset:offset + CHUNK_BYTES])
                        except BlockingIOError:
                            continue
                        except BrokenPipeError:
                            offset = len(data)
                        if offset == len(data):
                            selector.unregister(stream)
                            stream.close()
                        continue
                    try:
                        chunk = os.read(key.fd, CHUNK_BYTES)
                    except BlockingIOError:
                        continue
                    if not chunk:
                        selector.unregister(stream)
                    elif len(buffers[stream]) + len(chunk) > MAX_OUTPUT_BYTES:
                        raise NoResult("the conversion reached its output limit")
                    else:
                        buffers[stream].extend(chunk)
        return bytes(buffers[process.stdout]), bytes(buffers[process.stderr])

    def attempt(self, binary, args, unit, oracle=False):
        try:
            return self.run(binary, args, unit.data, oracle)
        except NoResult as error:
            return str(error)
