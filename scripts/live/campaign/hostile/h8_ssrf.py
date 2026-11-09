# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H8: push-notification webhook SSRF. A canary listener on 127.0.0.1 records every
request that reaches it; each hostile URL is registered as a webhook (standalone
`CreateTaskPushNotificationConfig` and inline in `SendMessage`), a terminal event
is caused, and any canary hit with the guard on is a bypass. The URL forms:
loopback in every spelling (decimal, octal, hex, short, mapped IPv6, trailing
dot, userinfo tricks, percent-encoding, IDN dots), other schemes, a name mapped
to 127.0.0.1 in /etc/hosts (restored afterwards), and, with the guard off
(`PUSH=private`, as a control), a redirect from one listener to another.

Needs: the target binary (A2A_HOSTILE_BIN). Writes /etc/hosts for one mapped name
and restores it byte for byte (checked by hash); pass --no-hosts to skip that.
Usage: python3 -I h8_ssrf.py [--no-hosts]. Evidence: evidence/h8_ssrf.jsonl.
"""

import hashlib
import http.server
import json
import os
import socketserver
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, msg, port, rpc, send, task_of  # noqa: E402

HOSTS = "/etc/hosts"
MAPPED = "a2a-camp-internal.example.net"
HITS = []
REDIRECT_TO = [None]


class Canary(http.server.BaseHTTPRequestHandler):
    def _hit(self):
        n = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(n) if n else b""
        HITS.append({"port": self.server.server_address[1], "path": self.path, "host": self.headers.get("host"),
                     "method": self.command, "len": len(body)})
        if REDIRECT_TO[0] and self.server.server_address[1] == CANARY_PORT:
            self.send_response(302)
            self.send_header("Location", REDIRECT_TO[0])
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")

    do_POST = do_GET = do_PUT = _hit

    def log_message(self, *a):
        pass


class Srv(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


CANARY_PORT = port()
SECOND_PORT = port()


def serve(p):
    s = Srv(("127.0.0.1", p), Canary)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    return s


def urls(p):
    return {
        "dotted loopback": f"http://127.0.0.1:{p}/h",
        "decimal": f"http://2130706433:{p}/h",
        "hex": f"http://0x7f000001:{p}/h",
        "hex dotted": f"http://0x7f.0x0.0x0.0x1:{p}/h",
        "octal": f"http://017700000001:{p}/h",
        "octal dotted": f"http://0177.0.0.01:{p}/h",
        "short 127.1": f"http://127.1:{p}/h",
        "short 127.0.1": f"http://127.0.1:{p}/h",
        "zero": f"http://0:{p}/h",
        "0.0.0.0": f"http://0.0.0.0:{p}/h",
        "mapped ipv6": f"http://[::ffff:127.0.0.1]:{p}/h",
        "mapped ipv6 hex": f"http://[::ffff:7f00:1]:{p}/h",
        "ipv6 compat": f"http://[::127.0.0.1]:{p}/h",
        "nat64": f"http://[64:ff9b::7f00:1]:{p}/h",
        "6to4": f"http://[2002:7f00:1::]:{p}/h",
        "ipv6 loopback": f"http://[::1]:{p}/h",
        "trailing dot": f"http://127.0.0.1.:{p}/h",
        "localhost": f"http://localhost:{p}/h",
        "localhost dot": f"http://localhost.:{p}/h",
        "LOCALHOST": f"http://LOCALHOST:{p}/h",
        "sub.localhost": f"http://x.localhost:{p}/h",
        "userinfo": f"http://user:pw@127.0.0.1:{p}/h",
        "userinfo public-looking": f"http://example.com@127.0.0.1:{p}/h",
        "userinfo host swap": f"http://127.0.0.1:{p}@example.com/h",
        "fragment": f"http://example.com#@127.0.0.1:{p}/h",
        "backslash": f"http://example.com\\@127.0.0.1:{p}/h",
        "tab in host": f"http://127.0.0.1\t:{p}/h",
        "percent-encoded dot": f"http://127%2e0%2e0%2e1:{p}/h",
        "percent-encoded host": f"http://%31%32%37.0.0.1:{p}/h",
        "fullwidth digits": f"http://１２７.０.０.１:{p}/h",
        "ideographic dot": f"http://127。0。0。1:{p}/h",
        "zone id": f"http://[fe80::1%25lo]:{p}/h",
        "link-local metadata": f"http://169.254.169.254:{p}/latest/meta-data/",
        "metadata decimal": f"http://2852039166:{p}/h",
        "cgnat": f"http://100.64.0.1:{p}/h",
        "private 10/8": f"http://10.0.0.1:{p}/h",
        "private 192.168": f"http://192.168.0.1:{p}/h",
        "single label": f"http://metadata:{p}/h",
        "kubernetes": f"http://kubernetes.default.svc:{p}/h",
        "internal suffix": f"http://svc.internal:{p}/h",
        "local suffix": f"http://printer.local:{p}/h",
        "mapped name": f"http://{MAPPED}:{p}/h",
        "file scheme": "file:///etc/passwd",
        "gopher": f"gopher://127.0.0.1:{p}/_x",
        "ftp": f"ftp://127.0.0.1:{p}/x",
        "ws": f"ws://127.0.0.1:{p}/x",
        "javascript": "javascript:alert(1)",
        "data": "data:text/plain,hi",
        "no scheme": f"127.0.0.1:{p}/h",
        "empty": "",
        "huge url": "http://example.com/" + "a" * 100000,
        "scheme only": "http://",
        "double slash host": f"http:///127.0.0.1:{p}/h",
        "port 0": "http://example.com:0/h",
        "port 99999": "http://example.com:99999/h",
        "nul in url": f"http://127.0.0.1\x00.example.com:{p}/h",
        "newline in url": f"http://127.0.0.1:{p}/h\r\nX-Injected: 1",
    }


def main() -> None:
    use_hosts = "--no-hosts" not in sys.argv
    saved = open(HOSTS, "rb").read()
    saved_hash = hashlib.sha256(saved).hexdigest()
    srv1, srv2 = serve(CANARY_PORT), serve(SECOND_PORT)
    r = Recorder("h8_ssrf")
    t = Target("h8_ssrf", {"STORE": "mem", "PUSH": "guard"})
    try:
        if use_hosts:
            with open(HOSTS, "ab") as f:
                f.write(f"127.0.0.1 {MAPPED}\n".encode())
        for label, url in urls(CANARY_PORT).items():
            HITS.clear()
            # standalone create on a held task
            st, a = send(t.port, "hold", now=True)
            task = task_of(a)
            time.sleep(0.1)
            cfg = {"taskId": task["id"], "url": url, "id": "c1", "token": "tok"}
            st, c = rpc(t.port, "CreateTaskPushNotificationConfig", cfg)
            created = isinstance(c, dict) and "result" in c
            code = (c.get("error") or {}).get("code") if isinstance(c, dict) else None
            rpc(t.port, "CancelTask", {"id": task["id"]})
            time.sleep(0.4)
            standalone_hits = len(HITS)
            # inline in the send
            HITS.clear()
            m = msg("echo:inline")
            st, b = rpc(t.port, "SendMessage", {"message": m, "configuration": {
                "taskPushNotificationConfig": {"url": url, "token": "tok"}}})
            inline_ok = isinstance(b, dict) and "result" in b
            time.sleep(0.6)
            inline_hits = len(HITS)
            hit = standalone_hits or inline_hits
            r.rec(f"webhook {label}: {url[:70]!r}", "held" if not hit else "DEVIATION",
                  {"standalone_created": created, "standalone_error": code, "standalone_canary_hits": standalone_hits,
                   "inline_accepted": inline_ok, "inline_canary_hits": inline_hits,
                   "message": (c.get("error") or {}).get("message", "")[:140] if isinstance(c, dict) else ""})
        r.rec("target alive, no panic", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
        t.stop()
        # control: guard off. The control proves the canary is reachable and the delivery path works,
        # and shows whether a 302 is followed.
        t2 = Target("h8_ssrf_private", {"STORE": "mem", "PUSH": "private"})
        try:
            HITS.clear()
            st, a = send(t2.port, "hold", now=True)
            task = task_of(a)
            rpc(t2.port, "CreateTaskPushNotificationConfig",
                {"taskId": task["id"], "url": f"http://127.0.0.1:{CANARY_PORT}/direct", "id": "c1"})
            rpc(t2.port, "CancelTask", {"id": task["id"]})
            time.sleep(1.0)
            r.rec("control (guard off): a loopback webhook is delivered", "held" if HITS else "DEVIATION", list(HITS))
            REDIRECT_TO[0] = f"http://127.0.0.1:{SECOND_PORT}/redirected"
            HITS.clear()
            st, a = send(t2.port, "hold", now=True)
            task = task_of(a)
            rpc(t2.port, "CreateTaskPushNotificationConfig",
                {"taskId": task["id"], "url": f"http://127.0.0.1:{CANARY_PORT}/redirect-me", "id": "c1"})
            rpc(t2.port, "CancelTask", {"id": task["id"]})
            time.sleep(1.5)
            followed = [h for h in HITS if h["port"] == SECOND_PORT]
            r.rec("a 302 from a webhook is not followed", "held" if not followed else "DEVIATION", list(HITS))
        finally:
            t2.stop()
    finally:
        with open(HOSTS, "wb") as f:
            f.write(saved)
        back = hashlib.sha256(open(HOSTS, "rb").read()).hexdigest()
        r.rec("/etc/hosts restored byte for byte", "held" if back == saved_hash else "DEVIATION", {"sha256": back})
        r.close()
        if t.alive():
            t.stop()
        srv1.shutdown()
        srv2.shutdown()


if __name__ == "__main__":
    main()
