#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""ListTasks with page tokens nobody issued, on the three bindings.

Needs: the binaries of scripts/live/campaign/live/agent/, python3. No model (the stub answers; two tasks are
made so a page exists). Claim under test: a malformed page token is InvalidParams (-32602), HTTP 400, gRPC
INVALID_ARGUMENT, never an empty page or a server error. Tokens tried: garbage, empty, a 100 kB string, a
NUL, a JSON-looking one, a valid token with a flipped character, and one issued by this server for another
filter (which may be accepted or refused: recorded, not graded).
"""
import json
import os
import sys
import subprocess
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403
import importlib.util

_spec = importlib.util.spec_from_file_location("tr", str(HERE / "05_tool_results.py"))
tr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tr)

def grpc_status(port, token):
    """ListTasks over raw gRPC (curl, HTTP/2 prior knowledge): the grpc-status and grpc-message of the reply."""
    raw = token.encode()
    varint = bytearray()
    n = len(raw)
    while True:
        b = n & 0x7F
        n >>= 7
        varint.append(b | (0x80 if n else 0))
        if not n:
            break
    msg = b"\x2a" + bytes(varint) + raw if raw else b""
    frame = b"\x00" + len(msg).to_bytes(4, "big") + msg
    path = WORK / "grpc-frame.bin"
    path.write_bytes(frame)
    p = subprocess.run(["curl", "-s", "--noproxy", "*", "--http2-prior-knowledge", "-D", "-", "-o", "/dev/null", "-H", "content-type: application/grpc",
                        "-H", "te: trailers", "-H", "a2a-version: 1.0", "--data-binary", f"@{path}", f"http://127.0.0.1:{port}/lf.a2a.v1.A2AService/ListTasks"],
                       capture_output=True, text=True, timeout=60)
    status = message = None
    for line in p.stdout.splitlines():
        if line.lower().startswith("grpc-status:"):
            status = line.split(":", 1)[1].strip()
        if line.lower().startswith("grpc-message:"):
            message = line.split(":", 1)[1].strip()
    return status, message


TOKENS = {"garbage": "not-a-token", "empty": "", "huge": "A" * 100_000, "nul": "ab\u0000cd", "jsonish": '{"offset":1}',
          "negative": "-1", "number": "999999999999999999999", "unicode": "‮аб", "slash": "../../x"}


def main():
    header = run_header("ListTasks with a malformed page token")
    out = {"header": header, "rows": []}
    stub = tr.Stub("finish", "stop")
    try:
        with Agent("listtasks", C_MODEL_URL=stub.url, C_TOOLS="inventory", C_PORT_RPC=0, C_PORT_REST=0, C_PORT_GRPC=0) as agent:
            for i in range(3):
                tr.send(agent, f"task {i}")
            # a real token, for the flipped-character variant
            st, body = rpc(agent.ports["rpc"], "ListTasks", {"pageSize": 1})
            real = (body.get("result") or {}).get("nextPageToken")
            note(f"a real next_page_token: {real!r}")
            if real:
                TOKENS["flipped"] = real[:-1] + ("A" if real[-1] != "A" else "B")
                TOKENS["truncated"] = real[: len(real) // 2]
            for name, tok in TOKENS.items():
                row = {"token": name}
                st, body = rpc(agent.ports["rpc"], "ListTasks", {"pageToken": tok})
                row["jsonrpc"] = {"http": st, "code": (body.get("error") or {}).get("code"), "ok": "result" in body,
                                  "tasks": len((body.get("result") or {}).get("tasks") or []) if "result" in body else None}
                q = urllib.parse.quote(tok[:2000], safe="")
                st, body = rest(agent.ports["rest"], "GET", f"/tasks?pageToken={q}")
                row["rest"] = {"http": st, "ok": st == 200}
                status, gmsg = grpc_status(agent.ports["grpc"], tok)
                row["grpc"] = {"grpc_status": status, "grpc_message": gmsg}
                out["rows"].append(row)
                print(f"   {name:10} jsonrpc {row['jsonrpc']}  rest {row['rest']}  grpc status {row['grpc']}", flush=True)
                if name != "empty":
                    check(row["jsonrpc"]["code"] == -32602, f"{name}: JSON-RPC answers -32602 (got {row['jsonrpc']['code']}, ok={row['jsonrpc']['ok']})")
                    check(row["rest"]["http"] == 400, f"{name}: HTTP+JSON answers 400 (got {row['rest']['http']})")
                    check(row["grpc"]["grpc_status"] == "3", f"{name}: gRPC answers INVALID_ARGUMENT, status 3 (got {row['grpc']['grpc_status']})")
            check(agent.alive(), "the agent is alive after every token")
    finally:
        stub.stop()
    save("10-listtasks-token.json", out)
    return finish()


if __name__ == "__main__":
    sys.exit(main())
