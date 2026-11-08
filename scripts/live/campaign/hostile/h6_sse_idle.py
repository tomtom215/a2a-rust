# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The SSE-non-reader and idle-connection groups of h6_h2_ws.py, split out so
that file stays under the 500-line ratchet. Imported by h6_h2_ws.py, which is
where the module docstring (what this needs, how to run it) lives; not meant
to be run on its own.
"""

import json
import os
import socket
import time

from h6_common import healthy, verdict
from lib import msg, rpc


# ── SSE readers that never read, idle connections ────────────────────────────

def sse(r, t):
    def nonreaders():
        # the sender is a normal client; the 50 subscribers never read.
        res = {}

        def sender():
            res["send"] = rpc(t.port, "SendMessage", {"message": msg("d1500/many:300000"), "configuration": {"returnImmediately": True}})
        sender()
        tid = res["send"][1]["result"]["task"]["id"]
        socks = []
        for _ in range(50):
            s = socket.create_connection(("127.0.0.1", t.port), timeout=10)
            s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 2048)
            body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SubscribeToTask", "params": {"id": tid}})
            s.sendall(("POST / HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nA2A-Version: 1.0\r\n"
                       f"Accept: text/event-stream\r\nContent-Length: {len(body)}\r\n\r\n{body}").encode())
            socks.append(s)
        done = None
        for _ in range(120):
            st, g = rpc(t.port, "GetTask", {"id": tid})
            done = g.get("result", {}).get("status", {}).get("state") if isinstance(g, dict) else None
            if done == "TASK_STATE_COMPLETED":
                break
            time.sleep(0.5)
        mid = t.rss_kb()
        time.sleep(3)
        for s in socks:
            s.close()
        return {"task_state_while_50_subscribers_never_read": done, "rss_kb_before_close": mid}
    verdict(t, r, "SSE: 50 subscribers never read a 300 000-event task; the task must still finish",
            nonreaders, bound_rss_mb=600, extra_ok=lambda d: d["task_state_while_50_subscribers_never_read"] == "TASK_STATE_COMPLETED")


def idle(r, t):
    def many_idle(n, label):
        socks = []
        refused = 0
        for _ in range(n):
            try:
                socks.append(socket.create_connection(("127.0.0.1", t.port), timeout=5))
            except OSError:
                refused += 1
        peak_fds, peak_rss = t.fds(), t.rss_kb()
        h_during = healthy(t)
        time.sleep(80)  # the 75 s idle timeout (the 30 s header-read timeout starts at the first byte)
        closed = 0
        for s in socks:
            s.setblocking(False)
            try:
                if s.recv(1) == b"":
                    closed += 1
            except BlockingIOError:
                pass
            except OSError:
                closed += 1
        for s in socks:
            s.close()
        return {"opened": len(socks), "refused": refused, "peak_fds": peak_fds, "peak_rss_kb": peak_rss,
                "legit_send_while_open": h_during, "closed_by_server_after_80s": closed}
    verdict(t, r, "10 000 idle connections that send nothing: the server closes them within its 75 s idle bound, serves others, recovers",
            lambda: many_idle(10000, "idle"), bound_rss_mb=500, bound_cpu_s=120,
            extra_ok=lambda d: d["closed_by_server_after_80s"] >= d["opened"] * 0.99 and d["legit_send_while_open"] is True)


