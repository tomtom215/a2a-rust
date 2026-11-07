#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# tool-loop-agent against model_stub.py's twelve scripted failures: a malformed
# tool call, an unknown tool, wrong-shaped arguments, an answer cut by the token
# limit, a 500 carrying a secret, a 429, a non-JSON 200, a 6 MiB answer, an endless
# tool-call loop, an empty reply, a tool call written as text, and a model that
# never answers. For each: the task state, the answer, the exit code and the
# seconds; then what the SDK sent back to the model after the bad turn.
#
# Needs: 03-build-examples.sh done (tool-loop-agent); python3; ports 3501-3512.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
T="$CARGO_TARGET_DIR/debug"
export NO_PROXY='*' no_proxy='*'
W="$ADOPTER_SCRATCH/model-stub-work"; rm -rf "$W"; mkdir -p "$W"
MODEL_STUB_LOG="$W/stub.log" python3 -I "$REPO/scripts/live/adopter/model_stub.py" 3500 >"$W/stub.out" 2>&1 & stub=$!
trap 'kill $stub 2>/dev/null' EXIT
for _ in $(seq 1 50); do (exec 3<>/dev/tcp/127.0.0.1/3512) 2>/dev/null && break; sleep 0.2; done
names=("" "truncated JSON arguments" "unknown tool" "arguments are an array" "cut by the token limit (finish_reason length)" "HTTP 500 carrying a secret" "HTTP 429" "HTTP 200, not JSON" "a 6 MiB answer" "the same tool call forever" "an empty reply" "a tool call written as text" "never answers")
for k in $(seq 1 12); do
  echo "=== scenario $k: ${names[$k]}"
  s=$(date +%s)
  TOOL_LOOP_BASE_URL=http://127.0.0.1:$((3500 + k))/v1 TOOL_LOOP_MODEL_TIMEOUT_SECS=8 \
    timeout 120 "$T/tool-loop-agent" "How is the checkout service doing?" >"$W/out" 2>"$W/err"; rc=$?
  echo "--- stdout"; head -c 900 "$W/out"; echo
  echo "--- stderr (without the telemetry notice)"; grep -v 'telemetry: nothing is exported' "$W/err" | head -c 700; echo
  echo "--- exit $rc in $(( $(date +%s) - s )) s; requests the stub saw: $(grep -c "\"scenario\": $k," "$W/stub.log")"
  echo
done
echo "##### what the SDK sent the model after the bad turn (scenarios 1-3: the last request's tool message)"
python3 -I - "$W/stub.log" <<'PY'
import json, sys
last = {}
for line in open(sys.argv[1]):
    r = json.loads(line)
    last[r["scenario"]] = r
for k in (1, 2, 3, 11):
    r = last.get(k)
    if not r: continue
    msgs = r["body"].get("messages", [])
    tool = [m for m in msgs if m.get("role") == "tool"]
    print(f"scenario {k}: turn {r['turn']}, {len(msgs)} messages; tool message(s): {[m.get('content') for m in tool]}")
PY
