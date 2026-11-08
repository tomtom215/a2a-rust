# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H2: a NUL (U+0000) inside a JSON string a caller sends, in every field of a
message the stores persist, per store. PostgreSQL's `jsonb` cannot hold
`\\u0000`, so a field the handler does not refuse can fail only at the write,
after the executor ran: a 500 for the caller's own input, and a task whose turn
was started but whose send was refused.

Needs: the target binary (A2A_HOSTILE_BIN), local PostgreSQL. Prints, per field
and store, the HTTP/JSON-RPC answer and whether a task exists afterwards (an
executor that ran and the send that failed). Writes evidence/h2_<store>.jsonl.
Usage: python3 -I h2_nul_json.py [store ...]
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import Recorder, Target, drop_db, fresh_db, rpc, task_of  # noqa: E402
import uuid  # noqa: E402

N = "a\u0000b"


def cases():
    def m(**kw):
        base = {"messageId": str(uuid.uuid4()), "role": "ROLE_USER", "parts": [{"text": "echo:x"}]}
        base.update(kw)
        return base
    yield "text", m(parts=[{"text": "echo:" + N}])
    yield "part.metadata.value", m(parts=[{"text": "echo:x", "metadata": {"k": N}}])
    yield "part.metadata.key", m(parts=[{"text": "echo:x", "metadata": {N: 1}}])
    yield "part.filename", m(parts=[{"text": "echo:x", "filename": N}])
    yield "part.mediaType", m(parts=[{"text": "echo:x", "mediaType": N}])
    yield "part.data", m(parts=[{"data": {"k": N}}])
    yield "part.url", m(parts=[{"url": "https://example.com/" + N, "mediaType": "text/plain"}])
    yield "message.metadata.value", m(metadata={"k": N})
    yield "message.metadata.key", m(metadata={N: 1})
    yield "message.extensions", m(extensions=[N])
    yield "message.referenceTaskIds", m(referenceTaskIds=[N])
    yield "idempotency.key", m(metadata={"a2a-rust.com/idempotency-key": N * 8},
                               extensions=["https://a2a-rust.com/extensions/idempotency/v1"])
    yield "role", m(role=N)


def run(store: str, tmp: str) -> dict:
    env = {"STORE": store, "TENANT": "header" if store.startswith("t") else "none"}
    db = None
    if "pg" in store:
        db = f"a2a_camp_h2_{store.replace('-', '_')}"
        env["DATABASE_URL"] = fresh_db(db)
    if "sqlite" in store:
        env["SQLITE_URL"] = f"sqlite://{tmp}/h2_{store}.db?mode=rwc"
    t = Target(f"h2_{store}", env)
    r = Recorder(f"h2_{store}")
    xh = {"x-tenant-id": "t1"} if store.startswith("t") else None
    try:
        for label, message in cases():
            before = rpc(t.port, "ListTasks", {"pageSize": 100}, extra=xh)[1]
            n0 = len(before["result"]["tasks"])
            st, ans = rpc(t.port, "SendMessage", {"message": message}, extra=xh)
            after = rpc(t.port, "ListTasks", {"pageSize": 100}, extra=xh)[1]
            n1 = len(after["result"]["tasks"])
            err = ans.get("error") if isinstance(ans, dict) else None
            code = err.get("code") if err else None
            internal = code == -32603 or st >= 500
            created = n1 - n0
            verdict = "held"
            if internal:
                verdict = "DEVIATION"
            r.rec(f"send NUL in {label}", verdict,
                  {"http": st, "code": code, "message": (err or {}).get("message"),
                   "tasks_created": created, "ok": task_of(ans) is not None})
        r.rec("target alive, no panic", "held" if t.alive() and not t.panics() else "DEVIATION",
              t.state())
    finally:
        r.close()
        t.stop()
        if db:
            drop_db(db)
    return r.tally


if __name__ == "__main__":
    stores = sys.argv[1:] or ["mem", "sqlite", "pg", "tmem", "tsqlite", "tpg", "enc-mem", "enc-pg"]
    with tempfile.TemporaryDirectory(prefix="a2a_camp_h2_") as tmp:
        for s in stores:
            print("====", s)
            run(s, tmp)
