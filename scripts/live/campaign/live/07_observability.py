#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Observability leaks: canary strings through traces, metrics, logs and the task.

Needs: the binaries of scripts/live/campaign/live/agent/, python3. `scripted` uses model_stub.py so every
canary is certain to pass through the loop; `live` (llama-server on 127.0.0.1:11434) lets a real model carry
the prompt and tool result canaries and records which came back in its reply.

Canaries (each planted once): in the prompt, in a message metadata value, in an HTTP header and in the
bearer token that authenticates, in a tool argument, in a tool result, in a skill's body and in a skill's
reference file, in the model's reply, in a provider error body, and in an OTLP export header (which the
collector is meant to receive). The export is OTLP over HTTP+JSON to a local receiver (otlp_sink.py)
with traces, metrics and logs all on. For each canary the output says which signals carried it, and which of
the agent's stderr, the task's status message, its artifacts and its history did.
Compare with book/src/building-agents/tool-loop/telemetry.md ("No content") and the observability pages.
"""
import json
import os
import sys
import time
import importlib.util

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403
from otlp_sink import Sink
import skills_fixture

_spec = importlib.util.spec_from_file_location("tr", str(HERE / "05_tool_results.py"))
tr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tr)

C = {
    "prompt": "CANARY_PROMPT_a1",
    "meta": "CANARY_META_a2",
    "header": "CANARY_HEADER_a3",
    "bearer": "CANARY_BEARER_a4",
    "toolarg": "CANARY_TOOLARG_a5",
    "toolresult": "CANARY_TOOLRESULT_a6",
    "skillbody": "CANARY_SKILLBODY_a7",
    "skillref": "CANARY_SKILLREF_a8",
    "reply": "CANARY_REPLY_a9",
    "errbody": "CANARY_ERRBODY_b1",
    "exportkey": "CANARY_EXPORTKEY_b2",
    "steer": "CANARY_STEER_b3",
}


def make_skills(root):
    skills_fixture.write(root / "canary-skill/SKILL.md",
                         skills_fixture.md("canary-skill", "Use for canary questions.", f"Body text {C['skillbody']}."))
    skills_fixture.write(root / "canary-skill/references/ref.md", f"Reference text {C['skillref']}.\n")


def search(name, sink, agent, task_json, extra_texts=None):
    canaries = list(C.values())
    hits = sink.canary_hits(canaries)
    stderr = agent.stderr()
    stdout = agent.stdout()
    rows = {}
    for key, canary in C.items():
        where = dict(hits[canary])
        if canary in stderr:
            where["agent-stderr"] = stderr.count(canary)
        if canary in stdout:
            where["agent-stdout"] = 1
        tj = json.dumps(task_json)
        if canary in tj:
            where["task-json"] = 1
        st = status_text(task_json) if task_json else ""
        if canary in st:
            where["task-status-message"] = 1
        for label, text in (extra_texts or {}).items():
            if canary in text:
                where[label] = 1
        rows[key] = where
    return rows


def scripted():
    header = run_header("observability canaries: scripted")
    out = {"header": header, "runs": {}}
    skills = WORK / "skills-canary"
    make_skills(skills)
    page = WORK / "page-canary.txt"
    page.write_text(f"Page text with {C['toolresult']} in it.")
    sink = Sink()
    script = [
        {"tool_calls": [tr.call("echo", text=C["toolarg"]), tr.call("fetch_page", url="http://example.invalid/x"),
                        tr.call("activate_skill", name="canary-skill"),
                        tr.call("read_skill_file", skill="canary-skill", path="references/ref.md")]},
        {"text": f"The summary contains {C['reply']}."},
    ]
    path = WORK / "script-canary.json"
    path.write_text(json.dumps(script))
    stub = tr.Stub("script", str(path))
    env = {"C_MODEL_URL": stub.url, "C_TOOLS": "inventory,evil", "C_PAGE_FILE": page, "C_SKILLS_OPERATOR": skills,
           "C_AUTH": f"{C['bearer']}:alice", "C_TELEMETRY": "1", "OTEL_EXPORTER_OTLP_ENDPOINT": sink.url(),
           "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json", "OTEL_METRIC_EXPORT_INTERVAL": "1000",
           "OTEL_EXPORTER_OTLP_HEADERS": f"x-api-key={C['exportkey']}", "OTEL_LOGS_EXPORTER": "otlp",
           "RUST_LOG": "debug", "C_STEER": 4}
    try:
        with Agent("canary", C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1, **env) as agent:
            msg = message(f"Summarise the page. {C['prompt']}")
            msg["metadata"] = {"customer_email": C["meta"]}
            st, headers, body = http(agent.ports["rpc"], "POST", "/", {"jsonrpc": "2.0", "id": 1, "method": "SendMessage", "params": {"message": msg}},
                                     {"authorization": f"Bearer {C['bearer']}", "x-canary": C["header"], "x-forwarded-for": C["header"],
                                      "user-agent": f"canary-client/{C['header']}"})
            task = task_of(body.get("result", {})) if isinstance(body, dict) else {}
            print(f"   state {(task.get('status') or {}).get('state')}", flush=True)
            # a steering message with a canary (the task has ended: refused, its text is in the refusal)
            http(agent.ports["rpc"], "POST", "/", {"jsonrpc": "2.0", "id": 2, "method": "SendMessage",
                                                      "params": {"message": message(f"steer {C['steer']}", task.get("id"), steering=True)}},
                 {"authorization": f"Bearer {C['bearer']}"})
            time.sleep(0.5)
        time.sleep(3)
        rows = search("scripted", sink, agent, task)
    finally:
        stub.stop()
    out["runs"]["scripted"] = rows
    print("   canary -> where it appeared (signals: traces, metrics, logs; ':request-header' is the export's own headers)", flush=True)
    for key, where in rows.items():
        print(f"   {key:10} {C[key]:24} {where}", flush=True)
    # Span attributes and log bodies that hold any of the strings, to show where.
    spans = sink.spans()
    out["span_names"] = sorted({s.get("name") for s in spans})
    out["span_count"] = len(spans)
    out["log_count"] = len(sink.logs())
    hit_attrs = []
    for s in spans:
        text = json.dumps(s)
        for key, canary in C.items():
            if canary in text:
                hit_attrs.append({"canary": key, "span": s.get("name"),
                                  "attrs": [a["key"] for a in s.get("attributes", []) if canary in json.dumps(a)]})
    out["span_hits"] = hit_attrs
    for h in hit_attrs:
        print(f"   span {h['span']!r} carries {h['canary']} in attributes {h['attrs']}", flush=True)
    logs = [l for l in sink.logs() if any(c in json.dumps(l) for c in C.values())]
    out["log_hits"] = [json.dumps(l)[:400] for l in logs[:10]]
    for l in out["log_hits"][:5]:
        print(f"   log record: {l[:260]}", flush=True)
    save("07-observability-scripted.json", out)
    # The documented claim: spans record no prompt, argument or result.
    for key in ("prompt", "toolarg", "toolresult", "skillbody", "skillref", "reply"):
        check("traces" not in rows[key], f"{key} canary is not in any exported span")
    for key in ("prompt", "meta", "header", "bearer", "toolarg", "toolresult", "skillbody", "skillref", "reply", "steer"):
        check("metrics" not in rows[key], f"{key} canary is not in any exported metric")
    for key in ("bearer", "header", "exportkey"):
        check(not any(k in rows[key] for k in ("traces", "logs", "agent-stderr", "agent-stdout", "task-json")),
              f"{key} canary is in no span, log, stderr or the task ({rows[key]})")
    return finish()


def err_body():
    header = run_header("observability canaries: provider error body")
    sink = Sink()
    stub = tr.Stub("err_body", C["errbody"])
    try:
        with Agent("canary-err", C_MODEL_URL=stub.url, C_TOOLS="inventory", C_TELEMETRY="1", C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1,
                   OTEL_EXPORTER_OTLP_ENDPOINT=sink.url(), OTEL_EXPORTER_OTLP_PROTOCOL="http/json", OTEL_METRIC_EXPORT_INTERVAL="1000",
                   OTEL_LOGS_EXPORTER="otlp") as agent:
            r = tr.send(agent, f"hello {C['prompt']}")
        time.sleep(3)
        rows = search("err", sink, agent, r["task"])
    finally:
        stub.stop()
    for key in ("errbody", "prompt"):
        print(f"   {key:10} {rows[key]}", flush=True)
    hits = []
    for s in sink.spans():
        text = json.dumps(s)
        if C["errbody"] in text:
            hits.append({"span": s.get("name"), "status": s.get("status"),
                         "attributes": [a["key"] for a in s.get("attributes", []) if C["errbody"] in json.dumps(a)],
                         "events": [{"name": e.get("name"), "attributes": [a["key"] for a in e.get("attributes", []) if C["errbody"] in json.dumps(a)]}
                                    for e in s.get("events", []) if C["errbody"] in json.dumps(e)]})
    for h in hits:
        print(f"   span {h['span']!r}: status {json.dumps(h['status'])[:300]}; attributes {h['attributes']}; events {h['events']}", flush=True)
    logs = [json.dumps(l)[:500] for l in sink.logs() if C["errbody"] in json.dumps(l)]
    for l in logs[:3]:
        print(f"   log record: {l[:400]}", flush=True)
    save("07-observability-err-body.json", {"header": header, "rows": rows, "status": r["status_text"], "span_hits": hits, "log_hits": logs})
    check("task-json" not in rows["errbody"] and "task-status-message" not in rows["errbody"], "the provider's error body is not in the task")
    check("traces" not in rows["errbody"], "the provider's error body is not in a span")
    note(f"errbody appears in: {rows['errbody']} (the book says the body goes to the operator's log)")
    return finish()


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "scripted"
    sys.exit({"scripted": scripted, "err": err_body}[mode]())
