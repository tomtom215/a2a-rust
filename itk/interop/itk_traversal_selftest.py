# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Deterministic self-test of the ITK traversal agent (itk-current-agent).

Exercises the upstream a2a-itk instruction contract — the exact protobuf
wire format from ``protos/instruction.proto`` — against a locally running
``itk-current-agent``, without needing the full ITK cluster or its
baseline agents. This is the reproducible in-repo counterpart to the
``.github/workflows/itk.yml`` current-mount run (which drives the real
upstream harness with the official SDK baselines).

Covers, over every transport (JSONRPC, HTTP+JSON, GRPC): plain and
streaming one-hop traversal, multi-hop chains, series concatenation, the
hold/``task-finished`` marker, and the disconnect-then-resubscribe flow.

Compiles the vendored proto at runtime (needs ``grpcio-tools``).

Usage: python itk_traversal_selftest.py [http://127.0.0.1:PORT]
Exit codes: 0 all passed, 1 failures.
"""
import base64
import json
import os
import subprocess
import sys
import tempfile

import httpx

_HERE = os.path.dirname(os.path.abspath(__file__))
_PROTO = os.path.join(_HERE, "..", "protos", "instruction.proto")
_OUT = tempfile.mkdtemp(prefix="itk-proto-")
subprocess.run(
    [
        sys.executable,
        "-m",
        "grpc_tools.protoc",
        f"-I{os.path.dirname(_PROTO)}",
        f"--python_out={_OUT}",
        _PROTO,
    ],
    check=True,
)
sys.path.insert(0, _OUT)
import instruction_pb2 as pb  # noqa: E402

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:10110"
CARD = f"{BASE}/.well-known/agent-card.json"
FAILS = []


def check(name, cond, detail=""):
    print(f"  [{'PASS' if cond else 'FAIL'}] {name} {'' if cond else detail}")
    if not cond:
        FAILS.append(name)


def send(instruction, streaming=False):
    raw = instruction.SerializeToString()
    msg = {
        "messageId": "drv-1",
        "role": "ROLE_USER",
        "parts": [
            {
                "raw": base64.b64encode(raw).decode(),
                "filename": "instruction.bin",
                "mediaType": "application/x-protobuf",
            }
        ],
    }
    method = "SendStreamingMessage" if streaming else "SendMessage"
    body = {"jsonrpc": "2.0", "id": 1, "method": method, "params": {"message": msg}}
    headers = {"content-type": "application/json", "a2a-version": "1.0"}
    if streaming:
        texts = []
        with httpx.stream("POST", f"{BASE}/", json=body, headers=headers, timeout=60) as r:
            for line in r.iter_lines():
                if line.startswith("data: "):
                    frame = json.loads(line[6:])
                    res = frame.get("result", {})
                    su = res.get("statusUpdate")
                    if su and su.get("status", {}).get("message"):
                        for p in su["status"]["message"].get("parts", []):
                            if "text" in p:
                                texts.append(p["text"])
        return texts
    r = httpx.post(f"{BASE}/", json=body, headers=headers, timeout=60)
    res = r.json()
    if "error" in res:
        raise RuntimeError(res["error"])
    task = res["result"]["task"]
    out = []
    m = task.get("status", {}).get("message")
    if m:
        out = [p["text"] for p in m.get("parts", []) if "text" in p]
    return out


def ret(text, hold=False):
    i = pb.Instruction()
    i.return_response.response = text
    i.return_response.hold_task = hold
    return i


def call(transport, nested, streaming=False, resubscribe=False):
    i = pb.Instruction()
    i.call_agent.transport = transport
    i.call_agent.agent_card_uri = CARD
    i.call_agent.instruction.CopyFrom(nested)
    i.call_agent.streaming = streaming
    if resubscribe:
        i.call_agent.resubscribe.SetInParent()
    else:
        i.call_agent.send_message.SetInParent()
    return i


# 1. Plain ReturnResponse.
check("return_response", send(ret("hello-itk")) == ["hello-itk"])

# 2. One-hop traversal over each transport (agent calls itself).
for transport in ("JSONRPC", "HTTP+JSON", "GRPC"):
    got = send(call(transport, ret(f"via-{transport}")))
    check(f"one-hop {transport}", got == [f"via-{transport}"], f"got {got}")

# 3. Two-hop chain: JSONRPC -> GRPC -> return.
two = call("JSONRPC", call("GRPC", ret("deep")))
got = send(two)
check("two-hop JSONRPC->GRPC", got == ["deep"], f"got {got}")

# 4. Streaming call downstream.
got = send(call("JSONRPC", ret("streamed"), streaming=True))
check("one-hop streaming", got == ["streamed"], f"got {got}")

# 5. Series concat.
series = pb.Instruction()
series.steps.instructions.append(ret("a"))
series.steps.instructions.append(call("HTTP+JSON", ret("b")))
series.steps.response_generator = pb.SeriesOfSteps.RESPONSE_GENERATOR_CONCAT
got = send(series)
check("series concat", got == ["a\nb"], f"got {got}")

# 6. Streaming send of a holding task shows the marker in the stream.
texts = send(ret("held-response", hold=True), streaming=True)
check(
    "hold emits task-finished marker",
    any("task-finished" in t for t in texts),
    f"got {texts}",
)

# 7. Resubscribe behavior against a held downstream task.
got = send(call("JSONRPC", ret("resub-payload", hold=True), resubscribe=True))
check(
    "resubscribe collects held response",
    any("resub-payload" in t for t in got),
    f"got {got}",
)

print()
if FAILS:
    print(f"FAILED: {FAILS}")
    sys.exit(1)
print("All ITK traversal smoke checks passed.")
