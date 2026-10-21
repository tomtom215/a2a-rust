# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The SDK arm of docs/tool-calling-study.md: the same prompts, sampler and
seeds as run.py, asked through this SDK end to end (examples/tool-loop-agent:
an A2A client, the A2A server, ToolLoopExecutor, ToolLoop, OpenAiChatModel).

Each run starts the example's release binary once, which serves the agent on
a loopback port, sends the prompt as a streaming SendMessage, and prints the
task's state, its `answer` artifact and its `tool-trace` artifact; this
parses them into run.py's shape so score.py reads both arms alike.

Usage:
  sdk_arm.py --bin PATH --port P --label L --thinking on|off --n N --seed-base S --out FILE

Differences from run.py, by construction, recorded in the output: one seed
for every turn of a run (OpenAiChatModel sends one fixed body; run.py varies
it per turn), the server's own tool-call ids (run.py fixes them), and the
SDK's turn bound of 8 (rig-agent's is 6).
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time

# run.py beside this file, by path: `python3 -I` leaves the script's own
# directory off sys.path, and the prompts and sampler must be run.py's own.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run import PROMPTS, SAMPLER, TURN_TIMEOUT_SECS  # noqa: E402

TRACE_LINE = re.compile(r"^(?P<name>[A-Za-z_][\w-]*)\((?P<args>.*)\) -> (?P<result>.*)$")


def parse(stdout):
    """The state, the answer and the trace from the example's output."""
    state = None
    sections = {}
    current = None
    for line in stdout.splitlines():
        if line.startswith("state: "):
            state = line.removeprefix("state: ").strip()
            current = None
        elif line in ("[ answer ]", "[ tool-trace ]"):
            current = line.strip("[] ")
            sections[current] = []
        elif current:
            sections[current].append(line)
    trace = []
    for line in sections.get("tool-trace", []):
        m = TRACE_LINE.match(line)
        if not m:
            continue
        try:
            args = json.loads(m["args"]) if m["args"].strip() else {}
        except json.JSONDecodeError:
            args = None
        trace.append({"name": m["name"], "arguments": args, "result": m["result"]})
    answer = "\n".join(sections["answer"]) if "answer" in sections else None
    return state, answer, trace


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--thinking", choices=("on", "off"), required=True)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--seed-base", type=int, required=True)
    ap.add_argument("--max-tokens", type=int, default=4096)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    with open(__file__, "rb") as f:
        harness = hashlib.sha256(f.read()).hexdigest()
    with open(a.bin, "rb") as f:
        binary = hashlib.sha256(f.read()).hexdigest()
    out = {"arm": "sdk", "label": a.label, "thinking": a.thinking, "n": a.n,
           "seed_base": a.seed_base, "max_tokens": a.max_tokens, "sampler": SAMPLER,
           "harness_sha256": harness, "binary_sha256": binary,
           "turn_timeout_secs": TURN_TIMEOUT_SECS, "runs": []}
    for k, kind in enumerate(PROMPTS):
        for i in range(a.n):
            seed = a.seed_base + 1000 * k + i
            extra = {"chat_template_kwargs": {"enable_thinking": a.thinking == "on"},
                     "seed": seed, "max_tokens": a.max_tokens, "cache_prompt": False, **SAMPLER}
            # The direct arm's urlopen waits 1800 s for a turn; OpenAiChatModel's
            # default is 120 s, and on a loaded machine a slow thinking turn
            # failed the SDK arm alone (2 runs on 2026-10-20), a skew the
            # study exists to rule out. Both arms now wait the same.
            env = dict(os.environ, TOOL_LOOP_BASE_URL=f"http://127.0.0.1:{a.port}/v1",
                       TOOL_LOOP_MODEL=a.label, TOOL_LOOP_THINKING="1",
                       TOOL_LOOP_MODEL_TIMEOUT_SECS=str(TURN_TIMEOUT_SECS),
                       TOOL_LOOP_EXTRA_BODY=json.dumps(extra))
            t0 = time.monotonic()
            proc = subprocess.run([a.bin, PROMPTS[kind]], env=env, capture_output=True,
                                  text=True, timeout=3600)
            state, answer, trace = parse(proc.stdout)
            completed = state == "TASK_STATE_COMPLETED" or state == "completed"
            out["runs"].append({
                "kind": kind, "seed": seed, "trace": trace,
                "final": answer if completed else None,
                "state": state, "exit": proc.returncode,
                "server_error": (not completed and "turns without answering" not in proc.stderr),
                "stderr": proc.stderr[-500:], "secs": round(time.monotonic() - t0, 1)})
            with open(a.out, "w") as f:
                json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
