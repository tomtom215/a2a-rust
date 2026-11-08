# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H3: steering (`with_steering`, extension https://a2a-rust.com/extensions/steering/v1)
attacked by a caller who does not own the task, across tenants, past the inbox
bound, in terminal states, racing the terminal transition, with duplicate and
oversized messages, and with the extension undeclared. One process, one store.

Needs: the target binary (A2A_HOSTILE_BIN); for `pg`, local PostgreSQL.
Usage: python3 -I h3_steering.py [mem|pg]   (mem = TenantAwareInMemoryTaskStore, pg = TenantAwarePostgresTaskStore;
isolate_tasks_by_caller needs a tenant-aware store)
Writes evidence/h3_<store>.jsonl. The multi-replica cases (claim holder killed,
claim lapse) are in c1_chaos.py.
"""

import json
import os
import sys
import threading
import time
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, drop_db, fresh_db, msg, rpc, send, task_of  # noqa: E402

STEER = "https://a2a-rust.com/extensions/steering/v1"
KEY = "a2a-rust.com/idempotency-key"
IDEM = "https://a2a-rust.com/extensions/idempotency/v1"
ALICE, BOB = "tokA", "tokB"


def state(t, task_id, token):
    st, r = rpc(t.port, "GetTask", {"id": task_id}, token=token)
    try:
        return r["result"]["status"]["state"]
    except (KeyError, TypeError):
        return None


def start_steer_task(t, token=ALICE, ctx=None, extra=None):
    st, r = send(t.port, "steer", token=token, now=True, ctx=ctx, extra=extra)
    task = task_of(r)
    assert task, r
    for _ in range(100):
        if state(t, task["id"], token) == "TASK_STATE_WORKING":
            return task
        time.sleep(0.05)
    raise RuntimeError("task never reached working")


def steer(t, task, text, token=ALICE, ext=True, mid=None, extra=None, metadata=None):
    m = msg(text, task=task, mid=mid, extensions=[STEER] if ext else None)
    if metadata:
        m["metadata"] = metadata
    st, r = rpc(t.port, "SendMessage", {"message": m}, token=token, extra=extra)
    return st, r


def code_of(r):
    return ((r or {}).get("error") or {}).get("code") if isinstance(r, dict) else None


def strip_ids(r) -> str:
    """The answer with every uuid and the JSON-RPC id removed, for comparison."""
    import re
    s = json.dumps(r, sort_keys=True)
    return re.sub(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|reference [0-9a-f]+",
                  "<id>", s)


def run(store: str) -> None:
    env = {"STORE": "tpg" if store == "pg" else "tmem", "AUTH": "bearer", "ISOLATE": "1", "STEER": "1", "STEER_CAP": "4",
           "STEER_MAX": "2048", "TENANT": "none"}
    db = None
    if store == "pg":
        db = "a2a_camp_h3_pg"
        env["DATABASE_URL"] = fresh_db(db)
    t = Target(f"h3_{store}", env)
    r = Recorder(f"h3_{store}")
    try:
        # 1. baseline: the owner steers and the executor sees it.
        task = start_steer_task(t)
        st, ans = steer(t, task, "hello")
        rec = (((ans.get("result") or {}).get("task") or {}).get("metadata") or {}) if isinstance(ans, dict) else {}
        r.rec("baseline: owner steers, receipt returned",
              "held" if st == 200 and task_of(ans) and "a2a-rust.com/steering" in json.dumps(rec) else "DEVIATION",
              {"http": st, "receipt": rec})
        time.sleep(0.3)
        st, g = rpc(t.port, "GetTask", {"id": task["id"]}, token=ALICE)
        arts = [a["artifactId"] for a in (g["result"].get("artifacts") or [])]
        r.rec("baseline: executor read the message", "held" if "steer-1" in arts else "DEVIATION", arts)

        # 2. a caller who does not own the task: byte-for-byte as "not found".
        st_b, ans_b = steer(t, task, "intruder", token=BOB)
        st_n, ans_n = steer(t, {"id": str(uuid.uuid4()), "contextId": task["contextId"]}, "intruder",
                            token=BOB)
        same = strip_ids(ans_b) == strip_ids(ans_n) and st_b == st_n
        r.rec("bob steers alice's task == bob steers a missing task", "held" if same else "DEVIATION",
              {"bob_vs_alice_task": [st_b, strip_ids(ans_b)[:300]],
               "bob_vs_missing": [st_n, strip_ids(ans_n)[:300]]})
        st, g = rpc(t.port, "GetTask", {"id": task["id"]}, token=ALICE)
        arts = [a["artifactId"] for a in (g["result"].get("artifacts") or [])]
        r.rec("bob's steer was not delivered", "held" if len(arts) == 1 else "DEVIATION", arts)

        # 2b. other operations by bob on alice's task are indistinguishable from missing.
        for op, params in (("GetTask", {"id": task["id"]}), ("CancelTask", {"id": task["id"]}),
                           ("ListTaskPushNotificationConfigs", {"taskId": task["id"]}),
                           ("SubscribeToTask", {"id": task["id"]})):
            miss = dict(params)
            k = "id" if "id" in params else "taskId"
            miss[k] = str(uuid.uuid4())
            s1, a1 = rpc(t.port, op, params, token=BOB, timeout=5)
            s2, a2 = rpc(t.port, op, miss, token=BOB, timeout=5)
            r.rec(f"bob {op} alice's task == missing task",
                  "held" if (s1, strip_ids(a1)) == (s2, strip_ids(a2)) else "DEVIATION",
                  {"alice": [s1, strip_ids(a1)[:200]], "missing": [s2, strip_ids(a2)[:200]]})

        # 3. cross-caller idempotency key and message id.
        key = "k-" + uuid.uuid4().hex
        m = msg("with key", task=task, extensions=[STEER, IDEM])
        m["metadata"] = {KEY: key}
        s1, a1 = rpc(t.port, "SendMessage", {"message": m}, token=ALICE)
        bob_task = start_steer_task(t, token=BOB)
        mb = msg("with alice's key", task=bob_task, extensions=[STEER, IDEM])
        mb["metadata"] = {KEY: key}
        s2, a2 = rpc(t.port, "SendMessage", {"message": mb}, token=BOB)
        text2 = json.dumps(a2)
        leaked = [x for x in (task["id"], m["messageId"]) if x in text2]
        r.rec("bob reuses alice's idempotency key: no id of alice's leaks",
              "held" if not leaked else "DEVIATION",
              {"alice_send": [s1, code_of(a1)], "bob": [s2, text2[:400]], "leaked": leaked})
        # and a duplicate message id across callers
        mid = "dup-" + uuid.uuid4().hex
        s1, a1 = steer(t, task, "dup-a", mid=mid)
        s2, a2 = steer(t, bob_task, "dup-b", token=BOB, mid=mid)
        r.rec("same messageId from two callers is not conflated",
              "held" if code_of(a2) is None and task_of(a2) else "DEVIATION",
              {"alice": [s1, code_of(a1)], "bob": [s2, code_of(a2), json.dumps(a2)[:200]]})
        # the same message id twice by one caller to the same task
        mid = "dup2-" + uuid.uuid4().hex
        s1, a1 = steer(t, bob_task, "dup2", token=BOB, mid=mid)
        s2, a2 = steer(t, bob_task, "dup2", token=BOB, mid=mid)
        r.rec("same messageId twice, same caller (no idempotency key)", "held",
              {"first": [s1, code_of(a1)], "second": [s2, code_of(a2), json.dumps(a2)[:200]]})

        # 4. undeclared extension to a working task.
        s1, a1 = steer(t, task, "no ext", ext=False)
        r.rec("message to a working task without the extension is refused (not queued)",
              "held" if code_of(a1) is not None else "DEVIATION", {"http": s1, "ans": json.dumps(a1)[:300]})
        time.sleep(0.2)
        st, g = rpc(t.port, "GetTask", {"id": task["id"]}, token=ALICE)
        arts = [a["artifactId"] for a in (g["result"].get("artifacts") or [])]
        r.rec("undeclared message was not delivered", "held" if "no ext" not in json.dumps(g) else "DEVIATION", arts)

        # 5. oversized steering messages (limit 2048 serialized bytes).
        for n in (1500, 2100, 70000, 5_000_000):
            s1, a1 = steer(t, task, "x" * n)
            ok = (n < 1700 and code_of(a1) is None) or (n > 1900 and (s1 in (413, 200) and code_of(a1) is not None))
            r.rec(f"steer message of {n} bytes", "held" if ok else "DEVIATION",
                  {"http": s1, "code": code_of(a1), "msg": json.dumps(a1)[:160]})

        # 6. flood past the inbox bound: a task whose executor never reads.
        st, rr = send(t.port, "hold", token=ALICE, now=True)
        held = task_of(rr)
        time.sleep(0.2)
        codes = {}
        for i in range(60):
            s1, a1 = steer(t, held, f"flood{i}")
            codes[str((s1, code_of(a1)))] = codes.get(str((s1, code_of(a1))), 0) + 1
        # `hold` does not read the inbox: cap 4 means 4 accepted, the rest refused with 503/-32xxx
        accepted = sum(v for k, v in codes.items() if k.endswith("None)"))
        r.rec("flood of 60 steering messages into an inbox of 4 that is never read",
              "held" if accepted == 4 else "DEVIATION", {"codes": codes, "accepted": accepted})
        rpc(t.port, "CancelTask", {"id": held["id"]}, token=ALICE)

        # 7. steering a task in a terminal state, and a cancelled one.
        s1, rr = send(t.port, "echo:done", token=ALICE)
        fin = task_of(rr)
        s2, a2 = steer(t, fin, "late")
        r.rec("steer a completed task is refused", "held" if code_of(a2) is not None else "DEVIATION",
              {"http": s2, "ans": json.dumps(a2)[:200]})
        st, g = rpc(t.port, "GetTask", {"id": held["id"]}, token=ALICE)
        s3, a3 = steer(t, held, "late")
        r.rec("steer a cancelled task is refused", "held" if code_of(a3) is not None else "DEVIATION",
              {"http": s3, "ans": json.dumps(a3)[:200], "state": g["result"]["status"]["state"]})

        # 8. race the terminal transition: 40 tasks each steered "done" and then hammered.
        outcomes = {}
        lock = threading.Lock()

        def racer(i):
            tk = start_steer_task(t)
            res = []
            for j in range(6):
                s, a = steer(t, tk, "done" if j == 0 else f"after{j}")
                res.append((s, code_of(a)))
            with lock:
                k = json.dumps(res)
                outcomes[k] = outcomes.get(k, 0) + 1
            # the task must end completed exactly once
            for _ in range(40):
                if state(t, tk["id"], ALICE) == "TASK_STATE_COMPLETED":
                    break
                time.sleep(0.05)
            with lock:
                outcomes["final=" + str(state(t, tk["id"], ALICE))] = outcomes.get("final=" + str(state(t, tk["id"], ALICE)), 0) + 1

        ths = [threading.Thread(target=racer, args=(i,)) for i in range(20)]
        [x.start() for x in ths]
        [x.join() for x in ths]
        finals = {k: v for k, v in outcomes.items() if k.startswith("final=")}
        r.rec("racing the terminal transition: every task ends completed",
              "held" if list(finals) == ["final=TASK_STATE_COMPLETED"] else "DEVIATION",
              {"finals": finals, "send_patterns": {k: v for k, v in outcomes.items() if not k.startswith("final=")}})
        r.rec("target alive, no panic", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()
        if db:
            drop_db(db)


if __name__ == "__main__":
    run(sys.argv[1] if len(sys.argv) > 1 else "mem")
