#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# A client that disconnects mid-stream: what does the agent do with the task?
# (The book says the task goes on running: "the task on the server is not
# cancelled, so resubscribe".) Against echo-agent's slow path (400 ms of work,
# no model needed): the `a2a stream` process is killed with SIGKILL after its
# first event, then the task is read back and subscribed to.
#
# Needs: 03-build-examples.sh done; ports 3111; python3.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
T="$CARGO_TARGET_DIR/debug"; A="$T/a2a"
export NO_PROXY='*' no_proxy='*'
env -i PATH="$PATH" A2A_BIND_ADDR=127.0.0.1:3111 "$T/echo-agent" >/tmp/disc-echo.log 2>&1 & pid=$!
trap 'kill $pid 2>/dev/null' EXIT
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:3111/.well-known/agent-card.json && break; sleep 0.2; done
U=http://127.0.0.1:3111

for round in 1 2 3; do
  echo "--- round $round: stream a slow task, kill the client after the first event"
  python3 -I - "$A" "$U" <<'PY'
import json, signal, subprocess, sys, time
a, u = sys.argv[1], sys.argv[2]
p = subprocess.Popen([a, "stream", u, "slow:disconnect me"], stdout=subprocess.PIPE, text=True)
first = p.stdout.readline()
p.send_signal(signal.SIGKILL); p.wait()
tid = json.loads(first)["task"]["id"]
print("first event:", first.strip()[:140])
print("killed the client; task id", tid)
for i in range(12):
    time.sleep(0.2)
    out = subprocess.run([a, "task", "get", u, tid], capture_output=True, text=True).stdout
    st = json.loads(out)["status"]["state"] if out.strip() else "?"
    print(f"  +{(i+1)*0.2:.1f}s task state: {st}")
    if st in ("TASK_STATE_COMPLETED", "TASK_STATE_FAILED", "TASK_STATE_CANCELED"):
        break
res = subprocess.run([a, "task", "get", u, tid], capture_output=True, text=True).stdout
d = json.loads(res)
print("artifacts after the disconnect:", [x["parts"][0].get("text") for x in d.get("artifacts", [])])
PY
done
echo "--- many abrupt disconnects at once (50 streams killed after the first byte), then does the agent still answer?"
python3 -I - "$A" "$U" <<'PY'
import signal, subprocess, sys, time
a, u = sys.argv[1], sys.argv[2]
ps = [subprocess.Popen([a, "stream", u, "slow:x"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True) for _ in range(50)]
for p in ps:
    p.stdout.readline()
    p.send_signal(signal.SIGKILL)
for p in ps:
    p.wait()
time.sleep(1)
r = subprocess.run([a, "send", u, "still alive?"], capture_output=True, text=True, timeout=20)
print("send after the burst: exit", r.returncode, "|", "Echo: still alive?" in r.stdout)
PY
echo "--- the agent's own log (errors from the disconnects?)"
grep -ciE 'error|warn|panic' /tmp/disc-echo.log; grep -iE 'error|warn|panic' /tmp/disc-echo.log | head -5 | cut -c1-240
