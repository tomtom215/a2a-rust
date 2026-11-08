#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""How often does a real model end a tool-using run with an empty reply, and what does the caller get then?

Needs: llama-server on 127.0.0.1:11434 (the run header records which model), the binaries of
scripts/live/campaign/live/agent/, python3. Sends the README's billing question N times (default 12) and,
for each run, records the task state, the status text, the number of tool calls that really ran (the agent's own
log), and whether the task carries the tool-trace artifact. Audit M-1 made an empty final reply a failed task;
what this measures is what that does to a run that already made calls: the side effects ran, the answer is
gone, and the trace artifact (which says what ran) is written only for a completed task.

    12_empty_after_tool.py [N] [restart|idem]  `restart`: ask for a restart (a side effect) instead; `idem`: live-model's
                                               idempotency prompt (a restart, then the same restart again)
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 12
    restart = "restart" in sys.argv
    idem = "idem" in sys.argv  # examples/live-model's idempotency prompt
    prompt = ("Restart the checkout service. Then call restart_service for checkout a second time to make sure the restart took."
              if idem else "Restart the checkout service, then tell me what you did." if restart else "How is the billing service doing?")
    header = run_header(f"empty replies after tool calls: {n} runs of {prompt!r}", os.environ.get("CAMPAIGN_MODEL_FILE"))
    rows = []
    with Agent("empty-after-tool", C_MODEL_URL=MODEL, C_TOOLS="inventory", C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1,
               C_EXTRA_BODY='{"max_tokens":300}', C_MAX_TURNS=6) as agent:
        for i in range(n):
            agent.tool_log.write_text("")
            t0 = time.time()
            st, body = rpc(agent.ports["rpc"], "SendMessage", {"message": message(prompt)}, timeout=600)
            task = task_of(body.get("result", {}))
            calls = [l for l in agent.tools() if " CALL " in l or " RESTART " in l]
            row = {"run": i, "seconds": round(time.time() - t0, 1), "state": (task.get("status") or {}).get("state"),
                   "status_text": status_text(task), "tool_calls_that_ran": len(calls), "restarts": sum(1 for l in calls if " RESTART " in l),
                   "has_trace_artifact": artifact(task, "tool-trace") is not None, "has_answer": artifact(task, "answer") is not None}
            rows.append(row)
            print(f"   run {i}: {row['state']} {row['seconds']}s; tools that ran {row['tool_calls_that_ran']}; trace artifact {row['has_trace_artifact']}; {row['status_text']!r}", flush=True)
    failed = [r for r in rows if r["state"] == "TASK_STATE_FAILED"]
    lost = [r for r in failed if r["tool_calls_that_ran"] and not r["has_trace_artifact"]]
    print(f"   {len(failed)} of {n} failed; {len(lost)} of them had run tools and carry no trace of them", flush=True)
    save(f"12-empty-after-tool-{os.environ.get('CAMPAIGN_MODEL_FILE', 'model')}{'-idem' if idem else '-restart' if restart else ''}.json",
         {"header": header, "prompt": prompt, "rows": rows, "failed": len(failed), "failed_after_tools_without_trace": len(lost)})
    note(f"{len(failed)}/{n} runs failed ({[r['status_text'][:60] for r in failed][:2]}); {len(lost)} ran tools and left no trace artifact")
    return 0


if __name__ == "__main__":
    sys.exit(main())
