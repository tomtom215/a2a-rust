#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A burst of events through an 8 s row lock under a 100 ms
statement_timeout. One replica runs `burst` (2,000 parts) or `burst6k`
(6,000); 0.3 s in, the task's row is locked for 8 s. Reports gaps in the
parts and the log 40 s later; the replica's log says how many of the
executor's writes failed. Usage: burst_live.py REPLICA LABEL [burst|burst6k]"""
import sys, time
import common

BIN, LABEL = sys.argv[1], sys.argv[2]
DB, PORT = "a2a_burst", 7861
rep = common.start(BIN, LABEL, PORT, common.fresh_db(DB), common.OUT / f"burst-{LABEL}.log")
common.psql(f"ALTER DATABASE {DB} SET statement_timeout = '100ms'", DB)
common.terminate_all(DB)  # the replica's connections pick the setting up
time.sleep(1.5)
t, _ = common.send(PORT, sys.argv[3] if len(sys.argv) > 3 else "burst", now=True)
time.sleep(0.3)
t0 = time.time()
common.psql(f"SET statement_timeout = 0; BEGIN; SELECT 1 FROM tasks WHERE id = '{t['id']}' FOR UPDATE; "
            "SELECT pg_sleep(8); COMMIT;", DB)
outage = time.time() - t0
time.sleep(40)
state, parts, seqs = common.stored_ticks(DB, t["id"])
print(f"{LABEL}: row locked {outage:.1f} s; state {state}; parts 0..{max(parts)}, missing {common.missing(parts, 0)}; "
      f"log 1..{max(seqs)}, missing {common.missing(seqs, 1)}")
rep.terminate(); rep.wait(timeout=20)
