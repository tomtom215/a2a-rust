#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A misbehaving A2A peer for exercising the `a2a` CLI.

    cli_stub.py [BASE_PORT]

Serves, each on its own loopback port (BASE_PORT defaults to 3400):

    BASE+1  401 on everything, `WWW-Authenticate: Bearer realm="a2a"`
    BASE+2  429 on everything, `Retry-After: 2`
    BASE+3  500 with a non-JSON body on everything
    BASE+4  accepts, reads the request, never answers (a half-open peer)
    BASE+5  a card that points at port BASE+6; BASE+6 answers a stream with
            one SSE event, then stalls for 60 s
    BASE+7  a card whose interface URL names another host (credential-forwarding guard)
    BASE+8  200 with a body that is not JSON, then closes the connection
    BASE+9  answers a stream with half an SSE event, then closes

Every request's method, path and headers are appended to
`$STUB_LOG` (default ./cli_stub.log), so a run can show which header values
were really sent. Needs python3 only.
"""
import json
import os
import socket
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

BASE = int(sys.argv[1]) if len(sys.argv) > 1 else 3400
LOG = os.environ.get("STUB_LOG", "cli_stub.log")
LOCK = threading.Lock()


def log(port, h, body=b""):
    with LOCK, open(LOG, "a") as f:
        f.write(f"{port} {h.command} {h.path} " + json.dumps(dict(h.headers)) + (f" body={body[:200]!r}\n" if body else "\n"))


def card(url, streaming=True):
    return {
        "name": "stub", "description": "misbehaving stub", "version": "0.0.1",
        "supportedInterfaces": [{"url": url, "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}],
        "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "s", "name": "s", "description": "s", "tags": ["t"]}],
        "capabilities": {"streaming": streaming},
    }


def make(port):
    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a):
            pass

        def _body(self):
            n = int(self.headers.get("content-length") or 0)
            return self.rfile.read(n) if n else b""

        def _send(self, code, body=b"", ctype="application/json", extra=()):
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            for k, v in extra:
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(body)

        def handle_any(self):
            body = self._body()
            log(port, self, body)
            k = port - BASE
            if k == 1:
                self._send(401, b'{"error":"unauthorized"}', extra=[("WWW-Authenticate", 'Bearer realm="a2a"')])
            elif k == 2:
                self._send(429, b'{"error":"slow down"}', extra=[("Retry-After", "2")])
            elif k == 3:
                self._send(500, b"<html>boom</html>", ctype="text/html")
            elif k == 4:
                time.sleep(120)
            elif k == 5:
                if "agent-card" in self.path:
                    self._send(200, json.dumps(card(f"http://127.0.0.1:{BASE + 6}")).encode())
                else:
                    self._send(404, b"{}")
            elif k == 6:
                if "agent-card" in self.path:
                    self._send(200, json.dumps(card(f"http://127.0.0.1:{BASE + 6}")).encode())
                    return
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Transfer-Encoding", "chunked")
                self.end_headers()
                ev = {"jsonrpc": "2.0", "id": 1, "result": {"task": {"id": "t1", "contextId": "c1", "status": {"state": "TASK_STATE_WORKING"}}}}
                data = f"data: {json.dumps(ev)}\n\n".encode()
                self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
                self.wfile.flush()
                time.sleep(60)
            elif k == 7:
                if "agent-card" in self.path:
                    self._send(200, json.dumps(card("http://evil.example.invalid:9")).encode())
                else:
                    self._send(404, b"{}")
            elif k == 8:
                self._send(200, b"this is not json", ctype="application/json")
                self.close_connection = True
            elif k == 9:
                if "agent-card" in self.path:
                    self._send(200, json.dumps(card(f"http://127.0.0.1:{BASE + 9}")).encode())
                    return
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(b'data: {"jsonrpc":"2.0","id":1,"result":{"task":{"id":"t1","con')
                self.wfile.flush()
                self.close_connection = True
            else:
                self._send(404, b"{}")

        do_GET = do_POST = do_DELETE = do_PUT = handle_any

    return H


def main():
    servers = []
    for i in range(1, 10):
        s = ThreadingHTTPServer(("127.0.0.1", BASE + i), make(BASE + i))
        s.daemon_threads = True
        threading.Thread(target=s.serve_forever, daemon=True).start()
        servers.append(s)
    print(f"cli_stub listening on 127.0.0.1:{BASE + 1}..{BASE + 9}", flush=True)
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    main()
