"""Run the server probes with the section 3.9 limits."""

import contextlib
import errno
import hashlib
import http.client
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

WALL_SECONDS = 600
MAX_BODY = 512 << 20
MAX_CGI_HEADERS = 64 << 10
MAX_CGI_PROCESSES = 3
ROOT = Path(__file__).resolve().parents[2]
LIMIT_RUN = ROOT / "tools/measure/bin/limit-run"
LIMITS = 'ulimit -t 60 && ulimit -f 1048576'
EXEC_NAMED = r"""
delete @ENV{qw(PWD OLDPWD SHLVL _ PERL_BADLANG __CF_USER_TEXT_ENCODING)};
my $program = shift @ARGV;
exec {$program} @ARGV;
die "$program: $!";
"""


class NoResult(Exception):
    """The invocation gave no complete result."""


def environment(work):
    return {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": str(Path(work) / ".home"),
        "LANG": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "TZ": "UTC",
        "SOURCE_DATE_EPOCH": "1700000000",
        "GHCRTS": "-M3g",
        "RANDOC_DATA_DIR": os.environ.get("RANDOC_DATA_DIR")
        or str(Path(os.environ.get("RANDOC_ENV", ROOT.parent / "env")) / "datapack"),
    }


def limited(*args, setup="exec"):
    return [sys.executable, str(LIMIT_RUN), "3000", str(WALL_SECONDS),
            "/bin/sh", "-c", f'{LIMITS} && {setup} "$@"', "sh", *args]


def named(program, name, *args):
    return limited("/usr/bin/perl", "-e", EXEC_NAMED, "--", str(program), name, *args,
                   setup="PERL_BADLANG=0 exec")


def hash_body(stream):
    digest = hashlib.sha256()
    size = 0
    while block := stream.read(64 << 10):
        size += len(block)
        if size > MAX_BODY:
            raise NoResult("response exceeds the size limit")
        digest.update(block)
    return {"sha256": digest.hexdigest(), "size": size}


def http_response(client, request):
    headers = {"Accept": request.accept, "Content-Type": "application/json"}
    client.request(request.method, request.path, request.content, headers)
    with client.getresponse() as response:
        body = hash_body(response)
        if response.length:
            raise NoResult("incomplete HTTP response body")
        return {"status": response.status, "content_type": response.getheader("Content-Type"), "body": body}


def local_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_for_listener(process, port):
    deadline = time.monotonic() + WALL_SECONDS
    while process.poll() is None and time.monotonic() < deadline:
        # Check the socket before the first request. An HTTP check would
        # add a second request to the version test.
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                if process.poll() is None:
                    return
        except (ConnectionRefusedError, TimeoutError):
            time.sleep(0.05)
        except OSError as error:
            if error.errno != errno.EADDRNOTAVAIL:
                raise
            time.sleep(0.05)
    raise NoResult(f"server did not start (exit {process.poll()})")


def stop(process):
    if process.poll() is None:
        # limit-run sends the signal to its own child group.
        process.terminate()
    process.wait()


def missing(reason):
    return {"no_result": str(reason)}


def http_run(program, requests):
    port = local_port()
    args = ["server", "--port", str(port)]
    if requests[0].timeout is not None:
        args.extend(("--timeout", str(requests[0].timeout)))
    results = {}
    with tempfile.TemporaryDirectory(prefix="server-http-") as work, tempfile.TemporaryFile() as stderr:
        with subprocess.Popen(named(program, "pandoc", *args), cwd=work, env=environment(work),
                              stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                              stderr=stderr) as process:
            try:
                wait_for_listener(process, port)
                client = http.client.HTTPConnection("127.0.0.1", port, timeout=WALL_SECONDS)
                with contextlib.closing(client):
                    for request in requests:
                        try:
                            results[request.id] = http_response(client, request)
                        except (OSError, http.client.HTTPException, NoResult) as error:
                            client.close()
                            results[request.id] = missing(error)
            except NoResult as error:
                if process.poll() == 125:
                    stderr.seek(0)
                    raise RuntimeError(stderr.read(4096).decode(errors="replace")) from error
                return {request.id: missing(error) for request in requests}
            finally:
                stop(process)
    return results


def cgi_response(stream):
    """Read the CGI status and content type, then hash the body bytes."""
    headers = {}
    remaining = MAX_CGI_HEADERS
    while remaining > 0:
        line = stream.readline(remaining + 1)
        remaining -= len(line)
        if line in (b"\n", b"\r\n"):
            break
        name, colon, value = line.partition(b":")
        if not colon or not line.endswith(b"\n") or remaining < 0:
            raise NoResult("invalid CGI headers")
        key = name.lower()
        if key in (b"status", b"content-type") and key in headers:
            raise NoResult("repeated CGI response header")
        headers[key] = value.strip()
    else:
        raise NoResult("CGI headers exceed the size limit")
    try:
        status = int(headers.get(b"status", b"200").split(maxsplit=1)[0])
        content_type = headers[b"content-type"].decode("latin-1")
    except (KeyError, ValueError, IndexError) as error:
        raise NoResult("missing or invalid CGI status or content type") from error
    if not 100 <= status <= 599:
        raise NoResult("invalid CGI response status")
    return {"status": status, "content_type": content_type, "body": hash_body(stream)}


def cgi_environment(work, request, body):
    env = environment(work)
    env.update({
        "REQUEST_METHOD": request["method"],
        "PATH_INFO": request["path"],
        "SCRIPT_NAME": "/pandoc-server.cgi",
        "QUERY_STRING": "",
        "CONTENT_TYPE": "application/json",
        "CONTENT_LENGTH": str(len(body)),
        "HTTP_ACCEPT": request["accept"],
        "REMOTE_ADDR": "127.0.0.1",
        "SERVER_NAME": "localhost",
        "SERVER_PORT": "80",
        "SERVER_PROTOCOL": "HTTP/1.1",
        "GATEWAY_INTERFACE": "CGI/1.1",
    })
    if request["timeout"] is not None:
        env["PANDOC_SERVER_TIMEOUT"] = str(request["timeout"])
    return env


def cgi_one(program, request):
    body = request["body"].encode()
    try:
        with tempfile.TemporaryDirectory(prefix="server-cgi-") as work, tempfile.TemporaryFile() as out:
            # The batch launcher sets the CPU and file size limits. Each
            # child inherits both limits and has a fresh work folder.
            done = subprocess.run(["pandoc-server.cgi"], executable=program, input=body,
                                  stdout=out, stderr=subprocess.DEVNULL, cwd=work,
                                  env=cgi_environment(work, request, body), timeout=WALL_SECONDS)
            if done.returncode:
                return missing(f"CGI exit {done.returncode}")
            out.seek(0)
            return cgi_response(out)
    except (NoResult, OSError, subprocess.TimeoutExpired) as error:
        return missing(error)


def cgi_worker(program, workers):
    """Process one batch under the process-group memory and wall limits."""
    requests = [json.loads(line) for line in sys.stdin.buffer]

    def run(request):
        return [request["id"], cgi_one(program, request)]

    with ThreadPoolExecutor(max_workers=workers) as pool:
        for result in pool.map(run, requests):
            print(json.dumps(result), flush=True)


def cgi_run(program, requests, *, workers=MAX_CGI_PROCESSES):
    with tempfile.TemporaryFile() as inputs, tempfile.TemporaryFile() as outputs:
        for request in requests:
            values = {"id": request.id, "path": request.path, "method": request.method,
                      "accept": request.accept, "timeout": request.timeout,
                      "body": request.content.decode()}
            inputs.write(json.dumps(values, ensure_ascii=False).encode() + b"\n")
        inputs.seek(0)
        command = limited(sys.executable, __file__, str(program), str(workers))
        result = subprocess.run(command, stdin=inputs, stdout=outputs,
                                stderr=subprocess.PIPE)
        outputs.seek(0)
        results = dict(json.loads(line) for line in outputs if line.endswith(b"\n"))
    if result.returncode == 125:
        raise RuntimeError(result.stderr.decode(errors="replace").strip())
    if result.returncode and len(results) == len(requests):
        raise RuntimeError(f"CGI batch ended with exit {result.returncode} after writing all results")
    for request in requests:
        if request.id not in results:
            results[request.id] = missing(f"CGI batch exit {result.returncode}")
    return results


def option_run(program, option):
    try:
        with tempfile.TemporaryDirectory(prefix="server-cli-") as work, \
             tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            env = environment(work)
            if option == "--version":
                # The version output includes this user-supplied path.
                env["XDG_DATA_HOME"] = "/var/empty"
            done = subprocess.run(named(program, "pandoc", "server", option), cwd=work, env=env,
                                  stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
            if done.returncode == 125:
                stderr.seek(0)
                raise RuntimeError(stderr.read(4096).decode(errors="replace"))
            if done.returncode < 0 or 128 <= done.returncode < 192:
                return missing(f"option exit {done.returncode}")
            stdout.seek(0)
            stderr.seek(0)
            if done.returncode == 251 and b"Heap exhausted" in stderr.read(4096):
                return missing("heap exhausted")
            stderr.seek(0)
            return {"exit": done.returncode, "stdout": hash_body(stdout), "stderr": hash_body(stderr)}
    except (OSError, NoResult) as error:
        return missing(error)


if __name__ == "__main__":
    cgi_worker(sys.argv[1], int(sys.argv[2]))
