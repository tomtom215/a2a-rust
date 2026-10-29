#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The database stopped for 4 s and started again mid-turn. One replica runs
`tick`; reports gaps in the task's parts and in its event log 6 s after the
server is ready again. Usage: restart_live.py REPLICA LABEL"""
import subprocess, sys, time
import common

BIN, LABEL = sys.argv[1], sys.argv[2]
DB, PORT = "a2a_restart", 7861
rep = common.start(BIN, LABEL, PORT, common.fresh_db(DB), common.OUT / f"restart-{LABEL}.log")
t, _ = common.send(PORT, "tick", now=True)
time.sleep(2.2)
t0 = time.time()
subprocess.run(["docker", "stop", "-t", "2", common.CONTAINER], capture_output=True)
time.sleep(4)
subprocess.run(["docker", "start", common.CONTAINER], capture_output=True)
common.wait_ready()
outage = time.time() - t0
time.sleep(6)
state, parts, seqs = common.stored_ticks(DB, t["id"])
print(f"{LABEL}: restart took {outage:.1f} s; state {state}; parts 0..{max(parts)}, missing {common.missing(parts, 0)}; "
      f"log 1..{max(seqs)}, missing {common.missing(seqs, 1)}")
rep.terminate(); rep.wait(timeout=20)
