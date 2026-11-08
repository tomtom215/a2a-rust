# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H1: hostile caller-controlled values against every store and two bindings.

Needs: the target binary (A2A_HOSTILE_BIN), a local PostgreSQL (A2A_LIVE_PG),
Python 3 stdlib. Proves (or refutes): that no id, token, number, timestamp,
tenant, status or URL a caller can type produces a 5xx, a -32603, text from
the database or the OS, a panic or a dead process, and that the stores answer
the same hostile input the same way. The class is the one the `ListTasks`
page-token fix closed (a token with a separator and a nonsense timestamp
reached PostgreSQL's `::timestamp` cast and came back as -32603 carrying the
database's own message).

Usage: python3 -I h1_inputs.py [store ...]   (default: every store)
Writes evidence/h1_<store>.jsonl and evidence/h1_matrix.json.
"""

import json
import os
import sys
import tempfile
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import (EV, Recorder, Target, fresh_db, drop_db, http, leaks, msg, rest, rpc,  # noqa: E402
                 send, task_of, BASE_HEADERS)

STORES = ["mem", "sqlite", "pg", "tmem", "tsqlite", "tpg", "enc-mem", "enc-pg"]
TENANT_HDR = {"x-tenant-id": "t1"}

IDS = {
    "empty": "", "nul": "a\u0000b", "newline": "a\nb", "cr": "a\rb", "tab": "a\tb",
    "long10k": "a" * 10000, "surrogate": "\ud800", "surrogate_pair_half": "x\udc00y",
    "emoji": "\U0001F600", "rtl": "ab‮cd", "zwj": "a‍b", "nel": "a\u0085b",
    "ls": "a b", "bom": "﻿a", "traversal": "../../etc/passwd", "sqli": "'; DROP TABLE tasks;--",
    "pct": "a%00b", "space": " ", "colon": "a:cancel", "slash": "a/b", "backslash": "a\\b",
    "q": "a?b=c", "hash": "a#b", "dquote": 'a"b', "json": '{"a":1}', "null": "null",
    "long300": "a" * 300, "uuid_upper": "A" * 36, "nonbmp_long": "\U0001F600" * 200,
}

NUMBERS = {  # raw JSON texts for pageSize / historyLength
    "neg1": "-1", "zero": "0", "int63": "9223372036854775808", "int64": "18446744073709551616",
    "huge_e": "1e999", "neg_e": "-1e999", "float": "1.5", "str_num": '"10"', "nan": '"NaN"',
    "null": "null", "true": "true", "arr": "[]", "obj": "{}", "bignum": "9" * 400,
    "minint": "-9223372036854775808", "u32max": "4294967296", "u32max1": "4294967295",
    "101": "101", "100": "100",
}

TIMESTAMPS = [  # statusTimestampAfter
    "", "x", "0000-01-01T00:00:00Z", "0000-12-31T23:59:59.999Z", "0001-01-01T00:00:00+01:00",
    "9999-12-31T23:59:59.999Z", "9999-12-31T23:59:60Z", "2026-12-31T23:59:60Z",
    "2026-06-30T23:59:60Z", "2026-02-30T00:00:00Z", "2026-01-01T00:00:00", "2026-01-01 00:00:00",
    "2026-01-01T00:00:00+99:99", "2026-01-01T00:00:00.9999999999Z", "\u0000", "1" * 5000,
    "2026-01-01T00:00:00Z\u0000", "-0001-01-01T00:00:00Z", "+2026-01-01T00:00:00Z", "10000-01-01T00:00:00Z",
]

TOKENS = {  # pageToken shapes (the first block is shaped like the stores' cursor)
    "year0000": "0000-01-01 00:00:00\nx", "year0000_frac": "0000-12-31 23:59:59.999999\nx",
    "leap": "2026-12-31 23:59:60\nx", "y9999": "9999-12-31 23:59:59.999999\nx",
    "nul_id": "2026-01-15 10:30:45\n\u0000", "nul_id2": "2026-01-15 10:30:45.123\nab\u0000cd",
    "long_id": "2026-01-15 10:30:45\n" + "a" * 10000, "empty_id": "2026-01-15 10:30:45\n",
    "surrogate_id": "2026-01-15 10:30:45\n\ud800", "emoji_id": "2026-01-15 10:30:45\n\U0001F600",
    "nl_in_id": "2026-01-15 10:30:45\n\n\n", "no_sep": "garbage", "empty": "", "just_nl": "\n",
    "ts_only": "2026-01-15 10:30:45", "T_sep": "2026-01-15T10:30:45\nx",
    "tz": "2026-01-15 10:30:45+00\nx", "pct": "%00", "b64": "MjAyNi0wMS0xNSAxMDozMDo0NQp4",
    "year0001": "0001-01-01 00:00:00\nx", "sqlite_style": "2026-01-15 10:30:45.123\nx",
    "future": "2999-01-01 00:00:00\nx", "ancient": "0001-01-01 00:00:00.000\nzzz",
    "frac1": "2026-01-15 10:30:45.1\nx", "frac6": "2026-01-15 10:30:45.123456\nx",
    "frac7": "2026-01-15 10:30:45.1234567\nx", "ctl_in_ts": "2026-01-15 10:30:\u000045\nx",
}

STATUSES = ["", "x", "TASK_STATE_WORKING", "working", "TASK_STATE_UNSPECIFIED", "\u0000",
            "TASK_STATE_COMPLETED' OR '1'='1", "TASK_STATE_" + "A" * 5000, "completed", "9", "-1"]


def start(store: str, tmp: str):
    env = {"STORE": store, "TENANT": "header" if store.startswith("t") else "none"}
    db = None
    if "pg" in store:
        db = f"a2a_camp_h1_{store.replace('-', '_')}"
        env["DATABASE_URL"] = fresh_db(db)
    if "sqlite" in store:
        env["SQLITE_URL"] = f"sqlite://{tmp}/{store}.db?mode=rwc"
    return Target(f"h1_{store}", env), db


def normalise(status, body):
    code = None
    if isinstance(body, dict):
        e = body.get("error")
        if isinstance(e, dict):
            code = e.get("code")
        elif "code" in body or "status" in body:
            code = body.get("code") or body.get("status")
    return (status, code)


def run(store: str, tmp: str, matrix: dict) -> dict:
    t, db = start(store, tmp)
    r = Recorder(f"h1_{store}")
    xh = TENANT_HDR if store.startswith("t") else None
    panics_seen = 0
    try:
        # Seed: three tasks, two in context c1, so a real page token exists.
        for i in range(3):
            send(t.port, f"echo:seed{i}", ctx="c1" if i < 2 else "c2", extra=xh)
        # A real token, for the "shaped like ours" cases to be compared with.
        st, real = rpc(t.port, "ListTasks", {"pageSize": 1}, extra=xh)
        real_tok = (real.get("result") or {}).get("nextPageToken") if isinstance(real, dict) else None
        r.rec("real page token issued", "held" if real_tok else "DEVIATION", repr(real_tok)[:80])

        def check(label, status, body, via):
            nonlocal panics_seen
            n = normalise(status, body)
            matrix.setdefault(label, {})[store] = list(n)
            problems = []
            if status >= 500 and status != 503:
                problems.append(f"http {status}")
            if n[1] == -32603:
                problems.append("-32603")
            lk = leaks(body)
            if lk:
                problems.append(f"leaks {lk}")
            if not t.alive():
                problems.append("TARGET DEAD")
            p = len(t.panics())
            if p > panics_seen:
                problems.append("PANIC in log")
                panics_seen = p
            r.rec(f"{via} {label}", "held" if not problems else "DEVIATION",
                  {"status": status, "norm": n, "problems": problems,
                   "body": (body if isinstance(body, (dict, list)) else str(body))
                   if problems else None})

        def rpcp(method, params, label):
            st, body = rpc(t.port, method, params, extra=xh)
            check(label, st, body, "rpc")

        def rpc_raw(method, raw_params, label):
            body = '{"jsonrpc":"2.0","id":1,"method":"%s","params":%s}' % (method, raw_params)
            hs = dict(BASE_HEADERS)
            hs.update(xh or {})
            st, _, data = http(t.port, "POST", "/", body, hs)
            try:
                parsed = json.loads(data)
            except ValueError:
                parsed = data.decode("utf-8", "replace")
            check(label, st, parsed, "rpc")

        def restq(path, label, method="GET", body=None):
            hs = dict(BASE_HEADERS)
            hs.update(xh or {})
            st, _, data = http(t.port, method, path, body, hs)
            try:
                parsed = json.loads(data) if data else None
            except ValueError:
                parsed = data.decode("utf-8", "replace")
            check(label, st, parsed, "rest")

        def q(s):
            return urllib.parse.quote(s.encode("utf-8", "surrogatepass"), safe="")

        # ── ListTasks: page tokens ──────────────────────────────────────────
        for k, v in TOKENS.items():
            rpcp("ListTasks", {"pageToken": v, "pageSize": 2}, f"list.pageToken.{k}")
            restq(f"/tasks?pageToken={q(v)}&pageSize=2", f"list.pageToken.{k}")
        # ── ListTasks: numbers ──────────────────────────────────────────────
        for k, v in NUMBERS.items():
            rpc_raw("ListTasks", '{"pageSize":%s}' % v, f"list.pageSize.{k}")
            rpc_raw("ListTasks", '{"historyLength":%s}' % v, f"list.historyLength.{k}")
            if k in ("neg1", "int63", "int64", "huge_e", "float", "nan", "bignum", "zero"):
                qv = v.strip('"')
                restq(f"/tasks?pageSize={q(qv)}", f"list.pageSize.{k}")
                restq(f"/tasks?historyLength={q(qv)}", f"list.historyLength.{k}")
        # ── ListTasks: timestamps, status, context, tenant ──────────────────
        for i, v in enumerate(TIMESTAMPS):
            rpcp("ListTasks", {"statusTimestampAfter": v}, f"list.statusTimestampAfter.{i}")
            restq(f"/tasks?statusTimestampAfter={q(v)}", f"list.statusTimestampAfter.{i}")
        for i, v in enumerate(STATUSES):
            rpcp("ListTasks", {"status": v}, f"list.status.{i}")
            restq(f"/tasks?status={q(v)}", f"list.status.{i}")
        for k, v in IDS.items():
            rpcp("ListTasks", {"contextId": v}, f"list.contextId.{k}")
            restq(f"/tasks?contextId={q(v)}", f"list.contextId.{k}")
            rpcp("ListTasks", {"tenant": v}, f"list.tenant.{k}")
        # ── the named-task operations with hostile ids ──────────────────────
        for k, v in IDS.items():
            rpcp("GetTask", {"id": v}, f"GetTask.id.{k}")
            rpcp("CancelTask", {"id": v}, f"CancelTask.id.{k}")
            rpcp("GetTaskPushNotificationConfig", {"taskId": v, "id": "c"}, f"GetPush.taskId.{k}")
            rpcp("GetTaskPushNotificationConfig", {"taskId": "x", "id": v}, f"GetPush.id.{k}")
            rpcp("ListTaskPushNotificationConfigs", {"taskId": v}, f"ListPush.taskId.{k}")
            rpcp("DeleteTaskPushNotificationConfig", {"taskId": "x", "id": v}, f"DelPush.id.{k}")
            restq(f"/tasks/{q(v)}", f"GetTask.id.{k}")
            restq(f"/tasks/{q(v)}:cancel", f"CancelTask.id.{k}", "POST", "{}")
            restq(f"/tasks/x/pushNotificationConfigs/{q(v)}", f"GetPush.id.{k}")
        # ── sends with hostile ids, metadata and extension URIs ─────────────
        for k, v in IDS.items():
            m = msg("echo:x")
            m["messageId"] = v
            rpcp("SendMessage", {"message": m}, f"Send.messageId.{k}")
            m = msg("echo:x")
            m["contextId"] = v
            rpcp("SendMessage", {"message": m}, f"Send.contextId.{k}")
            m = msg("echo:x")
            m["taskId"] = v
            rpcp("SendMessage", {"message": m}, f"Send.taskId.{k}")
            m = msg("echo:x", extensions=[v])
            rpcp("SendMessage", {"message": m}, f"Send.extension.{k}")
            m = msg("echo:x")
            m["metadata"] = {v: 1}
            rpcp("SendMessage", {"message": m}, f"Send.metadataKey.{k}")
            m = msg("echo:x")
            m["metadata"] = {"a2a-rust.com/idempotency-key": v}
            m["extensions"] = ["https://a2a-rust.com/extensions/idempotency/v1"]
            rpcp("SendMessage", {"message": m}, f"Send.idempotencyKey.{k}")
        # ── push config create with hostile id/token/tenant on an existing task ──
        st, s = send(t.port, "echo:pushbase", extra=xh)
        base = task_of(s)
        if base:
            for k, v in IDS.items():
                rpcp("CreateTaskPushNotificationConfig",
                     {"taskId": base["id"], "url": "https://example.com/h", "id": v},
                     f"CreatePush.id.{k}")
                rpcp("CreateTaskPushNotificationConfig",
                     {"taskId": base["id"], "url": "https://example.com/h", "token": v},
                     f"CreatePush.token.{k}")
        r.rec("target survived the battery", "held" if t.alive() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()
        if db:
            drop_db(db)
    return r.tally


def main() -> None:
    stores = sys.argv[1:] or STORES
    matrix: dict = {}
    tallies = {}
    with tempfile.TemporaryDirectory(prefix="a2a_camp_h1_") as tmp:
        for s in stores:
            print(f"==== {s}")
            tallies[s] = run(s, tmp, matrix)
    differing = {k: v for k, v in matrix.items()
                 if len({tuple(x) for x in v.values()}) > 1}
    out = {"stores": stores, "tallies": tallies, "cases": len(matrix),
           "differing": differing}
    (EV / "h1_matrix.json").write_text(json.dumps(out, indent=1))
    print(json.dumps({"stores": stores, "tallies": tallies, "cases": len(matrix),
                      "differing_cases": len(differing)}))


if __name__ == "__main__":
    main()
