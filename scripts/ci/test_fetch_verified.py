#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Behavioural test of scripts/ci/fetch_verified.sh against a local server.

The failures that took `ci.yml`'s static-checks job down on 2026-11-09 and the
observability job on the same day were a body that was not the archive and an
HTTP 504. This serves each of them, and the opposite (a good file), and checks
what the script leaves behind:

  good            exit 0, DEST holds the payload
  flaky_504       two 504s then the payload: exit 0 after retrying
  error_page      HTTP 200 with an HTML body, every time: exit 1, no DEST
  page_then_good  HTML once, then the payload: exit 0, so a corrupt transfer
                  is fetched again and a good one is not refused
  always_504      exit 1, no DEST
  wrong_digest    the payload, but not the pinned digest: exit 1, no DEST, and
                  the message names both digests
  http_remote     plain HTTP to a non-loopback host: exit 2, nothing fetched
  bad_digest_arg  a digest that is not 64 lowercase hex digits: exit 2

Each case also reads how many requests the server saw, because "exit 0" alone
does not show that a retry happened, and "exit 1" alone does not show that the
script tried more than once.

Usage: scripts/ci/test_fetch_verified.py          (exit 0 all pass, 1 a failure)
"""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "fetch_verified.sh"
PAYLOAD = b"the archive the maintainer reviewed\n" * 64
PAYLOAD_SHA = hashlib.sha256(PAYLOAD).hexdigest()
HTML = b"<html><body>502 Bad Gateway</body></html>"


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self) -> None:
        super().__init__(("127.0.0.1", 0), Handler)
        self.hits: dict[str, int] = {}
        self.lock = threading.Lock()

    def count(self, path: str) -> int:
        with self.lock:
            self.hits[path] = self.hits.get(path, 0) + 1
            return self.hits[path]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args: object) -> None:  # keep the test output clean
        pass

    def reply(self, status: int, body: bytes, ctype: str = "application/octet-stream") -> None:
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802 - http.server's name
        server: Server = self.server  # type: ignore[assignment]
        n = server.count(self.path)
        if self.path == "/good":
            self.reply(200, PAYLOAD)
        elif self.path == "/flaky_504":
            self.reply(504, HTML, "text/html") if n <= 2 else self.reply(200, PAYLOAD)
        elif self.path == "/error_page":
            self.reply(200, HTML, "text/html")
        elif self.path == "/page_then_good":
            self.reply(200, HTML, "text/html") if n == 1 else self.reply(200, PAYLOAD)
        elif self.path == "/always_504":
            self.reply(504, HTML, "text/html")
        else:
            self.reply(404, b"no such path", "text/plain")


def run(args: list[str], attempts: int = 3) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ, FETCH_ATTEMPTS=str(attempts), FETCH_RETRY_DELAY="0", FETCH_CURL_RETRY_DELAY="0")
    return subprocess.run(
        ["bash", str(SCRIPT), *args], env=env, capture_output=True, text=True, timeout=120, check=False
    )


def main() -> int:
    server = Server()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = f"http://127.0.0.1:{server.server_address[1]}"
    failures: list[str] = []

    def check(name: str, ok: bool, detail: str) -> None:
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: {detail}")
        if not ok:
            failures.append(name)

    with tempfile.TemporaryDirectory(prefix="fetch-verified-test.") as tmp:
        tmp_path = Path(tmp)

        def dest(name: str) -> Path:
            return tmp_path / name / "out.bin"

        # good
        d = dest("good")
        r = run([f"{base}/good", PAYLOAD_SHA, str(d)])
        check("good", r.returncode == 0 and d.read_bytes() == PAYLOAD, f"exit {r.returncode}")

        # a short pause before the next requests is not needed: counts are per path

        # flaky_504: curl's own retry (three) absorbs two 504s inside one attempt
        d = dest("flaky")
        r = run([f"{base}/flaky_504", PAYLOAD_SHA, str(d)])
        hits = server.hits.get("/flaky_504", 0)
        check(
            "flaky_504",
            r.returncode == 0 and d.exists() and d.read_bytes() == PAYLOAD and hits == 3,
            f"exit {r.returncode}, {hits} requests (want 3: two 504s, then the file)",
        )

        # error_page: a 200 whose body is not the archive, forever
        d = dest("error_page")
        r = run([f"{base}/error_page", PAYLOAD_SHA, str(d)], attempts=3)
        hits = server.hits.get("/error_page", 0)
        check(
            "error_page",
            r.returncode == 1 and not d.exists() and hits == 3 and PAYLOAD_SHA in r.stderr,
            f"exit {r.returncode}, DEST exists={d.exists()}, {hits} requests (want 3), "
            f"names the pinned digest={PAYLOAD_SHA in r.stderr}",
        )
        leftovers = list(d.parent.glob("*.part"))
        check("error_page leaves no partial file", not leftovers, f"{[p.name for p in leftovers]}")

        # page_then_good: a corrupt first transfer is fetched again
        d = dest("page_then_good")
        r = run([f"{base}/page_then_good", PAYLOAD_SHA, str(d)])
        hits = server.hits.get("/page_then_good", 0)
        check(
            "page_then_good",
            r.returncode == 0 and d.exists() and d.read_bytes() == PAYLOAD and hits == 2,
            f"exit {r.returncode}, {hits} requests (want 2)",
        )

        # always_504
        d = dest("always_504")
        r = run([f"{base}/always_504", PAYLOAD_SHA, str(d)], attempts=2)
        check("always_504", r.returncode == 1 and not d.exists(), f"exit {r.returncode}, DEST exists={d.exists()}")

        # wrong_digest: the file is fine, the pin is not what it is
        d = dest("wrong_digest")
        other = hashlib.sha256(b"not the payload").hexdigest()
        r = run([f"{base}/good", other, str(d)], attempts=2)
        check(
            "wrong_digest",
            r.returncode == 1 and not d.exists() and other in r.stderr and PAYLOAD_SHA in r.stderr,
            f"exit {r.returncode}, DEST exists={d.exists()}, stderr names both digests="
            f"{other in r.stderr and PAYLOAD_SHA in r.stderr}",
        )

        # http_remote
        d = dest("http_remote")
        r = run(["http://example.com/x", PAYLOAD_SHA, str(d)])
        check("http_remote", r.returncode == 2 and not d.exists(), f"exit {r.returncode}")

        # bad_digest_arg
        d = dest("bad_digest")
        r = run([f"{base}/good", "ABC123", str(d)])
        check("bad_digest_arg", r.returncode == 2 and not d.exists(), f"exit {r.returncode}")

    server.shutdown()
    if failures:
        print(f"\nfetch_verified: {len(failures)} failing: {', '.join(failures)}")
        return 1
    print("\nfetch_verified: every case behaves as stated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
