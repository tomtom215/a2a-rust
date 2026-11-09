#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The one delegation of 08_budget_delegation.py whose grant asks for tokens = u64::MAX of a budget that has
only a deadline, on its own: a coordinator (live-model's) and a real worker with a real model.

Needs: llama-server on 127.0.0.1:11434, the binaries of scripts/live/campaign/live/agent/, python3.
Before the fix (types, `BudgetHandle::grant`) the delegation failed to start with
`[-32602] message.metadata holds the integer 18446744073709551615`; after it the worker is granted 2^53.
"""
import importlib.util
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403

_spec = importlib.util.spec_from_file_location("bd", str(HERE / "08_budget_delegation.py"))
bd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bd)


def main():
    header = run_header("a grant that asks for u64::MAX tokens of a deadline-only budget", os.environ.get("CAMPAIGN_MODEL_FILE"))
    w = bd.worker("time-worker")
    c = Agent("time-coord", C_MODE="coordinator", C_WORKER_URL=f"http://127.0.0.1:{w.ports['rpc']}", C_PORT_RPC=0,
              C_PORT_REST=-1, C_PORT_GRPC=-1)
    try:
        s, t = bd.run_stream(c, "Say the word OK.", budget_ms=60000)
        r = bd.summary(t, s)
        print(f"-- {r['state']}; {r['status_text']!r}; answer {r['answer']!r}", flush=True)
        for l in c.tools():
            print("     coord: " + l[:300], flush=True)
        check(r["state"] == "TASK_STATE_COMPLETED", f"the delegation with a deadline-only budget completes ({r['state']})")
    finally:
        c.stop()
        w.stop()
    save("08b-grant-time-only.json", {"header": header, "result": r, "coord": c.tools()})
    return finish()


if __name__ == "__main__":
    sys.exit(main())
