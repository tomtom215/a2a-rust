#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A task's row locked for 2 s mid-turn under a 200 ms statement_timeout:
every write the turn makes meanwhile times out. One replica runs `tick`;
reports gaps in the task's parts and in its event log 6 s after.
Usage: lock_live.py REPLICA LABEL"""
import sys, time
import common

BIN, LABEL = sys.argv[1], sys.argv[2]
DB, PORT = "a2a_lock", 7861
url = common.fresh_db(DB)
common.psql(f"ALTER DATABASE {DB} SET statement_timeout = '200ms'", DB)
rep = common.start(BIN, LABEL, PORT, url, common.OUT / f"lock-{LABEL}.log")
t, _ = common.send(PORT, "tick", now=True)
time.sleep(2.2)
t0 = time.time()
common.psql(f"SET statement_timeout = 0; BEGIN; SELECT 1 FROM tasks WHERE id = '{t['id']}' FOR UPDATE; "
            "SELECT pg_sleep(2); COMMIT;", DB)
outage = time.time() - t0
time.sleep(6)
state, parts, seqs = common.stored_ticks(DB, t["id"])
print(f"{LABEL}: row locked {outage:.1f} s; state {state}; parts 0..{max(parts)}, missing {common.missing(parts, 0)}; "
      f"log 1..{max(seqs)}, missing {common.missing(seqs, 1)}")
rep.terminate(); rep.wait(timeout=20)
