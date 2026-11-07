#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Run the examples that need nothing beyond this machine, the way their READMEs
# say, and compare what they print with what the READMEs claim.
#   hello-agent    README curl, the card, SIGTERM
#   deploy-agent   the README's five curls, /health, /ready, SIGTERM and the drain
#   echo-agent     a default run: exit code and the coverage matrix
#
# Usage: 03-examples-offline.sh [hello|deploy|echo|all]
# Needs: 03-build-examples.sh done; ports 3000, 8080 free; curl, python3.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
T="$CARGO_TARGET_DIR/debug"
export NO_PROXY='*' no_proxy='*'
which_=${1:-all}
wait_for() { for _ in $(seq 1 100); do curl -s -o /dev/null "$1" && return 0; sleep 0.2; done; return 1; }

if [ "$which_" = hello ] || [ "$which_" = all ]; then
  echo "##### hello-agent"
  "$T/hello-agent" >/tmp/hello.out 2>/tmp/hello.err & pid=$!
  wait_for http://127.0.0.1:3000/.well-known/agent-card.json || echo "NEVER CAME UP"
  echo "--- README curl"
  curl -sS -X POST http://127.0.0.1:3000 -H 'content-type: application/json' -H 'A2A-Version: 1.0' \
    -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Tom"}]}}}'; echo
  echo "--- card"; curl -sS http://127.0.0.1:3000/.well-known/agent-card.json | head -c 600; echo
  echo "--- GET / (a browser tab)"; curl -s -o /dev/null -w 'HTTP %{http_code}\n' http://127.0.0.1:3000/
  kill -TERM $pid; wait $pid; echo "exit after SIGTERM: $?"
  echo "--- stdout"; cat /tmp/hello.out; echo "--- stderr"; head -c 600 /tmp/hello.err
fi

if [ "$which_" = deploy ] || [ "$which_" = all ]; then
  echo "##### deploy-agent (no database)"
  env -i PATH="$PATH" DRAIN_DELAY_SECONDS=3 "$T/deploy-agent" >/tmp/deploy.out 2>/tmp/deploy.err & pid=$!
  wait_for http://127.0.0.1:8080/health || echo "NEVER CAME UP"
  for p in /health /ready /.well-known/agent-card.json; do
    echo "--- GET $p"; curl -sS -i http://127.0.0.1:8080$p | head -c 900; echo
  done
  echo "--- POST /message:send (README)"
  curl -sS -X POST localhost:8080/message:send -H 'content-type: application/json' -H 'A2A-Version: 1.0' \
    -d '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}'; echo
  echo "--- POST / JSON-RPC"
  curl -sS -X POST localhost:8080/ -H 'content-type: application/json' -H 'A2A-Version: 1.0' \
    -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m2","role":"ROLE_USER","parts":[{"text":"Ada"}]}}}'; echo
  echo "--- SIGTERM; readiness while draining (3 s)"
  kill -TERM $pid; sleep 1
  curl -s -o /dev/null -w '/ready during drain: HTTP %{http_code}\n' http://127.0.0.1:8080/ready
  curl -s -o /dev/null -w '/health during drain: HTTP %{http_code}\n' http://127.0.0.1:8080/health
  wait $pid; echo "exit after SIGTERM: $?"
  echo "--- stdout"; cat /tmp/deploy.out; echo "--- stderr"; head -c 1500 /tmp/deploy.err
  echo "--- bad configuration is refused at startup"
  for v in PORT=0 PORT=99999 PORT=abc AGENT_URL=ftp://x AGENT_VERSION="a b" TASK_RETENTION_HOURS=1.5; do
    env -i PATH="$PATH" "$v" "$T/deploy-agent" >/tmp/c.out 2>/tmp/c.err & p=$!
    sleep 1; if kill -0 $p 2>/dev/null; then echo "$v: STILL RUNNING (not refused)"; kill $p; else wait $p; echo "$v: exit $? : $(head -c 200 /tmp/c.err | tr '\n' ' ')"; fi
  done
fi

if [ "$which_" = echo ] || [ "$which_" = all ]; then
  echo "##### echo-agent (default run)"
  s=$(date +%s)
  env -i PATH="$PATH" "$T/echo-agent" >/tmp/echo.out 2>/tmp/echo.err; echo "exit: $? in $(( $(date +%s) - s )) s"
  cat /tmp/echo.out | head -120; echo "--- stderr"; head -c 800 /tmp/echo.err
fi
