#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Audit M-1 against a real model: an empty final reply fails the task, a cut-off one says so.

Needs: llama-server on 127.0.0.1:11434 (one model; the run header records which, from
$CAMPAIGN_MODEL_FILE), the binaries of scripts/live/campaign/live/agent/, python3.
Proves (or fails to): for each of three cases and each of JSON-RPC, HTTP+JSON and gRPC,
the task state, failure class, status message, answer artifact metadata, and
gen_ai.invoke_agent.duration's error.type for the run.

  A  text answer cut at max_tokens=24 (no tool needed)        -> COMPLETED, finish-reason marker
  B  the README's billing question, max_tokens=48 (tools run) -> COMPLETED-with-marker or FAILED, recorded
  C  thinking on, max_tokens=40                                -> FAILED empty_answer (budget-exhausted)
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403
from otlp_sink import Sink

CASES = {
    "A": dict(prompt="Explain in detail why monitoring matters for a production service.",
              env={"C_EXTRA_BODY": '{"max_tokens":24}'}, tools="inventory"),
    "B": dict(prompt="How is the billing service doing?", env={"C_EXTRA_BODY": '{"max_tokens":48}'}, tools="inventory"),
    "C": dict(prompt="What is 17 times 23? Think it through.", env={"C_THINKING": "1", "C_EXTRA_BODY": '{"max_tokens":40}'},
              tools="inventory"),
}


def failure_class(task):
    md = ((task.get("status") or {}).get("message") or {}).get("metadata") or {}
    return md.get("a2a-rust.com/failure")


def summarize(task):
    ans = artifact(task, "answer")
    return {
        "state": (task.get("status") or {}).get("state"),
        "failure": failure_class(task),
        "status_text": status_text(task),
        "status_message_metadata": ((task.get("status") or {}).get("message") or {}).get("metadata"),
        "answer_text": artifact_text(task, "answer"),
        "answer_metadata": (ans or {}).get("metadata"),
        "trace_lines": (artifact_text(task, "tool-trace") or "").count("\n") + (1 if artifact_text(task, "tool-trace") else 0),
    }


def main(only):
    header = run_header("M-1 empty and cut-off final replies", os.environ.get("CAMPAIGN_MODEL_FILE"))
    allr = {"header": header, "runs": {}}
    for key, case in CASES.items():
        if only and key not in only:
            continue
        sink = Sink()
        env = {"C_MODEL_URL": MODEL, "C_TOOLS": case["tools"], "C_TELEMETRY": "1",
               "OTEL_EXPORTER_OTLP_ENDPOINT": sink.url(), "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
               "OTEL_METRIC_EXPORT_INTERVAL": "2000", "OTEL_BSP_SCHEDULE_DELAY": "500", **case["env"]}
        with Agent(f"m1-{key}", C_PORT_RPC=0, C_PORT_REST=0, C_PORT_GRPC=0, **env) as agent:
            print(f"-- case {key}: {case['prompt']!r} env={case['env']}", flush=True)
            runs = {}
            t0 = time.time()
            st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message(case["prompt"])})
            runs["jsonrpc"] = {"seconds": round(time.time() - t0, 1), "http": st, **summarize(task_of(body.get("result", {}))),
                               "rpc_error": body.get("error")}
            t0 = time.time()
            st, body = rest(agent.ports["rest"], "POST", "/message:send", {"message": message(case["prompt"])})
            runs["rest"] = {"seconds": round(time.time() - t0, 1), "http": st, **summarize(task_of(body)), "raw_error": body if st != 200 else None}
            t0 = time.time()
            lines = client("grpc", agent.ports["grpc"], "send", case["prompt"])
            ok = next((l["ok"] for l in lines if "ok" in l), None)
            runs["grpc"] = {"seconds": round(time.time() - t0, 1), **(summarize(task_of(ok)) if ok else {"client_lines": lines})}
            for binding, r in runs.items():
                print(f"   {binding:8} {json.dumps({k: v for k, v in r.items() if k != 'answer_text'}, default=str)}", flush=True)
                print(f"   {binding:8} answer_text={r.get('answer_text')!r}", flush=True)
            allr["runs"][key] = runs
        time.sleep(3)  # let the sink take the flush at shutdown
        points = sink.points("gen_ai.invoke_agent.duration")
        allr.setdefault("invoke_agent_duration", {})[key] = [
            {"error.type": p["attributes"].get("error.type"), "count": p["dp"].get("count")} for p in points]
        print(f"   gen_ai.invoke_agent.duration series: {allr['invoke_agent_duration'][key]}", flush=True)
        allr.setdefault("finish_reasons", {})[key] = sorted({
            json.dumps(a["value"]) for s in sink.spans() for a in s.get("attributes", [])
            if a.get("key") == "gen_ai.response.finish_reasons"})
        sink.stop()
        # expectations
        runs = allr["runs"][key]
        for binding, r in runs.items():
            if key == "A":
                check(r.get("state") == "TASK_STATE_COMPLETED" and (r.get("answer_metadata") or {}).get("a2a-rust.com/finish-reason") == "length",
                      f"A {binding}: completed with the finish-reason marker on the answer artifact")
                check("cut off" in (r.get("status_text") or ""), f"A {binding}: the Completed status message says the answer was cut off")
            if key == "C":
                check(r.get("state") == "TASK_STATE_FAILED", f"C {binding}: the task failed (state {r.get('state')})")
                check(r.get("failure") in ("budget-exhausted", {"class": "budget-exhausted"}) or "budget-exhausted" in json.dumps(r.get("failure")),
                      f"C {binding}: failure class budget-exhausted (got {r.get('failure')})")
        save(f"01-m1-{key}.json", {"header": header, "case": key, "prompt": case["prompt"], "env": case["env"],
                                    "runs": allr["runs"][key], "invoke_agent_duration": allr["invoke_agent_duration"][key],
                                    "finish_reasons": allr["finish_reasons"][key]})
    return finish()


if __name__ == "__main__":
    sys.exit(main(set(sys.argv[1:])))
