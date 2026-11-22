"""Read a work folder through directory descriptors and hash its contents."""

import hashlib
import json
import ntpath
import os
import signal
import stat
import sys
import zipfile
from urllib.parse import quote_from_bytes

MAX_DEPTH = 128
MAX_ENTRIES = 100_000
MAX_BYTES = 512 << 20
MAX_PATH_BYTES = 16 << 20
DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
FILE_FLAGS = os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW


class ByteBudget:
    def __init__(self):
        self.remaining = MAX_BYTES

    def digest(self, stream):
        digest = hashlib.sha256()
        while data := stream.read(min(1 << 20, self.remaining + 1)):
            if len(data) > self.remaining:
                self.remaining = 0
                raise ValueError("file capture exceeds the byte limit")
            self.remaining -= len(data)
            digest.update(data)
        return digest.hexdigest()


def digest_zip(stream, budget):
    digest = hashlib.sha256(b"ZIP\0")
    with zipfile.ZipFile(stream) as archive:
        members = archive.infolist()
        if len(members) > MAX_ENTRIES:
            raise ValueError("ZIP capture exceeds the entry limit")
        first = min(members, key=lambda member: member.header_offset, default=None)
        first_stored_mimetype = (first is not None and first.filename == "mimetype"
                                 and first.compress_type == zipfile.ZIP_STORED)
        seen = set()
        for member in sorted(members, key=lambda item: item.filename):
            name = member.filename
            if (name in seen or member.orig_filename != name or not name
                    or name.startswith("/") or ntpath.splitdrive(name)[0] or "\\" in name
                    or any(part in ("", ".", "..") for part in name.rstrip("/").split("/"))):
                raise ValueError("ZIP capture contains an unsafe or duplicate path")
            seen.add(name)
            if member.is_dir():
                continue
            if member.file_size > budget.remaining:
                raise ValueError("ZIP capture exceeds the byte limit")
            with archive.open(member) as contents:
                entry_hash = budget.digest(contents)
            key = name.encode("utf-8")
            digest.update(len(key).to_bytes(8, "big"))
            digest.update(key)
            digest.update(bytes.fromhex(entry_hash))
    return {"sha256": digest.hexdigest(), "first_stored_mimetype": first_stored_mimetype}


def open_directory(path, parent_fd=None):
    fd = os.open(path, DIRECTORY_FLAGS, dir_fd=parent_fd)
    try:
        return fd, os.scandir(fd)
    except OSError:
        os.close(fd)
        raise


def snapshot(root, zip_paths):
    """Return file states by escaped relative path. Do not open link targets."""
    root_fd, root_entries = open_directory(root)
    stack = [(root_fd, b"", root_entries)]
    result = {}
    budget = ByteBudget()
    path_bytes = 0
    try:
        while stack:
            fd, parent, entries = stack[-1]
            entry = next(entries, None)
            if entry is None:
                stack.pop()
                entries.close()
                os.close(fd)
                continue
            name = os.fsencode(entry.name)
            if name.startswith(b"._"):
                continue
            if len(result) >= MAX_ENTRIES:
                raise ValueError("file capture exceeds the entry limit")
            relative = parent + name
            path = quote_from_bytes(relative, safe="/")
            path_bytes += len(path)
            if path_bytes > MAX_PATH_BYTES:
                raise ValueError("file capture exceeds the path byte limit")
            mode = os.stat(name, dir_fd=fd, follow_symlinks=False).st_mode
            if stat.S_ISDIR(mode):
                if len(stack) > MAX_DEPTH:
                    raise ValueError("file capture exceeds the directory depth limit")
                child_fd, children = open_directory(name, fd)
                stack.append((child_fd, relative + b"/", children))
                result[path] = {"kind": "directory"}
            elif stat.S_ISLNK(mode):
                link = os.readlink(name, dir_fd=fd)
                result[path] = {"kind": "link", "sha256": hashlib.sha256(link).hexdigest()}
            elif stat.S_ISREG(mode):
                with os.fdopen(os.open(name, FILE_FLAGS, dir_fd=fd), "rb") as stream:
                    info = os.fstat(stream.fileno())
                    if not stat.S_ISREG(info.st_mode) or info.st_size > budget.remaining:
                        raise ValueError("file capture needs a bounded regular file")
                    state = {"kind": "file", "sha256": budget.digest(stream)}
                    if relative in zip_paths:
                        stream.seek(0)
                        try:
                            state["zip"] = digest_zip(stream, budget)
                        except (ValueError, RuntimeError, NotImplementedError, zipfile.BadZipFile):
                            state["invalid_zip"] = True
                    result[path] = state
            else:
                raise ValueError("file capture found a special file")
    finally:
        for fd, _, entries in reversed(stack):
            entries.close()
            os.close(fd)
    return result


if __name__ == "__main__":
    signal.alarm(600)
    try:
        paths = {os.fsencode(path) for path in json.loads(sys.argv[2])}
        json.dump(snapshot(sys.argv[1], paths), sys.stdout, sort_keys=True)
    except (OSError, ValueError) as error:
        sys.exit(str(error))
