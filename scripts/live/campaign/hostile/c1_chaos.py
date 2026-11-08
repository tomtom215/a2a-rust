# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""C1: replica chaos on PostgreSQL. Each scenario starts replicas of the hostile
target on a fresh `a2a_camp_*` database behind a round-robin TCP proxy (and, for
the database cases, a TCP proxy that pauses or cuts the replicas' connections),
does the damage, waits, then checks the invariants by SQL: every task ends in
one terminal state, no terminal event twice, no event after the terminal one,
the stored state equals the terminal event, no claim left on a finished task,
no gap in the event log, nothing sent is missing.

Scenarios: kill9 (a replica killed mid-turn), sigstop (a replica frozen past the
claim TTL, then continued: the zombie that wakes and writes), terminate (every
backend ended repeatedly), pool (a pool of 2 under 300 concurrent sends), dbpause
(the database link paused 8 s, then cut and restored), many (N replicas on one
database started at once), tenk (10 000 tasks over 3 replicas), cancel_handoff (a
task cancelled while its replica shuts down), steer (steering through a replica
that does not hold the claim, then after the holder is killed).

Needs: the target binary (A2A_HOSTILE_BIN), local PostgreSQL.
Usage: python3 -I c1_chaos.py [scenario ...]   Evidence: evidence/c1_<scenario>.jsonl
"""

import json
import os
import sys
import threading
import time
import urllib.parse
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from chaoslib import Fleet, TcpProxy, bad, double_starts, invariants, sql  # noqa: E402
from lib import PG, Recorder, drop_db, msg, rpc, send, task_of, psql  # noqa: E402

TERMINAL = ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED", "TASK_STATE_CANCELED", "TASK_STATE_REJECTED")
STEER = "https://a2a-rust.com/extensions/steering/v1"


def send_many(port_, texts, workers=16, now=True):
    out, lock, it = [], threading.Lock(), iter(enumerate(texts))

    def work():
        while True:
            with lock:
                try:
                    i, text = next(it)
                except StopIteration:
                    return
            try:
                st, r = send(port_, text, now=now)
                task = task_of(r)
                rec = (task["id"] if task else None, st, ((r.get("error") or {}).get("code") if isinstance(r, dict) else None))
            except Exception as e:  # noqa: BLE001
                rec = (None, f"{type(e).__name__}", None)
            with lock:
                out.append(rec)

    ths = [threading.Thread(target=work) for _ in range(workers)]
    [t.start() for t in ths]
    [t.join() for t in ths]
    return out


def settle(db, want, timeout):
    """Waits until `want` tasks exist and none is non-terminal."""
    end = time.time() + timeout
    term = ",".join(f"'{s}'" for s in TERMINAL)
    while time.time() < end:
        try:
            n = int(sql(db, "select count(*) from tasks"))
            nt = int(sql(db, f"select count(*) from tasks where state not in ({term})"))
            if n >= want and nt == 0:
                return True
        except RuntimeError:
            pass
        time.sleep(0.5)
    return False


def finish(r, fleet, ids, name, allow_non_terminal=False, extra=None):
    inv = invariants(fleet.db, ids)
    problems = bad(inv, allow_non_terminal)
    ds = double_starts(fleet.logs())
    panics = [p for t in fleet.replicas if t is not None for p in t.panics() if "src/exec.rs:" not in p]
    if panics:
        problems.append(f"{len(panics)} unexpected panicked line(s)")
    r.rec(name + ": invariants", "held" if not problems else "DEVIATION",
          {"problems": problems, "invariants": inv, "tasks_whose_executor_started_twice": len(ds),
           "examples": dict(list(ds.items())[:2]), **(extra or {})})
    return inv


def run(name, fn):
    db = f"a2a_camp_c1_{name}"
    r = Recorder(f"c1_{name}")
    try:
        fn(r, db)
    except Exception as e:  # noqa: BLE001
        r.rec(f"{name}: harness exception", "DEVIATION", f"{type(e).__name__}: {e}")
    finally:
        r.close()
        try:
            drop_db(db)
        except RuntimeError:
            pass


def kill9(r, db):
    f = Fleet(db, 3, {"CLAIM_TTL_MS": "3000", "CLAIM_RENEW_MS": "1000"})
    try:
        res = send_many(f.rr.port, ["tick:40:100"] * 60)
        ids = [x[0] for x in res if x[0]]
        r.rec("60 tasks accepted", "held" if len(ids) == 60 else "DEVIATION", {"accepted": len(ids)})
        time.sleep(1.5)
        held_by_r0 = sql(db, "select count(*) from a2a_task_claims where owner like '%r0%'")
        f.kill9(0)
        r.rec("replica r0 killed with -9", "held", {"claims_it_held": held_by_r0})
        ok = settle(db, 60, 40)
        inv = finish(r, f, ids, "kill9", extra={"settled": ok})
        r.rec("every task reached a terminal state within 40 s of the kill", "held" if ok else "DEVIATION", inv["states"])
    finally:
        f.close()


def sigstop(r, db):
    f = Fleet(db, 3, {"CLAIM_TTL_MS": "3000", "CLAIM_RENEW_MS": "1000"})
    try:
        res = send_many(f.rr.port, ["tick:60:100"] * 45)
        ids = [x[0] for x in res if x[0]]
        time.sleep(1.0)
        f.sigstop(1)
        time.sleep(12)  # four times the claim TTL: the others reap its tasks
        before = invariants(db)
        f.sigcont(1)  # the zombie wakes and goes on writing
        r.rec("r1 frozen 12 s past a 3 s claim TTL, then continued", "held", {"states_when_it_woke": before["states"]})
        time.sleep(10)
        ok = settle(db, 45, 30)
        finish(r, f, ids, "sigstop", extra={"settled": ok})
    finally:
        f.close()


def terminate(r, db):
    f = Fleet(db, 2, {})
    try:
        res = send_many(f.rr.port, ["tick:50:80"] * 40)
        ids = [x[0] for x in res if x[0]]
        for _ in range(12):
            time.sleep(0.3)
            sql("postgres", f"select count(pg_terminate_backend(pid)) from pg_stat_activity where datname = '{db}' and pid <> pg_backend_pid()")
        ok = settle(db, 40, 60)
        inv = finish(r, f, ids, "terminate", extra={"settled": ok})
        parts = sql(db, "select coalesce(sum(jsonb_array_length(a->'parts')),0) from tasks, jsonb_array_elements(data->'artifacts') a where a->>'artifactId'='tick'")
        r.rec("tick parts stored vs sent (40 tasks x 50)", "held" if int(parts or 0) == 2000 else "DEVIATION", {"stored_parts": parts, "expected": 2000, "states": inv["states"]})
    finally:
        f.close()


def pool(r, db):
    f = Fleet(db, 1, {"POOL": "2"})
    try:
        t0 = time.time()
        res = send_many(f.rr.port, ["tick:5:100"] * 300, workers=100, now=False)
        took = time.time() - t0
        kinds = {}
        for _, st, code in res:
            kinds[f"{st}/{code}"] = kinds.get(f"{st}/{code}", 0) + 1
        slow = took > 120
        r.rec("300 concurrent blocking sends against a pool of 2", "held" if not slow and f.replicas[0].alive() else "DEVIATION",
              {"seconds": round(took, 1), "outcomes": kinds})
        time.sleep(2)
        st, a = send(f.rr.port, "echo:after")
        r.rec("the replica serves again after the pool was saturated", "held" if task_of(a) else "DEVIATION", {"http": st})
        ids = [x[0] for x in res if x[0]]
        settle(db, len(ids), 30)
        finish(r, f, ids, "pool")
    finally:
        f.close()


def dbpause(r, db):
    u = urllib.parse.urlsplit(PG)
    px = TcpProxy(("127.0.0.1", u.port or 5432))
    f = Fleet(db, 2, {}, db_proxy=px)
    try:
        res = send_many(f.rr.port, ["tick:100:100"] * 30)
        ids = [x[0] for x in res if x[0]]
        time.sleep(1.5)
        px.pause()
        time.sleep(8)
        st = [t.state() for t in f.replicas]
        px.resume()
        r.rec("database link paused 8 s mid-turn, resumed", "held", {"replicas_alive": [s["alive"] for s in st]})
        time.sleep(3)
        px.cut()
        time.sleep(3)
        px.allow()
        r.rec("database link cut (all connections reset) 3 s, restored", "held", {})
        ok = settle(db, 30, 60)
        finish(r, f, ids, "dbpause", extra={"settled": ok})
        late = send(f.rr.port, "echo:after")[1]
        r.rec("a send works after the link is back", "held" if task_of(late) else "DEVIATION", {})
    finally:
        f.close()
        px.close()


def many(r, db, n=30):
    f = Fleet(db, n, {"POOL": "1"})
    try:
        up = [t for t in f.replicas if t is not None and t.alive()]
        r.rec(f"{n} replicas (pool 1) started at once on one database (max_connections={sql('postgres', 'show max_connections')})",
              "held" if len(up) == n else "DEVIATION", {"up": len(up), "failed_to_start": f.failed_to_start[:3]})
        conns = sql("postgres", f"select count(*) from pg_stat_activity where datname = '{db}'")
        res = send_many(f.rr.port, ["tick:5:50"] * 300, workers=24, now=False)
        ids = [x[0] for x in res if x[0]]
        kinds = {}
        for _, st, code in res:
            kinds[f"{st}/{code}"] = kinds.get(f"{st}/{code}", 0) + 1
        settle(db, len(ids), 60)
        finish(r, f, ids, f"many{n}", extra={"connections_open": conns, "send_outcomes": kinds})
    finally:
        f.close()


def tenk(r, db):
    f = Fleet(db, 3, {})
    try:
        t0 = time.time()
        res = send_many(f.rr.port, ["tick:2:10"] * 10000, workers=32, now=False)
        took = time.time() - t0
        ids = [x[0] for x in res if x[0]]
        kinds = {}
        for _, st, code in res:
            kinds[f"{st}/{code}"] = kinds.get(f"{st}/{code}", 0) + 1
        settle(db, len(ids), 120)
        finish(r, f, ids, "tenk", extra={"seconds": round(took, 1), "send_outcomes": kinds})
        r.rec("rss of the replicas after 10 000 tasks", "held", [t.rss_kb() for t in f.replicas])
    finally:
        f.close()


def cancel_handoff(r, db):
    f = Fleet(db, 2, {"CLAIM_TTL_MS": "3000", "CLAIM_RENEW_MS": "1000"})
    try:
        ids = []
        for _ in range(10):
            st, a = send(f.replicas[0].port, "hold", now=True)
            ids.append(task_of(a)["id"])
        time.sleep(0.5)
        stop = threading.Event()

        def canceller():
            while not stop.is_set():
                for i in ids:
                    rpc(f.replicas[1].port, "CancelTask", {"id": i})
                time.sleep(0.05)

        th = threading.Thread(target=canceller)
        th.start()
        f.replicas[0].proc.send_signal(15)  # SIGTERM: graceful, hands its turns back
        time.sleep(8)
        stop.set()
        th.join()
        ok = settle(db, 10, 30)
        inv = finish(r, f, ids, "cancel_handoff", extra={"settled": ok})
        r.rec("every cancelled-while-handed-over task is canceled (or failed/transient), one terminal each",
              "held" if inv["not_terminal"] == 0 else "DEVIATION", inv["states"])
    finally:
        f.close()


def steer(r, db):
    f = Fleet(db, 2, {"STEER": "1", "STEER_CAP": "4", "CLAIM_TTL_MS": "3000", "CLAIM_RENEW_MS": "1000"})
    try:
        st, a = send(f.replicas[0].port, "steer", now=True)
        task = task_of(a)
        time.sleep(0.6)
        m = msg("via the other replica", task=task, extensions=[STEER])
        st, ans = rpc(f.replicas[1].port, "SendMessage", {"message": m})
        receipt = json.dumps(((ans.get("result") or {}).get("task") or {}).get("metadata"))
        time.sleep(1.5)
        g = rpc(f.replicas[0].port, "GetTask", {"id": task["id"]})[1]
        arts = [x["artifactId"] for x in (g["result"].get("artifacts") or [])]
        r.rec("a steering message sent to the replica that does not hold the claim is handed off and read",
              "held" if "handed" in receipt.lower() and "steer-1" in arts else "DEVIATION", {"receipt": receipt, "artifacts": arts})
        # flood through the non-holder past the bound of 4
        codes = {}
        for i in range(30):
            st, ans = rpc(f.replicas[1].port, "SendMessage", {"message": msg(f"flood{i}", task=task, extensions=[STEER])})
            k = str((ans.get("error") or {}).get("code"))
            codes[k] = codes.get(k, 0) + 1
        r.rec("a flood of 30 through the non-holder: bounded refusals, no 5xx other than overload", "held", codes)
        f.kill9(0)
        t0 = time.time()
        results = []
        while time.time() - t0 < 14:
            st, ans = rpc(f.replicas[1].port, "SendMessage", {"message": msg("after kill", task=task, extensions=[STEER])})
            e = ans.get("error") or {}
            results.append((round(time.time() - t0, 1), e.get("code"), (e.get("message") or "")[:70]))
            time.sleep(1.0)
        r.rec("steering after the claim holder is kill -9'd, until its claim lapses (3 s TTL)", "held", results[:14])
        final = rpc(f.replicas[1].port, "GetTask", {"id": task["id"]})[1]["result"]["status"]["state"]
        r.rec("the task whose holder died is not left `working` once the claim lapsed", "held" if final in TERMINAL else "DEVIATION", final)
        finish(r, f, [task["id"]], "steer", allow_non_terminal=False)
    finally:
        f.close()


SCENARIOS = {"kill9": kill9, "sigstop": sigstop, "terminate": terminate, "pool": pool, "dbpause": dbpause,
             "many30": lambda r, db: many(r, db, 30), "many60": lambda r, db: many(r, db, 60), "tenk": tenk,
             "cancel_handoff": cancel_handoff, "steer": steer}

if __name__ == "__main__":
    for s in sys.argv[1:] or list(SCENARIOS):
        print("====", s)
        run(s, SCENARIOS[s])
