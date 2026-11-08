# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Smoke test of the hostile target: starts it on the in-memory store, sends one
message over JSON-RPC and REST, lists, and stops. Needs the target binary
(A2A_HOSTILE_BIN). Proves the harness reaches the server; attacks nothing."""

import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from lib import Target, rest, rpc, send, task_of  # noqa: E402

t = Target("smoke", {"STORE": "mem"})
try:
    st, r = send(t.port, "echo:hello")
    print("send", st, str(r)[:300])
    task = task_of(r)
    print("get", rpc(t.port, "GetTask", {"id": task["id"]})[0])
    print("list", str(rpc(t.port, "ListTasks", {}))[:200])
    print("rest list", str(rest(t.port, "GET", "/tasks"))[:200])
    print("state", t.state())
finally:
    print("exit", t.stop())
