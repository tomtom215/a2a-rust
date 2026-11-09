#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A delegation to a child that reports an absurd spend, and a grant with an unsendable meter.

Needs: the binaries of scripts/live/campaign/live/agent/ (coordinator mode), python3. No model: the child is
fake_worker.py, a scripted peer.

  huge       the child's final status says spent.tokens = u64::MAX: does the coordinator's task end?
  sane       the same child reporting 120: completes, the coordinator is charged 120 (control)
  timeonly   a coordinator given a deadline and no `tokens` meter, whose grant asks for tokens = u64::MAX
Proves (or fails to) that a peer's report cannot leave the parent's task `working`, and records what the
delegation does with a meter past 2^53.
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403

FAKE_PORT = 3811


def fake(spent):
    p = subprocess.Popen([sys.executable, "-I", str(HERE / "fake_worker.py"), str(FAKE_PORT), spent],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.8)
    return p


def trial(spent, label, **send):
    p = fake(spent)
    try:
        with Agent(f"hostile-{label}", C_MODE="coordinator", C_WORKER_URL=f"http://127.0.0.1:{FAKE_PORT}", C_PORT_RPC=0,
                   C_PORT_REST=-1, C_PORT_GRPC=-1, C_TELEMETRY="1", **send.pop("env", {})) as agent:
            st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message("go", **send)}, timeout=60)
            task = task_of(body.get("result", {}))
            first = (task.get("status") or {}).get("state")
            time.sleep(3)
            later = task_of(rpc(agent.ports["rpc"], "GetTask", {"id": task.get("id")})[1].get("result", {})) if task.get("id") else {}
            err = [l for l in agent.stderr().splitlines() if "ERROR" in l]
            import re
            return {"first": first, "after_3s": (later.get("status") or {}).get("state"), "status_text": status_text(later) or status_text(task),
                    "rpc_error": body.get("error"), "coord_log": agent.tools(),
                    "errors": [re.sub(r"a2a\.(rpc|task)\{[^}]*\}", "", l)[:300] for l in err[:4]]}
    finally:
        p.terminate()


def main():
    header = run_header("a delegation to a child that reports an absurd spend")
    out = {"header": header}
    for label, spent in (("sane", "120"), ("huge", "18446744073709551615")):
        r = trial(spent, label, budget_tokens=100000)
        out[label] = r
        print(f"-- child reports spent.tokens={spent}: coordinator {r['first']} then {r['after_3s']}; {r['status_text']!r}", flush=True)
        for l in r["coord_log"]:
            print("     coord: " + l[:260], flush=True)
        for l in r["errors"]:
            print("     ERROR: " + l, flush=True)
    check(out["sane"]["after_3s"] == "TASK_STATE_COMPLETED", "control: a child reporting 120 tokens completes the delegation")
    check(out["huge"]["after_3s"] in ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED"),
          f"a child reporting spent = u64::MAX leaves the coordinator's task terminal (it is {out['huge']['after_3s']})")
    r = trial("120", "timeonly", budget_ms=60000)
    out["timeonly"] = r
    print(f"-- deadline-only budget, grant asks tokens=u64::MAX: {r['first']} then {r['after_3s']}; {r['status_text']!r}; error {r['rpc_error']}", flush=True)
    for l in r["coord_log"]:
        print("     coord: " + l[:300], flush=True)
    save("09-hostile-child.json", out)
    return finish()


if __name__ == "__main__":
    sys.exit(main())
