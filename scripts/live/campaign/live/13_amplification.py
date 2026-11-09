#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What a hostile or broken provider, and a tool that returns a lot, can make the agent hold.

Needs: the binaries of scripts/live/campaign/live/agent/, python3. No model (model_stub.py).

  calls     one reply with N tool calls, M turns: the agent's peak RSS (sampled every 100 ms) and the time. The
            provider's reply is kept under the 8 MiB response cap, which is the only bound on N
  results   a tool that returns 1, 4, 8, 16 and 20 MiB: the task state, the size of what the caller got back
            (the tool-trace artifact carries the result), the model's next request, the peak RSS, and the
            reason the agent logged when the task failed
Proves nothing; it measures. The numbers are what `README.md` quotes.
"""
import importlib.util
import json
import os
import re
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: F401,F403

_spec = importlib.util.spec_from_file_location("tr", str(HERE / "05_tool_results.py"))
tr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tr)


def peak_of(agent, stop, peak):
    while not stop.is_set():
        peak[0] = max(peak[0], agent.rss_kb())
        time.sleep(0.1)


def calls(n, turns):
    stub = tr.Stub("calls_many", n)
    try:
        with Agent("amp-calls", C_MODEL_URL=stub.url, C_TOOLS="inventory", C_PORT_RPC=0, C_PORT_REST=-1, C_PORT_GRPC=-1, C_MAX_TURNS=turns) as agent:
            start, peak, stop = agent.rss_kb(), [0], threading.Event()
            threading.Thread(target=peak_of, args=(agent, stop, peak), daemon=True).start()
            r = tr.send(agent, "go", timeout=900)
            stop.set()
            return {"calls_per_reply": n, "max_turns": turns, "state": r["state"], "seconds": r["seconds"], "rss_start_kb": start,
                    "rss_peak_kb": peak[0], "status": r["status_text"][:120]}
    finally:
        stub.stop()


def results(kb):
    r = tr.scenario(f"amp-result-{kb}", None, tools="big", env={"C_TELEMETRY": "1", "RUST_LOG": "warn"},
                    script=[{"tool_calls": [tr.call("big_result", kb=kb)]}, {"text": "done"}])
    trace = artifact_text(r["task"], "tool-trace") or ""
    second = [q for q in r["requests"] if q["turn"] == 2]
    reason = [re.sub(r"a2a\.(rpc|task)\{[^}]*\}", "", l)[:260] for l in r["stderr"].splitlines() if "executor failed" in l][:1]
    return {"tool_result_kib": kb, "state": r["state"], "seconds": r["seconds"], "task_body_bytes": r["bytes"], "trace_chars": len(trace),
            "second_model_request_bytes": len(json.dumps(second[0]["body"])) if second else 0, "rss_after_kb": r["rss_after_kb"],
            "status_text": r["status_text"], "logged_reason": reason}


def main():
    header = run_header("amplification: calls per reply, tool result size")
    out = {"header": header, "calls": [], "results": []}
    for n, turns in ((10_000, 8), (40_000, 3), (90_000, 4)):
        row = calls(n, turns)
        out["calls"].append(row)
        print(f"   {n} calls x {turns} turns: {row['state']} in {row['seconds']}s, RSS {row['rss_start_kb']} -> peak {row['rss_peak_kb']} kB", flush=True)
    for kb in (1024, 4096, 8192, 16384, 20480):
        row = results(kb)
        out["results"].append(row)
        print(f"   tool result {kb} KiB: {row['state']} in {row['seconds']}s; GetTask body {row['task_body_bytes']} B; trace {row['trace_chars']} chars; "
              f"model request {row['second_model_request_bytes']} B; {row['status_text'][:60]!r} {row['logged_reason']}", flush=True)
    save("13-amplification.json", out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
