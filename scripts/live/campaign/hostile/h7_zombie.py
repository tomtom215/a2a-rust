# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H7: a task whose executor returned Ok(()) with no terminal state (the
contract the executor guide documents: the task stays `working`, a warning is
logged). What does that leave behind? How long does a `SubscribeToTask` stream
on it stay open, does the subscriber cap protect other tasks, does the task
ever end, can it be cancelled, and is it ever swept?

Needs: the target binary (A2A_HOSTILE_BIN). Usage: python3 -I h7_zombie.py [seconds]
(default 90 s observation). Writes evidence/h7_zombie.jsonl.
"""

import json
import os
import socket
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, msg, rpc, send, task_of  # noqa: E402


def open_sub(port, task_id):
    s = socket.create_connection(("127.0.0.1", port), timeout=10)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SubscribeToTask", "params": {"id": task_id}})
    s.sendall(("POST / HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nA2A-Version: 1.0\r\n"
               f"Accept: text/event-stream\r\nContent-Length: {len(body)}\r\n\r\n{body}").encode())
    return s


def main() -> None:
    watch = float(sys.argv[1]) if len(sys.argv) > 1 else 90.0
    t = Target("h7_zombie", {"STORE": "mem"})
    r = Recorder("h7_zombie")
    try:
        st, ans = send(t.port, "noterm:1", now=True)
        task = task_of(ans)
        time.sleep(1.0)
        st, g = rpc(t.port, "GetTask", {"id": task["id"]})
        state0 = g["result"]["status"]["state"]
        r.rec("executor returned Ok with no terminal: stored state", "held", {"state": state0})
        # 50 subscribers on the zombie
        subs = [open_sub(t.port, task["id"]) for _ in range(50)]
        time.sleep(1)
        first = [s.recv(400) for s in subs[:1]][0]
        r.rec("a SubscribeToTask stream on the zombie opens", "held", first[:200].decode("latin1"))
        t0 = time.time()
        closed = {}
        while time.time() - t0 < watch and len(closed) < len(subs):
            for i, s in enumerate(subs):
                if i in closed:
                    continue
                s.setblocking(False)
                try:
                    d = s.recv(65536)
                    if d == b"":
                        closed[i] = round(time.time() - t0, 1)
                    elif b"TASK_STATE_COMPLETED" in d or b"TASK_STATE_FAILED" in d or b"TASK_STATE_CANCELED" in d:
                        closed[i] = round(time.time() - t0, 1)
                except BlockingIOError:
                    pass
                except OSError:
                    closed[i] = round(time.time() - t0, 1)
            time.sleep(1)
        # The SSE keep-alive comment (DispatchConfig::sse_keep_alive_interval,
        # 30s) is specifically what keeps a quiet-but-open stream from ever
        # tripping the idle timeout (serve/graceful/config.rs:33-44's own
        # reasoning): a zombie task's subscribers are expected to stay open
        # indefinitely. What bounds them is the subscriber cap, checked next,
        # not a timer. Recorded as data, not a pass/fail: 0 released here is
        # the documented behaviour.
        r.rec(f"zombie subscribers' connections after {watch:.0f}s (documented: SSE keep-alive "
              "keeps a quiet stream open; the cap below is the real bound, not a timer)", "held",
              {"released": len(closed), "of": len(subs), "after_s": sorted(set(closed.values()))[:5]})
        st, g = rpc(t.port, "GetTask", {"id": task["id"]})
        r.rec("the zombie is still `working` after the observation", "held" if g["result"]["status"]["state"] == "TASK_STATE_WORKING" else "DEVIATION",
              g["result"]["status"])
        # the cap: fill the per-task cap of 256 with held streams, then one more
        more = []
        refused = None
        for i in range(300):
            s = open_sub(t.port, task["id"])
            more.append(s)
        time.sleep(1)
        extra = open_sub(t.port, task["id"])
        time.sleep(0.5)
        extra.settimeout(2)
        try:
            refused = extra.recv(4000).decode("latin1")[-700:]
        except OSError as e:
            refused = repr(e)
        r.rec("the per-task subscriber cap (256) refuses one more", "held" if "503" in (refused or "") or "RetryInfo" in (refused or "") or "Overloaded" in (refused or "") or "retry" in (refused or "").lower() or "limit" in (refused or "") else "DEVIATION", refused)
        # can the zombie be cancelled?
        st, c = rpc(t.port, "CancelTask", {"id": task["id"]})
        r.rec("CancelTask on the zombie", "held" if c.get("result") or c.get("error") else "DEVIATION", json.dumps(c)[:200])
        time.sleep(1)
        st, g = rpc(t.port, "GetTask", {"id": task["id"]})
        r.rec("zombie after cancel", "held" if g["result"]["status"]["state"] == "TASK_STATE_CANCELED" else "DEVIATION",
              g["result"]["status"]["state"])
        for s in subs + more + [extra]:
            s.close()
        r.rec("target alive", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()


if __name__ == "__main__":
    main()
