# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H5: HTTP/1.1 transport abuse against the combined listener (JSON-RPC and
HTTP+JSON on one port). Slowloris headers and bodies, Content-Length lies,
chunked framing abuse, deep JSON nesting, duplicate keys, invalid UTF-8,
header floods. After every attack: the process is alive, no `panicked` line,
RSS and descriptors are measured, and a legitimate send still succeeds.

Needs: the target binary (A2A_HOSTILE_BIN), Python 3 stdlib. Store: in-memory.
Usage: python3 -I h5_transport.py            (about 2 minutes: the slow cases wait)
Writes evidence/h5_transport.jsonl.
"""

import json
import os
import socket
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, msg, rpc, send, task_of  # noqa: E402

GOOD_BODY = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendMessage",
                        "params": {"message": msg("echo:x")}}).encode()


def conn(port):
    s = socket.create_connection(("127.0.0.1", port), timeout=10)
    return s


def req(path="/", extra=b"", cl=None, body=b"", method=b"POST", version=b"HTTP/1.1"):
    head = method + b" " + path.encode() + b" " + version + b"\r\nHost: 127.0.0.1\r\n" \
        b"Content-Type: application/json\r\nA2A-Version: 1.0\r\n"
    if cl is not None:
        head += b"Content-Length: " + str(cl).encode() + b"\r\n"
    return head + extra + b"\r\n" + body


def read_all(s, limit=65536, timeout=10):
    s.settimeout(timeout)
    out = b""
    try:
        while len(out) < limit:
            d = s.recv(65536)
            if not d:
                break
            out += d
    except (socket.timeout, OSError):
        pass
    return out


def status_of(raw: bytes):
    try:
        return int(raw.split(b" ", 2)[1])
    except (IndexError, ValueError):
        return None


def healthy(t):
    """A legitimate send within 10 s."""
    try:
        st, r = send(t.port, "echo:alive")
        return st == 200 and task_of(r) is not None
    except Exception as e:  # noqa: BLE001
        return f"{type(e).__name__}: {e}"


def main() -> None:
    t = Target("h5_transport", {"STORE": "mem"})
    r = Recorder("h5_transport")
    base = {"n": 0}

    def after(name, expect_ok=True, detail=None, accept=None):
        """Record the aftermath of an attack: liveness, panics, rss, fds, health."""
        time.sleep(0.3)
        h = healthy(t)
        st = t.state()
        problems = []
        if not st["alive"]:
            problems.append("TARGET DEAD")
        if st["panics"]:
            problems.append(f"{st['panics']} panicked line(s)")
        if h is not True:
            problems.append(f"legit send failed: {h}")
        if accept and not accept[0]:
            problems.append(accept[1])
        r.rec(name, "held" if not problems else "DEVIATION", {"problems": problems, "state": st, **(detail or {})})

    def answer(name, raw_request, ok_statuses, timeout=10, keep_open=False):
        s = conn(t.port)
        try:
            s.sendall(raw_request)
        except OSError:
            pass
        out = read_all(s, timeout=timeout)
        s.close()
        code = status_of(out)
        after(name, detail={"status": code, "first_line": out.split(b"\r\n", 1)[0][:100].decode("latin1")},
              accept=(code in ok_statuses or (code is None and None in ok_statuses),
                      f"status {code} not in {ok_statuses}"))

    # ── baseline ─────────────────────────────────────────────────────────────
    t0 = t.state()
    r.rec("baseline", "held", t0)

    # ── Content-Length and framing ───────────────────────────────────────────
    answer("CL declares 2 GB, sends 10 bytes then waits", req(cl=2_000_000_000, body=b"{}" * 5), [413, 400, None], timeout=5)
    answer("CL = 2^64", req(cl=2**64, body=b"{}"), [400, 413, None])
    answer("CL = -1", req(extra=b"", cl=-1, body=b"{}"), [400, None])
    answer("CL = 1e3 (not a number)", req(path="/", extra=b"", cl="1e3", body=b"{}" * 500), [400, None])
    answer("two Content-Length headers that disagree", req(extra=b"Content-Length: 5\r\n", cl=len(GOOD_BODY), body=GOOD_BODY), [400, None])
    answer("Content-Length and Transfer-Encoding: chunked together (smuggling)",
           req(extra=b"Transfer-Encoding: chunked\r\n", cl=len(GOOD_BODY), body=b"0\r\n\r\n"), [400, 200, None])
    answer("CL shorter than body: the rest is a second request", req(cl=len(GOOD_BODY), body=GOOD_BODY + b"GET /nope HTTP/1.1\r\n\r\n"), [200])
    answer("CL = 0 on a POST", req(cl=0, body=b""), [200, 400])
    answer("Expect: 100-continue then 3 GB declared", req(extra=b"Expect: 100-continue\r\n", cl=3_000_000_000), [413, 400, 417, 100, None], timeout=5)

    # ── chunked ──────────────────────────────────────────────────────────────
    def chunked(body_chunks):
        return req(extra=b"Transfer-Encoding: chunked\r\n", body=body_chunks)

    answer("chunk size 7fffffff then 1 MB", chunked(b"7fffffff\r\n" + b"a" * 1_000_000), [413, 400, None], timeout=5)
    answer("chunk size ffffffffffffffffff (overflow)", chunked(b"ffffffffffffffffff\r\nabc\r\n0\r\n\r\n"), [400, None])
    answer("chunk extension of 1 MB", chunked(b"5;" + b"x" * 1_000_000 + b"\r\nhello\r\n0\r\n\r\n"), [400, 413, 431, None], timeout=5)
    answer("valid chunked body", chunked(hex(len(GOOD_BODY))[2:].encode() + b"\r\n" + GOOD_BODY + b"\r\n0\r\n\r\n"), [200])
    answer("chunked with 100000 one-byte chunks", chunked(b"1\r\n{\r\n" * 100000 + b"0\r\n\r\n"), [400, 413, 200], timeout=20)
    answer("chunk trailer of 1 MB", chunked(hex(len(GOOD_BODY))[2:].encode() + b"\r\n" + GOOD_BODY + b"\r\n0\r\nX: " + b"a" * 1_000_000 + b"\r\n\r\n"), [200, 400, 413, 431, None], timeout=10)

    # ── JSON shape ───────────────────────────────────────────────────────────
    for depth in (1000, 100_000, 1_000_000):
        for label, o, c in (("array", "[", "]"), ("object", '{"a":', "}")):
            body = ('{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":' + o * depth + "1" + c * depth + "}").encode()
            answer(f"JSON nested {depth} deep ({label}) in params", req(cl=len(body), body=body), [200, 400, 413], timeout=10)
    body = (b'{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":' + b'{"metadata":' * 50000 + b"1" + b"}" * 50000 + b"}}")
    answer("nested metadata 50000 deep inside a message", req(cl=len(body), body=body), [200, 400, 413])
    for label, body in (
        ("duplicate method key", b'{"jsonrpc":"2.0","id":1,"method":"GetTask","method":"SendMessage","params":{"id":"x"}}'),
        ("duplicate params key", b'{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"x"},"params":{"id":"y"}}'),
        ("duplicate id inside params", b'{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"x","id":"y"}}'),
        ("BOM before JSON", b'\xef\xbb\xbf{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"x"}}'),
        ("invalid UTF-8 in a string", b'{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"\xff\xfe"}}'),
        ("overlong UTF-8 NUL", b'{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"\xc0\x80"}}'),
        ("trailing garbage after JSON", b'{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"x"}}garbage'),
        ("a batch of 10000 requests", b"[" + b",".join([b'{"jsonrpc":"2.0","id":%d,"method":"GetTask","params":{"id":"x"}}' % i for i in range(10000)]) + b"]"),
        ("empty batch", b"[]"),
        ("batch of notifications", b"[" + b",".join([b'{"jsonrpc":"2.0","method":"GetTask","params":{"id":"x"}}'] * 100) + b"]"),
        ("id is an object", b'{"jsonrpc":"2.0","id":{"a":1},"method":"GetTask","params":{"id":"x"}}'),
        ("id is a 1 MB string", b'{"jsonrpc":"2.0","id":"' + b"a" * 1_000_000 + b'","method":"GetTask","params":{"id":"x"}}'),
        ("method is a 1 MB string", b'{"jsonrpc":"2.0","id":1,"method":"' + b"a" * 1_000_000 + b'","params":{}}'),
        ("number 1e999 id", b'{"jsonrpc":"2.0","id":1e999,"method":"GetTask","params":{"id":"x"}}'),
    ):
        answer(f"JSON-RPC: {label}", req(cl=len(body), body=body), [200, 400, 413])
    # ── request line, path and header floods ─────────────────────────────────
    answer("request target of 1 MB", req(path="/" + "a" * 1_000_000, cl=2, body=b"{}"), [414, 400, 431, 404, None])
    answer("request target with invalid UTF-8 / controls", b"GET /tasks/\xff\xfe\x00 HTTP/1.1\r\nHost: x\r\n\r\n", [400, 404, None])
    answer("10 000 header lines", req(extra=b"".join(b"X-%d: v\r\n" % i for i in range(10000)), cl=len(GOOD_BODY), body=GOOD_BODY), [431, 400, 200, None])
    answer("one header value of 100 KB", req(extra=b"X-Big: " + b"a" * 100_000 + b"\r\n", cl=len(GOOD_BODY), body=GOOD_BODY), [431, 400, 200, None])
    answer("one header value of 10 MB", req(extra=b"X-Big: " + b"a" * 10_000_000 + b"\r\n", cl=len(GOOD_BODY), body=GOOD_BODY), [431, 400, None], timeout=10)
    answer("header with a bare LF", b"POST / HTTP/1.1\nHost: x\nContent-Length: 0\n\n", [400, 200, None])
    answer("header name with space (request smuggling)", req(extra=b"Transfer-Encoding : chunked\r\n", cl=len(GOOD_BODY), body=GOOD_BODY), [400, 200])
    answer("obsolete line folding", req(extra=b"X-A: 1\r\n  folded\r\n", cl=len(GOOD_BODY), body=GOOD_BODY), [400, 200])
    answer("HTTP/0.9 request", b"GET /\r\n", [400, None])
    answer("HTTP/1.0 without Host", b"POST / HTTP/1.0\r\nContent-Length: 2\r\n\r\n{}", [200, 400])
    answer("absolute-form request target to another host", b"POST http://evil.example/ HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\n\r\n{}", [200, 400, 421])
    answer("Host header of a foreign name (DNS rebinding)", req(extra=b"", cl=len(GOOD_BODY), body=GOOD_BODY).replace(b"Host: 127.0.0.1", b"Host: evil.example"), [200, 421, 403, 400])
    answer("TRACE method", b"TRACE / HTTP/1.1\r\nHost: x\r\n\r\n", [405, 404, 400, 501])
    answer("CONNECT method", b"CONNECT 127.0.0.1:80 HTTP/1.1\r\nHost: x\r\n\r\n", [405, 400, 404, 501, None])
    answer("method of 100 KB", b"A" * 100000 + b" / HTTP/1.1\r\nHost: x\r\n\r\n", [400, 405, 501, None])
    answer("pipelined 5000 GET /health in one write", b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n" * 5000, [200, 404], timeout=15)
    r.rec("segment done", "held", t.state())

    # ── slow clients (the documented bounds: header read 30 s, idle 75 s) ────
    results = {}

    def slow_headers(n=150):
        socks = []
        for _ in range(n):
            s = conn(t.port)
            s.sendall(b"POST / HTTP/1.1\r\nHost: x\r\nX-A: ")
            socks.append(s)
        t_open = time.time()
        closed_at = {}
        while time.time() - t_open < 45 and len(closed_at) < n:
            for i, s in enumerate(socks):
                if i in closed_at:
                    continue
                try:
                    s.sendall(b"a")  # one byte every ~2 s keeps it "making progress"
                except OSError:
                    closed_at[i] = time.time() - t_open
                    continue
                s.setblocking(False)
                try:
                    if s.recv(1) == b"":
                        closed_at[i] = time.time() - t_open
                except BlockingIOError:
                    pass
                except OSError:
                    closed_at[i] = time.time() - t_open
                s.setblocking(True)
            time.sleep(2)
        results["slow_headers"] = {"opened": n, "closed": len(closed_at),
                                   "first_close_s": round(min(closed_at.values()), 1) if closed_at else None,
                                   "last_close_s": round(max(closed_at.values()), 1) if closed_at else None,
                                   "fds_during": t.fds()}
        for s in socks:
            s.close()

    def slow_body(n=50):
        socks = []
        for _ in range(n):
            s = conn(t.port)
            s.sendall(req(cl=100000, body=b"{"))
            socks.append(s)
        t_open = time.time()
        results["slow_body_midway"] = {"fds": t.fds()}
        closed = 0
        while time.time() - t_open < 100 and closed < n:
            closed = 0
            for s in socks:
                s.setblocking(False)
                try:
                    if s.recv(1) == b"":
                        closed += 1
                    else:
                        closed += 1  # any response (408/400) also ends it
                except BlockingIOError:
                    pass
                except OSError:
                    closed += 1
                s.setblocking(True)
            time.sleep(3)
        results["slow_body"] = {"opened": n, "closed_by_server": closed,
                                "seconds": round(time.time() - t_open, 1)}
        for s in socks:
            s.close()

    th = [threading.Thread(target=slow_headers), threading.Thread(target=slow_body)]
    [x.start() for x in th]
    # while they run, a legitimate client must still be served
    t_end = time.time() + 20
    lat = []
    while time.time() < t_end:
        a = time.time()
        ok = healthy(t)
        lat.append(round(time.time() - a, 3))
        if ok is not True:
            lat.append(str(ok))
        time.sleep(1)
    [x.join() for x in th]
    sh = results.get("slow_headers", {})
    r.rec("slowloris headers: 150 connections trickling one byte / 2 s are closed by the 30 s header timeout",
          "held" if sh.get("closed") == 150 and (sh.get("last_close_s") or 99) <= 40 else "DEVIATION", sh)
    sb = results.get("slow_body", {})
    r.rec("slow body: 50 connections declaring 100000 bytes and sending 1 are closed within the idle bound",
          "held" if sb.get("closed_by_server") == 50 else "DEVIATION", {**sb, **results.get("slow_body_midway", {})})
    r.rec("legitimate clients served while both slow attacks ran",
          "held" if all(isinstance(x, float) and x < 3 for x in lat) else "DEVIATION", {"latencies_s": lat})
    time.sleep(2)
    after("after the slow attacks", detail={"fds_back": t.fds()})
    end = t.state()
    r.rec("descriptors and memory back near baseline after everything",
          "held" if end["fds"] <= t0["fds"] + 3 and end["rss_kb"] < t0["rss_kb"] * 4 else "DEVIATION",
          {"baseline": t0, "end": end})
    r.close()
    t.stop()


if __name__ == "__main__":
    main()
