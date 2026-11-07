#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The two details examples/hello-agent/README.md says the JSON-RPC binding will
# not let a reader omit, and what a browser tab gets: run them and print the
# answers so the README's quoted error texts can be compared with the real ones.
#
# Needs: 03-build-examples.sh done; port 3000 free; curl.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
export NO_PROXY='*' no_proxy='*'
"$CARGO_TARGET_DIR/debug/hello-agent" >/dev/null 2>&1 & pid=$!
trap 'kill $pid 2>/dev/null' EXIT
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:3000/.well-known/agent-card.json && break; sleep 0.2; done
post() { curl -sS -w '\n[HTTP %{http_code}]\n' -X POST localhost:3000/ -H 'content-type: application/json' "$@"; }
echo "--- README request, README header"
post -H 'A2A-Version: 1.0' -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}}'
echo "--- no A2A-Version header"
post -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}}'
echo "--- 'content' where the wire says 'parts'"
post -H 'A2A-Version: 1.0' -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m1","role":"ROLE_USER","content":[{"text":"Ada"}]}}}'
echo "--- the REST path against a JSON-RPC-only agent"
curl -sS -w '\n[HTTP %{http_code}]\n' -X POST localhost:3000/message:send -H 'content-type: application/json' -H 'A2A-Version: 1.0' -d '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Ada"}]}}'
echo "--- GET / (a browser tab)"
curl -sS -i localhost:3000/ | head -12
echo "--- a method that does not exist"
post -H 'A2A-Version: 1.0' -d '{"jsonrpc":"2.0","id":7,"method":"message/send","params":{}}'
echo "--- not JSON"
post -H 'A2A-Version: 1.0' -d 'hello'
echo "--- wrong content type"
curl -sS -w '\n[HTTP %{http_code}]\n' -X POST localhost:3000/ -H 'content-type: text/plain' -H 'A2A-Version: 1.0' -d '{}'
