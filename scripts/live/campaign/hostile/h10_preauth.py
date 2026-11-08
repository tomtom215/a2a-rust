# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H10: what an unauthenticated caller can learn. With bearer authentication on, every
request below carries no credential (or a wrong one); the answers are tabulated
against the plain 401 an ordinary known-method request gets. A different answer
is information a caller had no right to: which methods and routes exist, how a
body parses, what limit it hit.

Needs: the target binary (A2A_HOSTILE_BIN). Evidence: evidence/h10_preauth.jsonl and
a table on stdout. Usage: python3 -I h10_preauth.py
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import BASE_HEADERS, Recorder, Target, http  # noqa: E402


def norm(b: bytes) -> str:
    return re.sub(r"[0-9a-f]{8}-[0-9a-f-]{27}|reference [0-9a-f]+", "<id>", b.decode("utf-8", "replace"))[:150]


def main() -> None:
    t = Target("h10_preauth", {"STORE": "tmem", "AUTH": "bearer"})
    r = Recorder("h10_preauth")
    try:
        ok = lambda m: json.dumps({"jsonrpc": "2.0", "id": 1, "method": m, "params": {"id": "x"}})  # noqa: E731
        ref_st, ref_h, ref_b = http(t.port, "POST", "/", ok("GetTask"), dict(BASE_HEADERS))
        ref = (ref_st, ref_h.get("www-authenticate"), norm(ref_b))
        rest_ref = http(t.port, "GET", "/tasks/x", None, dict(BASE_HEADERS))
        rest_ref = (rest_ref[0], rest_ref[1].get("www-authenticate"), norm(rest_ref[2]))
        r.rec("reference: known method, no credential", "held", ref)
        r.rec("reference: known REST route, no credential", "held", rest_ref)
        cases = [
            ("JSON-RPC unknown method", "POST", "/", ok("NoSuchMethod")),
            ("JSON-RPC method of the wrong type", "POST", "/", '{"jsonrpc":"2.0","id":1,"method":5,"params":{}}'),
            ("JSON-RPC invalid JSON", "POST", "/", "{not json"),
            ("JSON-RPC empty body", "POST", "/", ""),
            ("JSON-RPC jsonrpc version 1.0", "POST", "/", '{"jsonrpc":"1.0","id":1,"method":"GetTask","params":{}}'),
            ("JSON-RPC batch", "POST", "/", "[" + ok("GetTask") + "]"),
            ("JSON-RPC body over the 4 MiB limit", "POST", "/", '{"a":"' + "x" * (5 * 1024 * 1024) + '"}'),
            ("JSON-RPC notification (no id)", "POST", "/", '{"jsonrpc":"2.0","method":"GetTask","params":{"id":"x"}}'),
            ("JSON-RPC wrong A2A-Version header", "POST", "/", ok("GetTask")),
            ("REST unknown route", "GET", "/nosuch", None),
            ("REST known route, unsupported method (DELETE /tasks/x)", "DELETE", "/tasks/x", None),
            ("REST POST /message:send with invalid JSON", "POST", "/message:send", "{not json"),
            ("REST GET /tasks/<10000 chars>", "GET", "/tasks/" + "a" * 10000, None),
            ("REST GET /tasks?pageSize=-1", "GET", "/tasks?pageSize=-1", None),
            ("REST OPTIONS /tasks (CORS preflight)", "OPTIONS", "/tasks", None),
            ("REST HEAD /tasks", "HEAD", "/tasks", None),
            ("GET /.well-known/agent-card.json (public by spec)", "GET", "/.well-known/agent-card.json", None),
            ("GET /extendedAgentCard", "GET", "/extendedAgentCard", None),
            ("GET /health", "GET", "/health", None),
            ("GET /ready", "GET", "/ready", None),
            ("GET /metrics", "GET", "/metrics", None),
        ]
        for label, method, path, body in cases:
            hs = dict(BASE_HEADERS)
            if "wrong A2A-Version" in label:
                hs["a2a-version"] = "0.3"
            if method in ("GET", "DELETE", "HEAD", "OPTIONS"):
                hs.pop("content-type", None)
            try:
                st, hd, d = http(t.port, method, path, body, hs, timeout=15)
            except Exception as e:  # noqa: BLE001
                r.rec(label, "held", f"connection error {type(e).__name__}")
                continue
            got = (st, hd.get("www-authenticate"), norm(d))
            expected_ref = rest_ref if path != "/" else ref
            public = any(x in label for x in ("agent-card", "/health", "/ready", "CORS", "HEAD"))
            same = got[:2] == expected_ref[:2] or st == 401
            r.rec(label, "held" if (same or public) else "DEVIATION",
                  {"status": st, "www-authenticate": hd.get("www-authenticate"), "body": norm(d), "public_by_design": public})
        r.rec("target alive", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()


if __name__ == "__main__":
    main()
