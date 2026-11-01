# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Live probe for NEW-b-2: does the client send loopback traffic through
HTTP_PROXY when NO_PROXY is unset?

Usage: python3 -I probe_proxy_loopback.py A2A_BIN AGENT_URL [NO_PROXY_VALUE]
Starts a logging forward proxy that answers every request 502, runs
`a2a card AGENT_URL` with HTTP_PROXY pointing at it, and prints what the
proxy saw. Also runs the same command with no proxy variables as a control.
"""
import http.server
import json
import os
import subprocess
import sys
import threading

a2a, url = sys.argv[1], sys.argv[2]
no_proxy = sys.argv[3] if len(sys.argv) > 3 else None
seen = []


class P(http.server.BaseHTTPRequestHandler):
    def _any(self):
        seen.append({"method": self.command, "target": self.path, "host": self.headers.get("host")})
        body = b"proxy probe: refused"
        self.send_response(502)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_POST = do_CONNECT = _any

    def log_message(self, *a):
        pass


srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), P)
port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()


def run(env_extra, label):
    env = {k: v for k, v in os.environ.items()
           if k.lower() not in ("http_proxy", "https_proxy", "no_proxy", "all_proxy")}
    env.update(env_extra)
    p = subprocess.run([a2a, "--timeout", "5", "card", url], capture_output=True, text=True,
                       env=env, timeout=60)
    print(f"[{label}] exit={p.returncode} stdout_bytes={len(p.stdout)} stderr={p.stderr.strip()[:200]!r}")


run({}, "control: no proxy variables")
print("proxy saw during control:", json.dumps(seen))
seen.clear()
env = {"HTTP_PROXY": f"http://127.0.0.1:{port}"}
if no_proxy is not None:
    env["NO_PROXY"] = no_proxy
run(env, f"HTTP_PROXY set, NO_PROXY={no_proxy!r}")
print("proxy saw:", json.dumps(seen))
print("LOOPBACK TRAFFIC WENT TO THE PROXY:", bool(seen))
