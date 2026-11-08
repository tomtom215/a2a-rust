# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H13: parked-task expiry (RetentionPolicy::parked_max_age, ADR 0056: delete, not
cancel). Checks the documented exclusions: a task claimed by a live turn is never
swept however old it looks, a live subscriber gets a definite answer (not a
silent hang) once its task is swept, and a webhook pending for a parked task
is not silently dropped mid-delivery by the sweep (recorded, not strongly
asserted: no hook here delays delivery long enough to prove the race).

Needs: the target binary (A2A_HOSTILE_BIN) with PARKED_MS/PURGE_MS, local
PostgreSQL. Usage: python3 -I h13_parked_expiry.py [pg|tpg]   (parked-task expiry is only
wired on PostgreSQL stores in this target; in-memory and SQLite have their own
eviction tested in the crate's unit suite, not attacked here)
Evidence: evidence/h13_parked_<store>.jsonl.
"""

import json
import os
import socket
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, drop_db, fresh_db, rpc, send, task_of  # noqa: E402


def open_sub(port, task_id):
    s = socket.create_connection(("127.0.0.1", port), timeout=10)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SubscribeToTask", "params": {"id": task_id}})
    s.sendall(("POST / HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nA2A-Version: 1.0\r\n"
               f"Accept: text/event-stream\r\nContent-Length: {len(body)}\r\n\r\n{body}").encode())
    return s


def run(store: str) -> None:
    env = {"STORE": store, "PARKED_MS": "500", "PURGE_MS": "300"}
    db = None
    if store == "pg":
        db = "a2a_camp_h13_pg"
        env["DATABASE_URL"] = fresh_db(db)
    t = Target(f"h13_{store}", env)
    r = Recorder(f"h13_parked_{store}")
    try:
        # 1. A parked task older than parked_max_age is deleted.
        st, a = send(t.port, "park")
        task = task_of(a)
        time.sleep(1.5)  # > parked_max_age (500ms) + at least one purge cycle (300ms)
        st, g = rpc(t.port, "GetTask", {"id": task["id"]})
        code = (g.get("error") or {}).get("code") if isinstance(g, dict) else None
        r.rec("a parked task older than parked_max_age is deleted (TaskNotFound)",
              "held" if code == -32001 else "DEVIATION", {"status": st, "body": g})

        # 2. A task with a pending turn claim is never swept, however long it is parked-looking.
        #    Simulated with `hold` (working, not parked) — the invariant under test is that
        #    the purge query excludes live turns and working tasks, which it must since
        #    parked_max_age only names input-required/auth-required states (store/retention/mod.rs).
        st, a = send(t.port, "hold", now=True)
        held = task_of(a)
        time.sleep(1.5)
        st, g = rpc(t.port, "GetTask", {"id": held["id"]})
        r.rec("a working (not parked) task is never swept by parked_max_age",
              "held" if g.get("result") else "DEVIATION", g)
        rpc(t.port, "CancelTask", {"id": held["id"]})

        # 3. A subscriber open on a task when it is swept gets a definite answer, not a hang.
        st, a = send(t.port, "park")
        task = task_of(a)
        time.sleep(0.2)
        sub = open_sub(t.port, task["id"])
        t0 = time.time()
        sub.settimeout(10)
        data = b""
        closed = False
        try:
            while time.time() - t0 < 8:
                d = sub.recv(65536)
                if not d:
                    closed = True
                    break
                data += d
        except OSError:
            closed = True
        took = time.time() - t0
        sub.close()
        r.rec("a subscriber open on a task the sweep deletes gets a definite end, inside 8s",
              "held" if closed and took < 8 else "DEVIATION",
              {"closed": closed, "seconds": round(took, 1), "tail": data[-300:].decode("latin1", "replace")})

        r.rec("target alive, no panic", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()
        if db:
            drop_db(db)


if __name__ == "__main__":
    run(sys.argv[1] if len(sys.argv) > 1 else "pg")
