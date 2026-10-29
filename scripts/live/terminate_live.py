#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Every backend ended mid-turn (pg_terminate_backend), the database up
throughout. One replica runs `tick` (a part every 500 ms); 2.2 s in, every
connection to its database is terminated. Reports which parts reached the
row 4 s later and whether GetTask still answers.
Usage: terminate_live.py REPLICA LABEL"""
import sys, time
import common

BIN, LABEL = sys.argv[1], sys.argv[2]
DB, PORT = "a2a_terminate", 7860
rep = common.start(BIN, LABEL, PORT, common.fresh_db(DB), common.OUT / f"term-{LABEL}.log", "warn")
t, _ = common.send(PORT, "tick", now=True)
time.sleep(2.2)
killed = common.terminate_all(DB)
time.sleep(4)
state, parts, _ = common.stored_ticks(DB, t["id"])
via_get = common.rpc(PORT, "GetTask", {"id": t["id"]})
print(f"{LABEL}: terminated {killed} backends; 4 s later state {state}, ticks 0..{max(parts) if parts else None} "
      f"stored, missing {common.missing(parts, 0)}; GetTask {'ok' if 'result' in via_get else via_get.get('error')}")
rep.terminate(); rep.wait(timeout=20)
