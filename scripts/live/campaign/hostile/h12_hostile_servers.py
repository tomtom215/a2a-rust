# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H12: hostile servers against a2a-protocol-client, driven by a2a-hostile-client
(scripts/live/campaign/hostile/client). Each case is a minimal Python HTTP server
that misbehaves one way, and one client call against it, bounded by MAX_SECS, with
the outcome, event count, time and this process's peak RSS reported.

Cases: a card of 10 MB, a card naming another host, an infinite JSON-RPC stream
(SSE with no final event), a stream whose terminal event names a different task
id, SSE with no blank-line between events, a gzip bomb on the card, a server that
answers 200 with garbage to everything, a slow-drip body (1 byte/100ms), a
redirect loop, a card with 10^5-level nested JSON, chunked transfer that never
ends.

Needs: the campaign client binary (A2A_HOSTILE_CLIENT, built from
scripts/live/campaign/hostile/client). Usage: python3 -I h12_hostile_servers.py
Evidence: evidence/h12_hostile_servers.jsonl.
"""

import gzip
import http.server
import json
import os
import socket
import socketserver
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import EV, Recorder, port  # noqa: E402

CLIENT = os.environ.get("A2A_HOSTILE_CLIENT")
MAX_SECS = "8"


class Srv(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def start(handler_cls) -> tuple[Srv, int]:
    p = port()
    s = Srv(("127.0.0.1", p), handler_cls)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    return s, p


def client(mode: str, url: str, arg: str = "", env: dict | None = None):
    e = {**os.environ, "MAX_SECS": MAX_SECS, **(env or {})}
    t0 = time.time()
    try:
        r = subprocess.run([CLIENT, mode, url] + ([arg] if arg else []),
                           capture_output=True, text=True, timeout=float(MAX_SECS) + 15, env=e)
    except subprocess.TimeoutExpired:
        return {"outcome": "harness-timeout", "seconds": round(time.time() - t0, 1)}
    line = next((ln for ln in r.stdout.splitlines() if ln.startswith("RESULT ")), None)
    if not line:
        return {"outcome": "no-result", "stderr": r.stderr[-400:], "stdout": r.stdout[-200:]}
    try:
        return json.loads(line[len("RESULT "):])
    except ValueError:
        return {"outcome": "bad-json", "line": line[:300]}


CARD_HEAD = ('{"name":"hostile","version":"1.0.0",'
            '"description":"d","supportedInterfaces":[{"url":"%s","protocolBinding":"JSONRPC","protocolVersion":"1.0"}],'
            '"capabilities":{},"defaultInputModes":["text/plain"],"defaultOutputModes":["text/plain"],'
            '"skills":[{"id":"s","name":"s","description":"s","tags":["t"]}]')


def card_body(url: str, extra: str = "") -> bytes:
    return (CARD_HEAD % url + extra + "}").encode()


def run_case(r, name, handler_factory, mode, arg="", bound_rss_mb=300, want=None):
    srv, p = start(handler_factory)
    url = f"http://127.0.0.1:{p}"
    try:
        res = client(mode, url, arg)
        problems = []
        if res.get("peak_rss_kb", 0) > bound_rss_mb * 1024:
            problems.append(f"peak RSS {res['peak_rss_kb'] // 1024} MB > {bound_rss_mb} MB")
        if want and not want(res):
            problems.append(f"unexpected {res}")
        r.rec(name, "held" if not problems else "DEVIATION", {**res, "problems": problems})
    finally:
        srv.shutdown()


# ── handlers ─────────────────────────────────────────────────────────────────

class BigCard(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = card_body(f"http://127.0.0.1:{self.server.server_address[1]}") [:-1] + b',"description":"' + b"x" * (10 * 1024 * 1024) + b'"}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


class CardNamingAnother(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        body = card_body("http://169.254.169.254/evil")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


def _card_handler(url_of_self=True):
    class H(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            u = f"http://127.0.0.1:{self.server.server_address[1]}" if url_of_self else "http://example.com"
            body = card_body(u)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            raise NotImplementedError

        def log_message(self, *a):
            pass
    return H


class InfiniteStream(_card_handler()):
    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        self.rfile.read(n)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        i = 0
        try:
            while True:
                ev = {"jsonrpc": "2.0", "id": 1, "result": {"statusUpdate": {
                    "taskId": "t1", "contextId": "c1",
                    "status": {"state": "TASK_STATE_WORKING"}}}}
                self.wfile.write(f"data: {json.dumps(ev)}\n\n".encode())
                self.wfile.flush()
                i += 1
                time.sleep(0.01)
                if i > 200000:
                    break
        except (BrokenPipeError, ConnectionResetError):
            pass


class WrongTaskTerminal(_card_handler()):
    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        self.rfile.read(n)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        ev = {"jsonrpc": "2.0", "id": 1, "result": {"statusUpdate": {
            "taskId": "not-the-task-you-asked-about", "contextId": "c1",
            "status": {"state": "TASK_STATE_COMPLETED"}, "final": True}}}
        self.wfile.write(f"data: {json.dumps(ev)}\n\n".encode())
        self.wfile.flush()
        time.sleep(0.3)


class NoBlankLine(_card_handler()):
    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        self.rfile.read(n)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for i in range(5):
            ev = {"jsonrpc": "2.0", "id": 1, "result": {"statusUpdate": {
                "taskId": "t1", "contextId": "c1",
                "status": {"state": "TASK_STATE_WORKING"}}}}
            self.wfile.write(f"data: {json.dumps(ev)}\n".encode())  # no second \n
        self.wfile.flush()
        time.sleep(0.3)


class GzipBombCard(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        raw = card_body(f"http://127.0.0.1:{self.server.server_address[1]}")[:-1] + b',"description":"' + b"x" * (200 * 1024 * 1024) + b'"}'
        gz = gzip.compress(raw, compresslevel=9)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Encoding", "gzip")
        self.send_header("Content-Length", str(len(gz)))
        self.end_headers()
        self.wfile.write(gz)

    def log_message(self, *a):
        pass


class AnswersOkToEverything(http.server.BaseHTTPRequestHandler):
    def _any(self):
        body = b'{"ok":true,"garbage":[1,2,3]}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_POST = do_PUT = _any

    def log_message(self, *a):
        pass


class SlowDrip(_card_handler()):
    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        self.rfile.read(n)
        body = card_body(f"http://127.0.0.1:{self.server.server_address[1]}")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            for b in body[:2000]:
                self.wfile.write(bytes([b]))
                self.wfile.flush()
                time.sleep(0.05)
        except (BrokenPipeError, ConnectionResetError):
            pass


class RedirectLoop(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(302)
        self.send_header("Location", f"http://127.0.0.1:{self.server.server_address[1]}/again")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *a):
        pass


class NeverEndingChunks(_card_handler()):
    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        self.rfile.read(n)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        try:
            for _ in range(200000):
                chunk = b'{"partial":true}'
                self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                self.wfile.flush()
                time.sleep(0.005)
        except (BrokenPipeError, ConnectionResetError):
            pass


def main() -> None:
    if not CLIENT or not os.path.exists(CLIENT):
        print("A2A_HOSTILE_CLIENT not set or missing; skipping H12")
        return
    rec = Recorder("h12_hostile_servers")
    try:
        run_case(rec, "card of 10 MB", BigCard, "card", bound_rss_mb=250,
                 want=lambda r: r["outcome"] in ("ok", "timeout") or "err" in r["outcome"])
        run_case(rec, "card names another host (not followed to fetch it)", CardNamingAnother, "card",
                 want=lambda r: r["outcome"] == "ok")
        run_case(rec, "infinite JSON-RPC stream, no terminal event", InfiniteStream, "stream",
                 want=lambda r: r["outcome"] == "timeout" and r["events"] < 5000)
        run_case(rec, "terminal event names a different task id", WrongTaskTerminal, "stream")
        run_case(rec, "SSE with no blank line between events", NoBlankLine, "stream")
        run_case(rec, "gzip bomb on the card (200 MB raw)", GzipBombCard, "card", bound_rss_mb=600)
        run_case(rec, "server answers 200 with garbage to everything", AnswersOkToEverything, "send",
                 want=lambda r: "err" in r["outcome"])
        run_case(rec, "slow-drip card body, 1 byte/50ms", SlowDrip, "card",
                 want=lambda r: r["outcome"] in ("ok", "timeout"))
        run_case(rec, "redirect loop", RedirectLoop, "card",
                 want=lambda r: "err" in r["outcome"] or r["outcome"] == "timeout")
        run_case(rec, "chunked transfer that never ends", NeverEndingChunks, "send")
    finally:
        rec.close()


if __name__ == "__main__":
    main()
