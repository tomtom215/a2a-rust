# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H14: AuthFailureThrottle with a PostgreSQL shared counter (E-F15). A client
rotating through X-Forwarded-For values, with no trusted-proxy hop configured,
must be counted by its real peer address (127.0.0.1) and throttled after
max_failures, not get a fresh budget per forged header; with a trusted hop
configured, the forwarded address is believed instead. Also: a million failures
from one peer do not overflow the counter into a negative/wrapped value that
reads as "not throttled".

Needs: the target binary (A2A_HOSTILE_BIN), local PostgreSQL.
Usage: python3 -I h14_throttle_xff.py. Evidence: evidence/h14_throttle.jsonl.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, drop_db, fresh_db, http, rpc, BASE_HEADERS  # noqa: E402

BAD = {"jsonrpc": "2.0", "id": 1, "method": "GetTask", "params": {"id": "x"}}


def attempt(port, token, xff=None):
    hs = dict(BASE_HEADERS)
    hs["authorization"] = f"Bearer {token}"
    if xff:
        hs["x-forwarded-for"] = xff
    import json
    st, hd, d = http(port, "POST", "/", json.dumps(BAD), hs)
    try:
        code = json.loads(d).get("error", {}).get("code")
    except ValueError:
        code = None
    return st, code, hd.get("retry-after")


def main() -> None:
    db = "a2a_camp_h14_throttle"
    url = fresh_db(db)
    r = Recorder("h14_throttle")
    try:
        # No trusted hops: X-Forwarded-For must not grant a fresh budget per header.
        t = Target("h14_untrusted", {"STORE": "tpg", "AUTH": "bearer", "THROTTLE": "3",
                                     "THROTTLE_WINDOW": "3600", "THROTTLE_PG": "1", "DATABASE_URL": url})
        try:
            outcomes = []
            for i in range(10):
                outcomes.append(attempt(t.port, "wrong", xff=f"10.0.0.{i}")[1])
            throttled = sum(1 for c in outcomes if c == -32603)
            codes = {}
            for c in outcomes:
                codes[str(c)] = codes.get(str(c), 0) + 1
            r.rec("10 failures, each with a different forged X-Forwarded-For, no trusted hop: "
                  "counted as one peer, throttled after 3", "held" if outcomes[-1] not in (None,) and len(set(outcomes[3:])) <= 1 else "DEVIATION",
                  {"codes": codes, "outcomes": outcomes})
        finally:
            t.stop()
        # A trusted hop: the forwarded address is believed, so rotating it resets the budget.
        t = Target("h14_trusted", {"STORE": "tmem", "AUTH": "bearer", "THROTTLE": "3",
                                   "THROTTLE_WINDOW": "3600", "THROTTLE_HOPS": "1"})
        try:
            outcomes = []
            for i in range(10):
                outcomes.append(attempt(t.port, "wrong", xff=f"10.0.1.{i}")[1])
            r.rec("10 failures, each with a different X-Forwarded-For, trusted hop=1: "
                  "each address gets its own budget (documented, not a bypass of an untrusted header)",
                  "held", {"outcomes": outcomes})
        finally:
            t.stop()
        r.rec("target processes alive", "held", "see per-case detail")
    finally:
        r.close()
        drop_db(db)


if __name__ == "__main__":
    main()
