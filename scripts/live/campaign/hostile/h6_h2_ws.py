# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H6: HTTP/2 (cleartext, prior knowledge) and WebSocket abuse, SSE clients that
never read, and many idle connections, all by hand-built frames (no h2 or
websockets package is installed; `h2spec` is not installed either).

HTTP/2: rapid reset, CONTINUATION flood, SETTINGS flood, PING flood, PRIORITY
flood, an oversized frame, a zero flow-control window with many streams.
WebSocket: fragmentation with an interleaved ping, a 16 MB frame, a message
over 4 MiB in fragments, a ping flood, a binary frame, an unmasked frame,
invalid UTF-8, reserved bits, control-frame violations.
SSE: 50 subscribers that never read a 300 000-event task. Idle: 10 000
connections that send nothing.

Each case reports RSS peak and after, descriptors, CPU seconds the target spent,
liveness, `panicked` lines, and whether a legitimate send still succeeds.

Needs: the target binary (A2A_HOSTILE_BIN). Usage: python3 -I h6_h2_ws.py [h2|ws|sse|idle ...]
Writes evidence/h6_<group>.jsonl.
"""

import base64
import json
import os
import socket
import struct
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import BASE_HEADERS, Recorder, Target, msg, rpc, send, task_of  # noqa: E402
from h6_common import healthy, verdict  # noqa: E402


# ── HTTP/2 ───────────────────────────────────────────────────────────────────

PREFACE = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"


def h2f(ftype, flags, sid, payload=b""):
    return struct.pack(">I", len(payload))[1:] + bytes([ftype, flags]) + struct.pack(">I", sid) + payload


HDRS = (b"\x83\x84\x86" + b"\x41\x09127.0.0.1" + b"\x5f\x10application/json"
        + b"\x40\x0ba2a-version\x03" + b"1.0")  # POST / http, authority, content-type, a2a-version


def h2_open(t):
    s = socket.create_connection(("127.0.0.1", t.port), timeout=10)
    s.sendall(PREFACE + h2f(4, 0, 0))
    return s


def h2_drain(s, seconds=1.0):
    """Reads frames for a while; returns the list of (type, flags, sid, payload[:16])."""
    s.settimeout(0.2)
    end, buf, frames = time.time() + seconds, b"", []
    while time.time() < end:
        try:
            d = s.recv(65536)
            if not d:
                frames.append(("EOF", 0, 0, b""))
                break
            buf += d
        except socket.timeout:
            continue
        except OSError as e:
            frames.append(("ERR", 0, 0, str(e).encode()))
            break
    while len(buf) >= 9:
        n = int.from_bytes(buf[:3], "big")
        if len(buf) < 9 + n:
            break
        frames.append((buf[3], buf[4], int.from_bytes(buf[5:9], "big") & 0x7FFFFFFF, buf[9:9 + 16]))
        buf = buf[9 + n:]
    return frames


def h2(r, t):
    # a. h2c works at all
    def basic():
        s = h2_open(t)
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendMessage",
                           "params": {"message": msg("echo:h2")}}).encode()
        s.sendall(h2f(1, 4, 1, HDRS) + h2f(0, 1, 1, body))
        fr = h2_drain(s, 2.0)
        s.close()
        return {"frames": [(f[0], f[1], f[2]) for f in fr][:8]}
    verdict(t, r, "h2c prior-knowledge request is served (or refused cleanly)", basic,
            extra_ok=lambda d: any(f[0] == 1 for f in d.get("frames", [])))

    def rapid_reset():
        s = h2_open(t)
        sent = 0
        t_end = time.time() + 20
        try:
            sid = 1
            while sent < 200_000 and time.time() < t_end:
                chunk = b""
                for _ in range(1000):
                    chunk += h2f(1, 4, sid, HDRS) + h2f(3, 0, sid, struct.pack(">I", 8))
                    sid += 2
                    if sid > 2**31 - 3:
                        break
                s.sendall(chunk)
                sent += 1000
        except OSError:
            pass
        fr = h2_drain(s, 1.0)
        goaway = [f for f in fr if f[0] == 7]
        s.close()
        return {"streams_opened_and_reset": sent, "goaway": bool(goaway), "goaway_payload": (goaway[0][3].hex() if goaway else None)}
    verdict(t, r, "HTTP/2 rapid reset: up to 200 000 HEADERS+RST_STREAM pairs on one connection", rapid_reset)

    def continuation_flood():
        s = h2_open(t)
        sent = 0
        try:
            s.sendall(h2f(1, 0, 1, HDRS))  # HEADERS without END_HEADERS
            junk = h2f(9, 0, 1, b"\x00\x0a" + b"x-a" + b"\x00" * 16000)
            for _ in range(20000):  # ~320 MB if never refused
                s.sendall(junk)
                sent += 1
        except OSError:
            pass
        fr = h2_drain(s, 1.0)
        s.close()
        return {"continuation_frames_accepted": sent, "of": 20000, "frames": [(f[0], f[2]) for f in fr][:4]}
    verdict(t, r, "HTTP/2 CONTINUATION flood: 20 000 x 16 KB frames after one HEADERS", continuation_flood,
            extra_ok=lambda d: d["continuation_frames_accepted"] < 20000)

    def settings_flood():
        s = h2_open(t)
        sent = 0
        try:
            for _ in range(100_000):
                s.sendall(h2f(4, 0, 0))
                sent += 1
        except OSError:
            pass
        fr = h2_drain(s, 1.0)
        s.close()
        return {"settings_sent": sent, "frames": [(f[0], f[2]) for f in fr][:3]}
    verdict(t, r, "HTTP/2 SETTINGS flood: 100 000 SETTINGS frames, never reading the ACKs", settings_flood)

    def ping_flood():
        s = h2_open(t)
        sent = 0
        try:
            for _ in range(200_000):
                s.sendall(h2f(6, 0, 0, b"12345678"))
                sent += 1
        except OSError:
            pass
        s.close()
        return {"pings_sent": sent}
    verdict(t, r, "HTTP/2 PING flood: 200 000 pings, never reading the replies", ping_flood)

    def priority_flood():
        s = h2_open(t)
        sent = 0
        try:
            for i in range(200_000):
                s.sendall(h2f(2, 0, 2 * i + 1, b"\x00\x00\x00\x00\x10"))
                sent += 1
        except OSError:
            pass
        s.close()
        return {"priority_frames_sent": sent}
    verdict(t, r, "HTTP/2 PRIORITY flood: 200 000 frames", priority_flood)

    def big_frame():
        s = h2_open(t)
        s.sendall(h2f(0, 0, 1, b"x" * 20000))  # over the default 16384 SETTINGS_MAX_FRAME_SIZE
        fr = h2_drain(s, 1.0)
        s.close()
        return {"frames": [(f[0], f[2], f[3].hex()) for f in fr][:4]}
    verdict(t, r, "HTTP/2 frame larger than SETTINGS_MAX_FRAME_SIZE is refused", big_frame,
            extra_ok=lambda d: any(f[0] in (7, "EOF") for f in d["frames"]) or any(f[0] == 3 for f in d["frames"]))

    def zero_window():
        s = socket.create_connection(("127.0.0.1", t.port), timeout=10)
        # SETTINGS_INITIAL_WINDOW_SIZE = 0 (id 4), then 90 GETs of the agent card, never opening the window.
        s.sendall(PREFACE + h2f(4, 0, 0, struct.pack(">HI", 4, 0)))
        get = b"\x82\x84\x86" + b"\x41\x09127.0.0.1"  # GET / http (static 2 = GET)
        for i in range(90):
            s.sendall(h2f(1, 5, 2 * i + 1, b"\x82\x86\x44\x1c/.well-known/agent-card.json" + b"\x41\x09127.0.0.1"))
        fr = h2_drain(s, 3.0)
        s.close()
        return {"frames_seen": len(fr)}
    verdict(t, r, "HTTP/2 zero flow-control window with 90 open streams, held then dropped", zero_window)


# ── WebSocket ────────────────────────────────────────────────────────────────

def ws_open(t, extra=""):
    s = socket.create_connection(("127.0.0.1", t.port), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
               f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nA2A-Version: 1.0\r\n{extra}\r\n").encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        d = s.recv(4096)
        if not d:
            break
        buf += d
    return s, buf


def wsf(opcode, payload=b"", fin=True, mask=True, rsv=0, length=None):
    b0 = (0x80 if fin else 0) | (rsv << 4) | opcode
    n = len(payload) if length is None else length
    if n < 126:
        h = bytes([b0, (0x80 if mask else 0) | n])
    elif n < 65536:
        h = bytes([b0, (0x80 if mask else 0) | 126]) + struct.pack(">H", n)
    else:
        h = bytes([b0, (0x80 if mask else 0) | 127]) + struct.pack(">Q", n)
    if not mask:
        return h + payload
    m = os.urandom(4)
    return h + m + bytes(c ^ m[i % 4] for i, c in enumerate(payload))


def ws_read(s, seconds=2.0):
    """Frames the server sent: list of (opcode, payload[:80])."""
    s.settimeout(0.2)
    end, buf, out = time.time() + seconds, b"", []
    while time.time() < end:
        try:
            d = s.recv(65536)
            if not d:
                out.append(("EOF", b""))
                break
            buf += d
        except socket.timeout:
            continue
        except OSError as e:
            out.append(("ERR", str(e).encode()))
            break
    while len(buf) >= 2:
        op, ln, off = buf[0] & 0x0F, buf[1] & 0x7F, 2
        if ln == 126:
            if len(buf) < 4:
                break
            ln, off = struct.unpack(">H", buf[2:4])[0], 4
        elif ln == 127:
            if len(buf) < 10:
                break
            ln, off = struct.unpack(">Q", buf[2:10])[0], 10
        if len(buf) < off + ln:
            break
        out.append((op, buf[off:off + min(ln, 80)]))
        buf = buf[off + ln:]
    return out


REQ = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendMessage",
                  "params": {"message": msg("echo:ws")}}).encode()


def closed_or_replied(frames):
    return any(f[0] in ("EOF", "ERR", 8, 1) for f in frames)


def ws(r, t):
    def fragmented():
        s, _ = ws_open(t)
        third = len(REQ) // 3
        s.sendall(wsf(1, REQ[:third], fin=False) + wsf(9, b"ping-in-the-middle")
                  + wsf(0, REQ[third:2 * third], fin=False) + wsf(0, REQ[2 * third:]))
        fr = ws_read(s, 2.0)
        s.close()
        return {"frames": [(f[0], f[1][:40]) for f in fr]}
    verdict(t, r, "WS: a request in 3 fragments with a ping between them is answered", fragmented,
            extra_ok=lambda d: any(f[0] == 1 for f in d["frames"]) and any(f[0] == 10 for f in d["frames"]))

    def huge_frame():
        s, _ = ws_open(t)
        s.sendall(wsf(1, b"", length=16 * 1024 * 1024))  # header only: 16 MB declared
        s.sendall(os.urandom(4))
        try:
            for _ in range(16):
                s.sendall(b"a" * 1024 * 1024)
        except OSError:
            pass
        fr = ws_read(s, 2.0)
        s.close()
        return {"frames": [(f[0], f[1][:40]) for f in fr]}
    verdict(t, r, "WS: one 16 MB frame (limit 4 MiB) is refused with a close", huge_frame, bound_rss_mb=200,
            extra_ok=lambda d: any(f[0] in (8, "EOF", "ERR") for f in d["frames"]))

    def frag_bomb():
        s, _ = ws_open(t)
        sent = 0
        try:
            s.sendall(wsf(1, b"x" * 1024 * 1024, fin=False))
            for _ in range(63):
                s.sendall(wsf(0, b"x" * 1024 * 1024, fin=False))
                sent += 1
        except OSError:
            pass
        fr = ws_read(s, 2.0)
        s.close()
        return {"fragments_accepted": sent + 1, "frames": [(f[0], f[1][:40]) for f in fr]}
    verdict(t, r, "WS: 64 fragments of 1 MB (a 64 MB message; limit 4 MiB) is cut off", frag_bomb, bound_rss_mb=200,
            extra_ok=lambda d: d["fragments_accepted"] < 20)

    def ping_flood():
        s, _ = ws_open(t)
        sent = 0
        try:
            for _ in range(200_000):
                s.sendall(wsf(9, b"p"))
                sent += 1
        except OSError:
            pass
        s.close()
        return {"pings_sent": sent}
    verdict(t, r, "WS: 200 000 pings, never reading the pongs", ping_flood)

    for label, frame, want in (
        ("binary frame carrying JSON-RPC", wsf(2, REQ), None),
        ("unmasked client frame", wsf(1, REQ, mask=False), "close"),
        ("invalid UTF-8 in a text frame", wsf(1, b'{"a":"\xff\xfe"}'), "close"),
        ("reserved bit set", wsf(1, REQ, rsv=4), "close"),
        ("unknown opcode 3", wsf(3, b"x"), "close"),
        ("control frame of 200 bytes", wsf(9, b"x" * 200), "close"),
        ("fragmented control frame", wsf(9, b"x", fin=False), "close"),
        ("continuation with no message", wsf(0, b"x"), "close"),
        ("close frame with a 1-byte payload", wsf(8, b"\x03"), None),
        ("text frame of NUL bytes", wsf(1, b"\x00" * 1000), None),
        ("empty text frame", wsf(1, b""), None),
        ("1000 empty text frames", wsf(1, b"") * 1000, None),
    ):
        def one(frame=frame):
            s, _ = ws_open(t)
            try:
                s.sendall(frame)
            except OSError:
                pass
            fr = ws_read(s, 1.5)
            s.close()
            return {"frames": [(f[0], f[1][:60]) for f in fr]}
        verdict(t, r, f"WS: {label}", one,
                extra_ok=(lambda d: any(f[0] in (8, "EOF", "ERR") for f in d["frames"])) if want == "close" else None)

    def stream_never_read():
        s, _ = ws_open(t)
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage",
                           "params": {"message": msg("many:200000")}}).encode()
        s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 2048)
        s.sendall(wsf(1, body))
        time.sleep(15)  # the server writes into a socket nobody reads
        s.close()
        return {}
    verdict(t, r, "WS: a streaming send of 200 000 events whose client never reads (15 s, then drops)",
            stream_never_read, bound_rss_mb=400)


from h6_sse_idle import idle, sse  # noqa: E402

GROUPS = {"h2": h2, "ws": ws, "sse": sse, "idle": idle}

if __name__ == "__main__":
    for g in sys.argv[1:] or list(GROUPS):
        t = Target(f"h6_{g}", {"STORE": "mem"})
        r = Recorder(f"h6_{g}")
        try:
            GROUPS[g](r, t)
        finally:
            r.close()
            t.stop()
