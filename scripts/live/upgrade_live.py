#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Rolling upgrade, live: a v0.15.0 replica (old) and a replica of this
checkout (new) on one database. Prints PASS or FAIL per scenario.
Usage: upgrade_live.py OLD_REPLICA NEW_REPLICA [--no-guard]
--no-guard disables the upgrade guard's triggers, as a control run."""
import sys, time
import common

OLD, NEW = sys.argv[1], sys.argv[2]
NO_GUARD = "--no-guard" in sys.argv
DB = "a2a_upgrade_live"
URL = common.fresh_db(DB)
logs = {}
def psql(db, sql): return common.psql(sql, db)
def start(binary, name, port):
    logs[name] = common.OUT / f"upgrade-{name}.log"
    return common.start(binary, name, port, URL, logs[name])
rpc, send = common.rpc, common.send
def get(task): return rpc(N_PORT, "GetTask", {"id": task["id"]})["result"]
def arts(t): return sorted(a["artifactId"] for a in (t.get("artifacts") or []))
O_PORT, N_PORT = 7801, 7802
results = []
def check(name, ok, detail=""): results.append((name, ok)); print(f"{'PASS' if ok else 'FAIL'}  {name}  {detail}")

o = start(OLD, "old", O_PORT)            # the fleet before the upgrade
n = start(NEW, "new", N_PORT)            # the first upgraded replica joins
print("triggers:", psql(DB, "SELECT tgrelid::regclass||'.'||tgname FROM pg_trigger WHERE tgname LIKE 'a2a_legacy%' ORDER BY 1").replace("\n", ", "))
if NO_GUARD:
    for t in ("tasks", "task_events"):
        psql(DB, f"ALTER TABLE {t} DISABLE TRIGGER a2a_legacy_write_check; ALTER TABLE {t} DISABLE TRIGGER a2a_legacy_write_record")
    print("guard DISABLED (control run)")

# S1: N runs a task; a continuation reaches O, which admits it (0.15 cannot see N's turn).
t1, err = send(N_PORT, "hold", now=True)
_, err = send(O_PORT, "park", t1)
time.sleep(1.0)
s = get(t1)
check("S1 old replica's turn cannot write a task the new replica runs",
      "old-early" not in arts(s) and s["status"]["state"] == "TASK_STATE_WORKING",
      f"old answered {('error ' + str(err.get('code')) + ' ' + err.get('message','')[:90]) if err else 'ok'}; stored state {s['status']['state']}, artifacts {arts(s)}")

# S2: a cancel sent to O stops N's executor.
c = rpc(O_PORT, "CancelTask", {"id": t1["id"]})
t0 = time.time(); stopped = None
while time.time() - t0 < 10:
    if "hold ended, cancelled=true" in open(logs["new"]).read(): stopped = time.time() - t0; break
    time.sleep(0.05)
check("S2 a cancel sent to the old replica stops the new replica's executor",
      stopped is not None and get(t1)["status"]["state"] == "TASK_STATE_CANCELED",
      f"old answered {c.get('result',{}).get('status',{}).get('state') or c.get('error')}; executor stopped after {stopped and round(stopped, 2)} s")

# S3: O runs a task; a continuation on N waits for that turn and is refused.
t2, err = send(O_PORT, "hold", now=True)
t0 = time.time(); _, err = send(N_PORT, "more", t2); waited = time.time() - t0
check("S3 the new replica refuses a continuation while the old replica runs the task",
      bool(err) and "already being processed" in err.get("message", ""),
      f"after {waited:.2f} s: {err and err.get('message','')[:80]}")
rpc(O_PORT, "CancelTask", {"id": t2["id"]})

# S4: O parks a task; N continues it and keeps what O stored.
t3, err = send(O_PORT, "park")
t0 = time.time(); t3b, err = send(N_PORT, "finish", t3); took = time.time() - t0
s = get(t3)
check("S4 a task the old replica parked continues on the new replica, keeping its artifacts",
      not err and s["status"]["state"] == "TASK_STATE_COMPLETED" and {"old-early", "old-late"} <= set(arts(s)),
      f"in {took:.2f} s; state {s['status']['state']}, artifacts {arts(s)}")

print("legacy claims left:", psql(DB, "SELECT count(*) FROM a2a_task_claims WHERE scope = 'a2a.legacy'"))
for p in (o, n): p.send_signal(2)
for p in (o, n): p.wait(timeout=20)
print(f"{sum(ok for _, ok in results)} of {len(results)} passed")
