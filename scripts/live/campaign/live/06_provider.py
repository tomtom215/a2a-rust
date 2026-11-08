#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A misbehaving model provider (scripted: model_stub.py, no real model).

Needs: the binaries of scripts/live/campaign/live/agent/, python3. For each of: HTML with a 200, a stream
that goes silent, a stream that trickles keep-alives for ever, a 100 MB `data:` line, a 100 MB JSON body,
finish reasons nobody sent before, a body or stream cut mid-JSON, 429 with a `Retry-After` of a year / in the
past / not a number, a chunked body that stalls, and a 502 whose body is a canary (with a forged log line and
ANSI in it): the agent must stay bounded (time, memory), classify the failure, and keep the provider's body out
of the task's status message. Then 200 replies with 200 different `model` names, to count the metric series.
Proves (or fails to) those, and records every number.
"""
import json
import os
import re
import sys
import time
import importlib.util

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403
from otlp_sink import Sink

_spec = importlib.util.spec_from_file_location("tr", str(HERE / "05_tool_results.py"))
tr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tr)

CANARY = "CANARY_PROVIDER_ERROR_BODY_91e2"


def run(name, stub_args, env=None, stream=False, expect=None, timeout=300):
    env = dict(env or {})
    if stream:
        env["C_DRAFTS"] = "1"
    r = tr.scenario(name, stub_args, tools="inventory", env=env, max_turns=3)
    tr.show(name, r)
    return r


def main():
    header = run_header("provider misbehaviour (scripted)")
    out = {"header": header}
    rows = []

    def record(key, r, ok=None, what=""):
        rows.append({"case": key, "state": r["state"], "seconds": r["seconds"], "status_text": r["status_text"][:200],
                     "failure": r["failure"], "rss_kb": [r["rss_before_kb"], r["rss_after_kb"]], "alive": r["alive"]})
        if ok is not None:
            check(ok, f"{key}: {what} (state {r['state']}, {r['seconds']}s, status {r['status_text'][:90]!r})")

    r = run("html", ("html",))
    record("html 200", r, r["state"] == "TASK_STATE_FAILED" and "could not be read" in r["status_text"], "classified as an unreadable answer")

    r = run("sse_never", ("sse_never",), env={"C_TIMEOUT_SECS": 6}, stream=True)
    record("sse never (idle stream, 6 s model timeout)", r, r["state"] == "TASK_STATE_FAILED" and 5 < r["seconds"] < 20, "ended by the model timeout")

    r = run("sse_trickle", ("sse_trickle",), env={"C_TIMEOUT_SECS": 6}, stream=True)
    record("sse trickle (a keep-alive per second, 6 s model timeout)", r, r["seconds"] < 30 and r["state"] == "TASK_STATE_FAILED", "ended by the model timeout although no read was idle")

    r = run("json_trickle", ("json_trickle",), env={"C_TIMEOUT_SECS": 6})
    record("json trickle (1 byte a second, 6 s timeout)", r, r["seconds"] < 30 and r["state"] == "TASK_STATE_FAILED", "ended by the model timeout")

    r = run("sse_big", ("sse_big", 100 * 1024 * 1024), stream=True)
    record("sse 100 MB data line", r, r["state"] == "TASK_STATE_FAILED" and r["rss_after_kb"] < 400_000, "bounded memory, classified")

    r = run("json_big", ("json_big", 100 * 1024 * 1024))
    record("json 100 MB body", r, r["state"] == "TASK_STATE_FAILED" and r["rss_after_kb"] < 400_000, "bounded memory, classified")

    for reason in ("weird", "", "tool_calls", "content_filter", "FUNCTION_CALL"):
        r = run(f"finish_{reason or 'empty'}", ("finish", reason))
        ans = artifact(r["task"], "answer")
        record(f"finish_reason={reason!r}", r)
        print(f"   answer metadata {(ans or {}).get('metadata')}; status message metadata {((r['task'].get('status') or {}).get('message') or {}).get('metadata')}", flush=True)
        check(r["state"] in ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED") and r["alive"], f"finish_reason={reason!r}: a terminal state, agent alive ({r['state']})")

    for s in (False, True):
        r = run(f"cut_mid_{'stream' if s else 'body'}", ("cut_mid",), stream=s)
        record(f"cut mid {'stream' if s else 'body'}", r, r["state"] == "TASK_STATE_FAILED", "classified")
        print(f"   status: {r['status_text']!r}", flush=True)

    for ra in ("31536000", "Wed, 21 Oct 2015 07:28:00 GMT", "0", "-5", "abc", "1e10", "99999999999999999999"):
        r = run(f"retry_{abs(hash(ra)) % 10000}", ("retry_after", ra), env={"C_RETRY": "3:5"})
        record(f"429 Retry-After: {ra}", r, r["state"] == "TASK_STATE_FAILED" and r["seconds"] < 30, "failed without parking the turn")
        print(f"   Retry-After {ra!r}: {r['state']} in {r['seconds']}s; failure {r['failure']}; requests {len(r['requests'])}", flush=True)

    r = run("stall", ("stall_chunked", 60), env={"C_TIMEOUT_SECS": 6})
    record("chunked body that stalls (6 s timeout)", r, r["seconds"] < 30 and r["state"] == "TASK_STATE_FAILED", "ended by the model timeout")

    # A 502 whose body carries a canary, a forged log line and ANSI.
    body = f"upstream error {CANARY}\n2026-11-08T00:00:00.000000Z ERROR forged: admin session opened \x1b[31mRED\x1b[0m\x1b]0;title\x07"
    r = run("err_body", ("err_body", body), env={"C_TELEMETRY": "1"})
    record("502 with a canary body", r, CANARY not in r["status_text"] and CANARY not in json.dumps(r["task"]), "the provider's body is not in the task")
    log_lines = r["stderr"].splitlines()
    forged = [l for l in log_lines if l.startswith("2026-11-08T00:00:00.000000Z ERROR forged")]
    raw_esc = "\x1b[31mRED" in r["stderr"]
    print(f"   operator log: canary present {CANARY in r['stderr']}; a forged line begins a log line: {bool(forged)}; raw ESC bytes present: {raw_esc}", flush=True)
    out["err_body_log"] = {"canary_in_log": CANARY in r["stderr"], "forged_line_starts_a_line": bool(forged), "raw_escape_bytes": raw_esc,
                           "log_excerpt": [l[:300] for l in log_lines if "CANARY" in l or "forged" in l][:6]}
    save("06-err-body-stderr.txt", r["stderr"][-6000:])
    check(not forged, "the operator log has no line a provider could forge")
    check(not raw_esc, "the provider's ANSI escapes are not written raw to the operator's log")

    out["rows"] = rows
    save("06-provider-misbehaviour.json", out)
    for row in rows:
        print(f"   {row['case']:62} {row['state']:22} {row['seconds']:>6}s  rss {row['rss_kb']}", flush=True)
    return finish()


def cardinality():
    header = run_header("metric label cardinality: 200 replies, 200 model names")
    sink = Sink()
    stub = tr.Stub("models")
    try:
        with Agent("cardinality", C_MODEL_URL=stub.url, C_TOOLS="inventory", C_TELEMETRY="1", C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1,
                   OTEL_EXPORTER_OTLP_ENDPOINT=sink.url(), OTEL_EXPORTER_OTLP_PROTOCOL="http/json", OTEL_METRIC_EXPORT_INTERVAL="1000") as agent:
            for i in range(200):
                tr.send(agent, f"varied prompt number {i} with text {os.urandom(4).hex()}")
            rss = agent.rss_kb()
        time.sleep(3)
    finally:
        stub.stop()
    names = sorted(sink.metric_names())
    result = {}
    for metric in names:
        pts = sink.points(metric)
        series = {json.dumps(p["attributes"], sort_keys=True) for p in pts}
        labels = {}
        for p in pts:
            for k, v in p["attributes"].items():
                labels.setdefault(k, set()).add(str(v))
        result[metric] = {"series": len(series), "labels": {k: len(v) for k, v in labels.items() if len(v) > 1}}
    for m, v in result.items():
        print(f"   {m}: {v['series']} series; labels with >1 value: {v['labels']}", flush=True)
    worst = max(((v["labels"].get("gen_ai.response.model", 0)), m) for m, v in result.items())
    print(f"   most distinct gen_ai.response.model values on one metric: {worst}", flush=True)
    save("06-metric-cardinality.json", {"header": header, "after_200_requests": result, "agent_rss_kb": rss})
    check(worst[0] <= 20, f"200 provider-chosen model names make {worst[0]} distinct gen_ai.response.model label values on {worst[1]}")
    sink.stop()
    return finish()


if __name__ == "__main__":
    sys.exit(cardinality() if sys.argv[1:] == ["cardinality"] else main())
