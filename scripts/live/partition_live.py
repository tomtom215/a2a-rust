#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Two replicas, each reaching PostgreSQL through its own toxiproxy link: N1
through pg_a (15432), N2 through pg_b (15433). Cuts N1's link mid-turn for
5 s and for 20 s (under and past the claim's 15 s time to live), and
restarts the database mid-turn. Prints PASS or FAIL per scenario.
Needs toxiproxy's API on 127.0.0.1:8474 (README.md). Usage: partition_live.py REPLICA"""
import json, subprocess, sys, time, urllib.request
import common

NEW = sys.argv[1]
DB = "a2a_partition_live"
common.fresh_db(DB)
def tox(path, body=None):
    req = urllib.request.Request("http://127.0.0.1:8474" + path, json.dumps(body).encode() if body else None,
                                 {"content-type": "application/json"})
    with urllib.request.urlopen(req) as r: return r.read()
def link(name, on): tox(f"/proxies/{name}", {"enabled": on})
logs = {}
def start(name, port, proxy_port):
    logs[name] = common.OUT / f"part-{name}.log"
    url = common.PG.rsplit(":", 1)[0] + f":{proxy_port}/{DB}"
    return common.start(NEW, name, port, url, logs[name])
rpc, send = common.rpc, common.send
def stored(task):
    row = common.psql(f"SELECT state, data FROM tasks WHERE id = '{task['id']}'", DB)
    state, data = row.split("|", 1)
    parts = [len(a["parts"]) for a in json.loads(data).get("artifacts") or [] if a["artifactId"] == "tick"]
    return state, (parts[0] if parts else 0)
def logged(name, needle): return needle in open(logs[name]).read()
N1, N2 = 7811, 7812
n1 = start("n1", N1, 15432); n2 = start("n2", N2, 15433)
results = []
def check(name, ok, detail): results.append(ok); print(f"{'PASS' if ok else 'FAIL'}  {name}\n      {detail}")

# P1: N1's link down 5 s, under the time to live.
t, _ = send(N1, "tick", now=True); time.sleep(2)
before = stored(t); link("pg_a", False); t0 = time.time()
_, err = send(N2, "more", t); refused_at = time.time() - t0
time.sleep(max(0, 5 - (time.time() - t0))); link("pg_a", True); time.sleep(4)
after = stored(t)
check("P1 a 5 s partition of the running replica: the task stays its own and continues",
      err is not None and "already being processed" in err["message"] and after[1] > before[1] and after[0] == "TASK_STATE_WORKING",
      f"N2's continuation during the partition: {err and err['message'][:60]!r} after {refused_at:.1f} s; "
      f"tick parts {before[1]} before, {after[1]} 4 s after the link came back; state {after[0]}")
c = rpc(N2, "CancelTask", {"id": t["id"]}); t0 = time.time()
while time.time() - t0 < 10 and not logged("n1", "cancelled"): time.sleep(0.05)
check("P1b and a cancel on the other replica still stops it", logged("n1", "cancelled"),
      f"stopped {time.time() - t0:.2f} s after the cancel; state {stored(t)[0]}")

# P2: N1's link down 20 s, past the time to live: N2 takes the task over.
t, _ = send(N1, "tick", now=True); time.sleep(2)
link("pg_a", False); cut = time.time()
time.sleep(16)
done, err = send(N2, "finish", t); took_over = time.time() - cut
parts_at_takeover = stored(t)[1]
stops = open(logs["n1"]).read().count("cancelled")
time.sleep(max(0, 20 - (time.time() - cut))); link("pg_a", True); back = time.time()
stopped = None
while time.time() - back < 10 and stopped is None:
    if open(logs["n1"]).read().count("cancelled") > stops: stopped = time.time() - back
    time.sleep(0.05)
time.sleep(max(0, 5 - (time.time() - back)))
final = stored(t)
check("P2 a 20 s partition: the other replica takes the task over after the time to live, and the cut-off replica writes nothing more",
      err is None and final[0] == "TASK_STATE_COMPLETED" and final[1] == parts_at_takeover and stopped is not None,
      f"N2 ran the continuation {took_over:.1f} s after the cut ({err or done['status']['state']}); "
      f"tick parts {parts_at_takeover} at takeover, {final[1]} 5 s after N1's link came back; final state {final[0]}; "
      f"N1's executor stopped {f'{stopped:.1f} s' if stopped is not None else 'not within 10 s'} after its link came back")

# P3: the database restarts mid-turn.
t, _ = send(N1, "tick", now=True); time.sleep(2)
before = stored(t); subprocess.run(["docker", "restart", "-t", "2", common.CONTAINER], capture_output=True)
common.wait_ready()
time.sleep(5); after = stored(t)
check("P3 a database restart mid-turn: the turn resumes writing once it is back",
      after[1] > before[1] and after[0] == "TASK_STATE_WORKING",
      f"tick parts {before[1]} before the restart, {after[1]} 5 s after it was ready; state {after[0]}")
rpc(N1, "CancelTask", {"id": t["id"]})
for p in (n1, n2): p.send_signal(2)
for p in (n1, n2): p.wait(timeout=30)
print(f"{sum(results)} of {len(results)} passed")
