# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H11: JSON-RPC notifications (a request with no `id`) and batches. A notification
gets no reply (HTTP 204), so a caller cannot see whether it was authenticated,
authorized or run. Does an UNAUTHENTICATED notification, or one by the wrong
caller, execute? With bearer authentication and caller isolation on, an
unauthenticated `SendMessage` notification must start nothing, an
unauthenticated `CancelTask` notification must cancel nothing, and bob's
notification must not touch alice's task. The same through a batch.

Needs: the target binary (A2A_HOSTILE_BIN). Evidence: evidence/h11_notifications.jsonl.
"""

import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import BASE_HEADERS, Recorder, Target, http, msg, rpc, send, task_of  # noqa: E402


def post(t, body, token=None):
    hs = dict(BASE_HEADERS)
    if token:
        hs["authorization"] = f"Bearer {token}"
    return http(t.port, "POST", "/", body if isinstance(body, str) else json.dumps(body), hs)


def count(t, token="tokA"):
    st, r = rpc(t.port, "ListTasks", {"pageSize": 100}, token=token)
    return len((r.get("result") or {}).get("tasks") or [])


def main() -> None:
    t = Target("h11_notifications", {"STORE": "tmem", "AUTH": "bearer", "STEER": "1"})
    r = Recorder("h11_notifications")
    try:
        n0 = count(t)
        note = {"jsonrpc": "2.0", "method": "SendMessage", "params": {"message": msg("echo:note")}}
        st, _, d = post(t, note)
        time.sleep(0.5)
        r.rec("unauthenticated SendMessage notification starts nothing", "held" if count(t) == n0 else "DEVIATION",
              {"status": st, "tasks_before": n0, "tasks_after": count(t)})
        st, _, d = post(t, note, "bad-token")
        time.sleep(0.5)
        r.rec("SendMessage notification with a wrong token starts nothing", "held" if count(t) == n0 else "DEVIATION",
              {"status": st, "tasks_after": count(t)})
        st, _, d = post(t, note, "tokA")
        time.sleep(0.5)
        r.rec("authenticated SendMessage notification (recorded: does a valid notification run?)", "held",
              {"status": st, "tasks_after": count(t), "ran": count(t) > n0})
        # a held task of alice's
        st, a = send(t.port, "hold", token="tokA", now=True)
        task = task_of(a)
        time.sleep(0.3)
        cancel = {"jsonrpc": "2.0", "method": "CancelTask", "params": {"id": task["id"]}}
        for label, tok in (("no credential", None), ("a wrong token", "bad"), ("bob's token", "tokB")):
            post(t, cancel, tok)
            time.sleep(0.4)
            g = rpc(t.port, "GetTask", {"id": task["id"]}, token="tokA")[1]
            st_ = g["result"]["status"]["state"]
            r.rec(f"CancelTask notification with {label} leaves alice's task alone", "held" if st_ == "TASK_STATE_WORKING" else "DEVIATION", st_)
        # batches: a mix of notification and request
        batch = [{"jsonrpc": "2.0", "method": "CancelTask", "params": {"id": task["id"]}},
                 {"jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {"id": task["id"]}}]
        st, hd, d = post(t, batch)
        time.sleep(0.4)
        g = rpc(t.port, "GetTask", {"id": task["id"]}, token="tokA")[1]
        r.rec("unauthenticated batch [CancelTask notification, GetTask] cancels nothing and leaks nothing",
              "held" if g["result"]["status"]["state"] == "TASK_STATE_WORKING" and task["id"] not in d.decode() else "DEVIATION",
              {"status": st, "www-authenticate": hd.get("www-authenticate"), "body": d[:200].decode("latin1")})
        # bob's authenticated batch against alice's task
        st, hd, d = post(t, batch, "tokB")
        time.sleep(0.4)
        g = rpc(t.port, "GetTask", {"id": task["id"]}, token="tokA")[1]
        r.rec("bob's batch [CancelTask notification, GetTask] on alice's task does nothing",
              "held" if g["result"]["status"]["state"] == "TASK_STATE_WORKING" else "DEVIATION",
              {"status": st, "body": d[:240].decode("latin1")})
        # steering by notification
        steer = {"jsonrpc": "2.0", "method": "SendMessage", "params": {"message": {
            **msg("steered by notification", task=task, extensions=["https://a2a-rust.com/extensions/steering/v1"])}}}
        post(t, steer, "tokB")
        post(t, steer)
        r.rec("target alive", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
        # a batch of 1000 authenticated GetTask: bounded?
        big = [{"jsonrpc": "2.0", "id": i, "method": "GetTask", "params": {"id": task["id"]}} for i in range(1000)]
        t0 = time.time()
        st, hd, d = post(t, big, "tokA")
        r.rec("a batch of 1000 authenticated requests", "held" if st in (200, 400, 413) else "DEVIATION",
              {"status": st, "seconds": round(time.time() - t0, 2), "bytes": len(d)})
        t0 = time.time()
        big = [{"jsonrpc": "2.0", "id": i, "method": "SendMessage", "params": {"message": msg("echo:b")}} for i in range(200)]
        st, hd, d = post(t, big, "tokA")
        r.rec("a batch of 200 SendMessage (does one request bypass the per-request limits?)", "held" if st in (200, 400, 413) else "DEVIATION",
              {"status": st, "seconds": round(time.time() - t0, 2), "tasks_now": count(t)})
    finally:
        r.close()
        t.stop()


if __name__ == "__main__":
    main()
