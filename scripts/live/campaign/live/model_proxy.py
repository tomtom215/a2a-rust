#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A logging pass-through between the SDK and a real model server.

    model_proxy.py LISTEN_PORT UPSTREAM_PORT LOGFILE

Needs python3. It forwards every request to http://127.0.0.1:UPSTREAM_PORT unchanged
(streaming answers are relayed chunk by chunk) and appends one JSON line per request to
LOGFILE: the request body (what the SDK really sent the model) and the response's status,
size and, up to 20 kB, its text. It exists so a driver can say what the model was sent
after a steering message, a tool result with control characters, a skill's text.

A request path that is not printable ASCII starting with "/" is answered 400 (it would
otherwise raise inside http.client and drop the connection), and a response header the
upstream sends is relayed with any CR or LF removed from its name and replaced by a space in
its value.
"""
import http.client
import json
import re
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LISTEN, UP, LOG = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
LOCK = threading.Lock()
N = [0]


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_GET(self):
        self.forward(b"")

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        self.forward(self.rfile.read(n))

    def forward(self, body):
        path = self.path
        if not re.fullmatch(r"/[\x21-\x7e]*", path):
            self.send_error(400, "the request path must be printable ASCII and start with /")
            return
        with LOCK:
            N[0] += 1
            seq = N[0]
        t0 = time.time()
        conn = http.client.HTTPConnection("127.0.0.1", UP, timeout=3600)
        headers = {k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection", "content-length", "accept-encoding")}
        headers["Content-Length"] = str(len(body))
        conn.request(self.command, path, body=body, headers=headers)
        r = conn.getresponse()
        self.send_response(r.status)
        sent = bytearray()
        for k, v in r.getheaders():
            if k.lower() in ("transfer-encoding", "connection", "content-length"):
                continue
            self.send_header(k.replace("\r", "").replace("\n", ""), v.replace("\r", " ").replace("\n", " "))
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        total = 0
        while True:
            piece = r.read1(65536)
            if not piece:
                break
            total += len(piece)
            if len(sent) < 20000:
                sent.extend(piece[: 20000 - len(sent)])
            self.wfile.write(f"{len(piece):x}\r\n".encode() + piece + b"\r\n")
            self.wfile.flush()
        self.wfile.write(b"0\r\n\r\n")
        conn.close()
        try:
            req = json.loads(body) if body else None
        except ValueError:
            req = {"unparsed": body[:2000].decode("utf-8", "replace")}
        with LOCK:
            with open(LOG, "a") as f:
                f.write(json.dumps({"seq": seq, "t_start": t0, "t_end": time.time(), "path": self.path, "request": req,
                                    "status": r.status, "response_bytes": total,
                                    "response_head": sent.decode("utf-8", "replace")}) + "\n")


if __name__ == "__main__":
    s = ThreadingHTTPServer(("127.0.0.1", LISTEN), H)
    s.daemon_threads = True
    print(f"model_proxy {LISTEN} -> {UP}", flush=True)
    s.serve_forever()
