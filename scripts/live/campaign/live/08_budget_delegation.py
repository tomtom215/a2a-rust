#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Budgets and delegation with a real model (ADR 0023, examples/live-model's coordinator).

Needs: llama-server on 127.0.0.1:11434, the binaries of scripts/live/campaign/live/agent/ (the coordinator is
`C_MODE=coordinator` of the same binary), python3.

  exhaust    a token budget spent mid-run: 150, 700, 2000 tokens against the billing question (tools, 3 turns)
  deadline   a 4 s deadline while the model's turn and a tool run longer than that
  delegate   a coordinator hands a worker what is left of its budget: completes; budget too small; the worker's
             process killed with SIGKILL mid-run (the resume policy, then Lost); the coordinator cancelled mid-run
             (cascade to the worker, whose tool is dropped); a budget with a deadline only (no `tokens`)
For each: state, failure class, status text, the budget status the task reported (spent/left), seconds.
"""
import json
import os
import signal
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403

SLEEP = "Call the sleep_work tool with seconds set to {n}. When it is done, reply with the single word WORK-DONE."
BILLING = "How is the billing service doing?"


def budget_reports(stream):
    """Every budget status a stream's status updates carried, last one last."""
    out = []
    for e in stream.events:
        ev = e.get("result", e)
        u = ev.get("statusUpdate")
        if u and u.get("metadata"):
            for k, v in u["metadata"].items():
                if "budget" in k:
                    out.append(v)
    return out


def final(binding_port, task):
    return task_of(rpc(binding_port, "GetTask", {"id": task})[1].get("result", {}))


def run_stream(agent, prompt, **kw):
    s = Stream("jsonrpc", agent.ports["rpc"], prompt, **kw)
    s.done.wait(1500)
    task = s.task_id
    t = final(agent.ports["rpc"], task) if task else {}
    return s, t


def summary(t, s=None):
    return {"state": (t.get("status") or {}).get("state"), "status_text": status_text(t),
            "failure": ((t.get("status") or {}).get("message") or {}).get("metadata"),
            "answer": artifact_text(t, "answer") or artifact_text(t, "relay"),
            "budget": (budget_reports(s) or [None])[-1] if s else None}


def worker(name, **env):
    return Agent(name, C_MODEL_URL=MODEL, C_TOOLS="inventory,slow", C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1,
                 C_EXTRA_BODY='{"max_tokens":300}', C_MAX_TURNS=6, **env)


def main():
    header = run_header("budgets and delegation with a real model", os.environ.get("CAMPAIGN_MODEL_FILE"))
    out = {"header": header}

    # ---- exhaust, directly on the worker
    with worker("bud-worker") as w:
        for tokens in (150, 700, 2000):
            t0 = time.time()
            s, t = run_stream(w, BILLING, budget_tokens=tokens)
            r = summary(t, s) | {"seconds": round(time.time() - t0, 1), "budget_tokens": tokens}
            r["tools"] = [l for l in w.tools() if "CALL" in l][-3:]
            out[f"exhaust_{tokens}"] = r
            print(f"-- exhaust {tokens} tokens: {r['state']} in {r['seconds']}s; {r['status_text']!r}; failure {r['failure']}; budget {r['budget']}", flush=True)
            if tokens == 150:
                check(r["state"] == "TASK_STATE_FAILED" and "budget-exhausted" in json.dumps(r["failure"]),
                      f"a 150-token budget ends the run budget-exhausted ({r['state']}, {r['failure']})")
        # deadline: a long turn
        t0 = time.time()
        s, t = run_stream(w, "Write a long, detailed essay of about 300 words on why monitoring matters.", budget_ms=4000)
        r = summary(t, s) | {"seconds": round(time.time() - t0, 1)}
        out["deadline_turn"] = r
        print(f"-- 4 s deadline during a model turn: {r['state']} in {r['seconds']}s; {r['status_text']!r}; failure {r['failure']}", flush=True)
        check(r["seconds"] < 15, f"a 4 s deadline ended a running model turn within 15 s (took {r['seconds']} s)")
        t0 = time.time()
        s, t = run_stream(w, SLEEP.format(n=30), budget_ms=4000)
        r = summary(t, s) | {"seconds": round(time.time() - t0, 1)}
        out["deadline_tool"] = r
        print(f"-- 4 s deadline during a 30 s tool: {r['state']} in {r['seconds']}s; {r['status_text']!r}", flush=True)
        check(r["seconds"] < 15, f"a 4 s deadline ended a running tool within 15 s (took {r['seconds']} s)")
        time.sleep(2)
        # the model server is free again after the deadline cut a turn?
        t0 = time.time()
        s, t = run_stream(w, "Say the word OK.", budget_tokens=100000)
        out["after_deadline"] = summary(t, s) | {"seconds": round(time.time() - t0, 1)}
        print(f"-- a request right after the deadline cut a turn: {out['after_deadline']['state']} in {out['after_deadline']['seconds']}s", flush=True)

    # ---- delegation
    def pair(name, **cenv):
        w = worker(f"{name}-worker")
        c = Agent(f"{name}-coord", C_MODE="coordinator", C_WORKER_URL=f"http://127.0.0.1:{w.ports['rpc']}", C_PORT_RPC=0,
                  C_PORT_REST=-1, C_PORT_GRPC=-1, **cenv)
        return w, c

    # completes
    w, c = pair("del-ok")
    try:
        t0 = time.time()
        s, t = run_stream(c, SLEEP.format(n=2), budget_tokens=100000)
        r = summary(t, s) | {"seconds": round(time.time() - t0, 1), "coord_tools": c.tools(), "worker_tools": w.tools()}
        out["delegate_ok"] = r
        print(f"-- delegate, budget 100000 tokens: {r['state']} in {r['seconds']}s; budget {r['budget']}; answer {r['answer']!r}", flush=True)
        for l in c.tools():
            print("     coord: " + l[:300], flush=True)
        check(r["state"] == "TASK_STATE_COMPLETED", "a coordinator with a budget delegates and completes")
        spent = ((r["budget"] or {}).get("spent") or {}).get("tokens")
        check(isinstance(spent, int) and 0 < spent < 100000, f"the coordinator's budget was charged what the worker spent (spent.tokens = {spent})")
    finally:
        c.stop(); w.stop()

    # small
    w, c = pair("del-small")
    try:
        t0 = time.time()
        s, t = run_stream(c, BILLING, budget_tokens=300)
        r = summary(t, s) | {"seconds": round(time.time() - t0, 1), "coord_tools": c.tools()}
        out["delegate_small"] = r
        print(f"-- delegate, budget 300 tokens: {r['state']} in {r['seconds']}s; {r['status_text']!r}; budget {r['budget']}", flush=True)
        for l in c.tools():
            print("     coord: " + l[:400], flush=True)
        check(r["state"] == "TASK_STATE_FAILED", "a budget too small for the worker's run fails the delegation")
    finally:
        c.stop(); w.stop()

    # worker killed
    w, c = pair("del-kill")
    try:
        s = Stream("jsonrpc", c.ports["rpc"], SLEEP.format(n=60), None, budget_tokens=100000)
        s.wait_task()
        wait_for(lambda: any("START sleep_work" in l for l in w.tools()), 180)
        t0 = time.time()
        w.proc.send_signal(signal.SIGKILL)
        w.proc.wait()
        s.done.wait(300)
        took = time.time() - t0
        task = s.task_id
        t = final(c.ports["rpc"], task)
        r = summary(t, s) | {"seconds_after_kill": round(took, 1), "coord_tools": c.tools()}
        out["delegate_worker_killed"] = r
        print(f"-- worker SIGKILLed mid-run: coordinator {r['state']} {took:.1f}s later; {r['status_text']!r}", flush=True)
        for l in c.tools():
            print("     coord: " + l[:400], flush=True)
        check(r["state"] in ("TASK_STATE_FAILED", "TASK_STATE_CANCELED") and took < 120, f"a killed worker ends the coordinator's task within 120 s ({r['state']}, {took:.1f}s)")
    finally:
        c.stop(); w.stop()

    # coordinator cancelled: cascade
    w, c = pair("del-cancel")
    try:
        s = Stream("jsonrpc", c.ports["rpc"], SLEEP.format(n=60), None, budget_tokens=100000)
        s.wait_task()
        wait_for(lambda: any("START sleep_work" in l for l in w.tools()), 180)
        t0 = time.time()
        cst, cbody = rpc(c.ports["rpc"], "CancelTask", {"id": s.task_id})
        s.done.wait(60)
        dropped = wait_for(lambda: any("DROP sleep_work" in l for l in w.tools()), 30)
        took = time.time() - t0
        t = final(c.ports["rpc"], s.task_id)
        wt = None
        r = summary(t, s) | {"seconds": round(took, 1), "worker_tool_dropped": bool(dropped), "coord_tools": c.tools()}
        out["delegate_cancel"] = r
        print(f"-- coordinator cancelled mid-run: coordinator {r['state']}; worker's tool dropped: {bool(dropped)} after {took:.1f}s", flush=True)
        for l in c.tools():
            print("     coord: " + l[:300], flush=True)
        check(bool(dropped), "cancelling the coordinator cancels the worker's run (its sleeping tool was dropped)")
    finally:
        c.stop(); w.stop()

    # a budget with a deadline only
    w, c = pair("del-time")
    try:
        t0 = time.time()
        s, t = run_stream(c, SLEEP.format(n=1), budget_ms=60000)
        r = summary(t, s) | {"seconds": round(time.time() - t0, 1), "coord_tools": c.tools()}
        out["delegate_time_only"] = r
        print(f"-- coordinator given a deadline only (grant asks tokens=u64::MAX): {r['state']}; {r['status_text']!r}", flush=True)
        for l in c.tools():
            print("     coord: " + l[:400], flush=True)
    finally:
        c.stop(); w.stop()

    save(f"08-budget-delegation-{os.environ.get('CAMPAIGN_MODEL_FILE', 'model')}.json", out)
    return finish()


if __name__ == "__main__":
    sys.exit(main())
