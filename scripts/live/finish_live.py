#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A cancel taken on another replica, then the owner killed with SIGKILL.

Two replicas share one PostgreSQL database. A task is held on replica A;
`CancelTask` goes to replica B, which stores `canceled` and leaves the ending
to A; A is killed before it ends the turn. Reports, 1 s after the kill, what
the store holds (state, the event log's ending, the claim row) and then waits
for the survivor's reaper, reporting when the log gained its ending and the
claim went. Before the reaper's finish phase (hostile campaign W2-hostile-3)
the log never gained it and the claim stayed until the sweep.

Usage: finish_live.py REPLICA LABEL [MODE] [WAIT_SECONDS]
MODE `hold` (default) holds the task until cancelled; `ask` has the owner call
a real model (llama.cpp `llama-server` at $C_MODEL_URL, see replica/src/main.rs)
so the owner dies in the middle of a generation.
Exit status: 0 the ending was logged once and the claim released within the
wait (default 120 s); 1 otherwise. Needs PostgreSQL as in README.md (set
A2A_LIVE_LOCAL=1 to use a local `psql` and no container)."""
import sys, time
import common

BIN, LABEL = sys.argv[1], sys.argv[2]
MODE = sys.argv[3] if len(sys.argv) > 3 else "hold"
WAIT = float(sys.argv[4]) if len(sys.argv) > 4 else 120.0
PROMPT = {"hold": "hold",
          "ask": "ask Explain, in detail, how a tidal lock arises and why the Moon always shows one face."}[MODE]
DB, PORT_A, PORT_B = "a2a_finish", 7870, 7871

common.wait_ready()
url = common.fresh_db(DB)
a = common.start(BIN, "A", PORT_A, url, common.OUT / f"finish-{LABEL}-a.log", "warn")
b = common.start(BIN, "B", PORT_B, url, common.OUT / f"finish-{LABEL}-b.log", "warn")
try:
    task, err = common.send(PORT_A, PROMPT, now=True)
    assert task, err
    tid = task["id"]
    for _ in range(100):  # A takes the turn's claim before it runs the executor
        if common.psql(f"SELECT count(*) FROM a2a_task_claims WHERE task_id = '{tid}'", DB) == "1":
            break
        time.sleep(0.05)
    if MODE == "ask":
        # The owner is in the middle of a generation when it is killed: the model
        # server says a slot is processing, read just before the cancel.
        import json, urllib.request
        busy = False
        for _ in range(60):
            slots = json.load(urllib.request.urlopen("http://127.0.0.1:11434/slots", timeout=5))
            busy = any(sl.get("is_processing") for sl in slots)
            if busy:
                break
            time.sleep(0.5)
        print(f"{LABEL} (ask): model slot processing before the cancel: {busy}")
        assert busy, "the model never started generating"
        time.sleep(2)
    canceled = common.rpc(PORT_B, "CancelTask", {"id": tid})
    a.kill()  # SIGKILL: A's next renewal, at which it would learn of the cancel, never comes
    a.wait()
    time.sleep(1)

    def endings() -> int:
        return int(common.psql(
            f"SELECT count(*) FROM task_events WHERE task_id = '{tid}' AND payload::text LIKE '%TASK_STATE_CANCELED%'", DB) or 0)

    def claims() -> int:
        return int(common.psql(f"SELECT count(*) FROM a2a_task_claims WHERE task_id = '{tid}'", DB) or 0)

    state = common.psql(f"SELECT state FROM tasks WHERE id = '{tid}'", DB)
    print(f"{LABEL} ({MODE}): after the kill: CancelTask answered "
          f"{'ok' if 'result' in canceled else canceled.get('error')}; stored state {state}; "
          f"events carrying the ending: {endings()}; claim rows: {claims()}")
    started = time.time()
    done = None
    while time.time() - started < WAIT:
        if endings() >= 1 and claims() == 0:
            done = time.time() - started
            break
        time.sleep(1)
    time.sleep(3)  # a duplicate would land here
    n, c = endings(), claims()
    print(f"{LABEL}: " + (f"after {done:.0f} s the log carries the ending and the claim is gone"
                          if done is not None else f"after {WAIT:.0f} s the ending is still missing or the claim stayed")
          + f"; final: events carrying the ending {n}, claim rows {c}")
    sys.exit(0 if done is not None and n == 1 and c == 0 else 1)
finally:
    for p in (a, b):
        if p.poll() is None:
            p.terminate()
            p.wait(timeout=20)
