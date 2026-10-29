#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Round trips one send and one read of a task make, through pgcount.py.

Starts the proxy on 15440 in front of the database, a replica behind it,
warms it with three pairs, then records one SendMessage and the GetTask after
it between two marks. Prints the round trips each made and their statements.

Usage: rt_measure.py REPLICA LABEL [new|continue]
  new       the send creates its task
  continue  the send continues a task parked at input-required
PG_LAT_MS (the proxy's) adds latency to each server reply.
Writes $A2A_LIVE_OUT/rt-LABEL.jsonl, which rt_cat.py summarises.
"""
import json, signal, subprocess, sys, time
import common

BIN, LABEL = sys.argv[1], sys.argv[2]
MODE = sys.argv[3] if len(sys.argv) > 3 else "new"
DB, PORT, PROXY = "a2a_rt", 7850, 15440
common.fresh_db(DB)
upstream = common.PG.rsplit(":", 1)[1]
out = common.OUT / f"rt-{LABEL}.jsonl"
proxy = subprocess.Popen([sys.executable, "-I", str(common.ROOT / "scripts/live/pgcount.py"),
                          str(PROXY), upstream, str(out)])
time.sleep(0.5)
url = common.PG.rsplit(":", 1)[0] + f":{PROXY}/{DB}"
rep = common.start(BIN, "rt", PORT, url, common.OUT / "rt-replica.log")
parked = []
def prepare():
    if MODE == "continue":
        parked.append(common.send(PORT, "park")[0])
def pair():
    t, _ = common.send(PORT, "finish", parked.pop() if MODE == "continue" else None)
    send_done = time.time()
    common.rpc(PORT, "GetTask", {"id": t["id"]})
    return send_done
for _ in range(3):
    prepare(); pair()
prepare(); time.sleep(1.5)
proxy.send_signal(signal.SIGUSR1); time.sleep(0.05)
start = time.time(); send_done = pair(); end = time.time()
time.sleep(0.3); proxy.send_signal(signal.SIGUSR1); time.sleep(0.2)
rep.send_signal(signal.SIGINT); rep.wait(timeout=20); proxy.kill()
recs = [json.loads(l) for l in open(out)]
marks = [i for i, r in enumerate(recs) if "mark" in r]
window = recs[marks[0] + 1:marks[1]]
send = [r for r in window if r["t"] <= send_done]; get = [r for r in window if r["t"] > send_done]
print(f"{LABEL}: send {len(send)} round trips, get {len(get)}, total {len(window)} (wall {1000*(end-start):.1f} ms)")
for name, part in (("SEND", send), ("GET", get)):
    print(f"--- {name}")
    for r in part:
        print(f"  c{r['conn']:<2} {r['msgs']:<8} {' | '.join(r['stmts']) or '-'}"[:200])
