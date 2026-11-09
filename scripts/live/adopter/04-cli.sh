#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The `a2a` command-line client against a running example agent and against a
# misbehaving stub: every subcommand, exit codes, bad input, no network, wrong
# URL, a 401, a 429, a 500, a half-open peer, a stalled and a cut stream, and a
# check that a secret given on the command line is not printed.
#
# Needs: 03-build-examples.sh done (a2a, echo-agent), python3, openssl, curl;
# ports 3111 and 3401-3409 free. Output goes to stdout: a transcript, one
# block per command (command, stdout, stderr, exit code, seconds).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
T="$CARGO_TARGET_DIR/debug"
A="$T/a2a"
W="$ADOPTER_SCRATCH/cli-work"
rm -rf "$W"; mkdir -p "$W"; cd "$W"
export NO_PROXY='*' no_proxy='*'   # loopback only; keep the sandbox proxy out of it

t() { # label, then the command
  local label=$1; shift
  echo "=== $label"
  echo "\$ $*"
  local s e
  s=$(date +%s.%N)
  timeout 90 "$@" >"$W/out" 2>"$W/err"
  local rc=$?
  e=$(date +%s.%N)
  echo "--- stdout ($(wc -c <"$W/out") bytes)"; head -c "${MAXOUT:-1200}" "$W/out"; echo
  echo "--- stderr ($(wc -c <"$W/err") bytes)"; head -c 1200 "$W/err"; echo
  printf -- "--- exit %s in %.2fs\n\n" "$rc" "$(echo "$e - $s" | bc)"
}

echo "##### start the agents"
env -i PATH="$PATH" A2A_BIND_ADDR=127.0.0.1:3111 "$T/echo-agent" >"$W/echo-agent.log" 2>&1 &
ECHO_PID=$!
STUB_LOG="$W/stub.log" python3 -I "$REPO/scripts/live/adopter/cli_stub.py" 3400 >"$W/stub.out" 2>&1 &
STUB_PID=$!
trap 'kill $ECHO_PID $STUB_PID 2>/dev/null' EXIT
for _ in $(seq 1 100); do
  curl -s -o /dev/null http://127.0.0.1:3111/.well-known/agent-card.json && break; sleep 0.2
done
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:3405/.well-known/agent-card.json && break; sleep 0.2; done
head -5 "$W/echo-agent.log"; cat "$W/stub.out"
U=http://127.0.0.1:3111

echo "##### help, version, usage errors"
t "version" "$A" --version
t "help" "$A" --help
MAXOUT=2500 t "send help" "$A" send --help
t "no args" "$A"
t "unknown subcommand" "$A" frobnicate
t "completions (not a subcommand)" "$A" completions bash
t "--json (not a flag)" "$A" send $U hi --json
t "send without text" "$A" send $U
t "send empty text" "$A" send $U ""
t "bad URL: no scheme" "$A" card 127.0.0.1:3111
t "bad URL: garbage" "$A" card "not a url"
t "timeout 0" "$A" card $U --timeout 0
t "header without =" "$A" card $U --header novalue
t "header-env unset var" "$A" card $U --header-env X-A=NO_SUCH_VAR_A2A
t "binding unknown" "$A" card $U --binding carrier-pigeon

echo "##### against echo-agent on $U"
MAXOUT=800 t "card" "$A" card $U
t "send" "$A" send $U "hello from the walk-through"
t "send --binding rest" "$A" send $U "over rest" --binding rest
t "send --binding jsonrpc" "$A" send $U "over jsonrpc" --binding jsonrpc
t "stream" "$A" stream $U "stream me"
t "send --no-wait slow" "$A" send $U "slow:hold on" --no-wait
TASK=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["task"]["id"])' "$W/out" 2>/dev/null || true)
echo "task id: $TASK"
t "task get" "$A" task get $U "$TASK"
t "task subscribe (ends when task ends)" "$A" task subscribe $U "$TASK"
t "task cancel" "$A" task cancel $U "$TASK"
t "task cancel again (terminal)" "$A" task cancel $U "$TASK"
t "task get unknown" "$A" task get $U no-such-task
t "task list" "$A" task list $U --page-size 2
t "task list context filter (none)" "$A" task list $U --context-id nope
t "task list bad page token" "$A" task list $U --page-token "!!!"
t "send with --context-id" "$A" send $U "ctx" --context-id my-context
t "send --tenant" "$A" send $U "tenant" --tenant acme
t "send unicode+newline" "$A" send $U $'héllo 世界\nline two'
# A message above 128 KiB cannot be one argument (execve): see 04b-cli-followup.sh.

echo "##### bindings other than the card's: grpc, websocket (echo-agent serves JSON-RPC and REST here)"
t "grpc to a port with nothing" "$A" send 127.0.0.1:3111 "x" --binding grpc --grpc-plaintext
t "websocket to an HTTP port" "$A" send ws://127.0.0.1:3111 "x" --binding websocket

echo "##### no network, wrong URL"
t "closed port" "$A" card http://127.0.0.1:1
t "closed port, --binding" "$A" send http://127.0.0.1:1 hi --binding jsonrpc
t "unresolvable name" "$A" card http://no-such-host.invalid
t "wrong path (404 card)" "$A" card $U/nothing/here
t "card URL given as-is" "$A" card $U/.well-known/agent-card.json
t "https to an http port" "$A" card https://127.0.0.1:3111
t "http to a TLS-less gRPC port" "$A" card http://127.0.0.1:3111/ --timeout 2

echo "##### a 401, a 429, a 500, a non-JSON 200 (stub ports 3401, 3402, 3403, 3408)"
SECRET='Bearer s3cr3t-token-do-not-print'
t "401 via card" "$A" send http://127.0.0.1:3401 hi
t "401 via --binding, with a secret header" "$A" send http://127.0.0.1:3401 hi --binding jsonrpc --header "Authorization=$SECRET"
echo "stub saw: $(grep -c s3cr3t "$W/stub.log") request(s) carrying the secret"
export A2A_AUTH="$SECRET"
t "401 with --header-env" "$A" send http://127.0.0.1:3401 hi --binding jsonrpc --header-env Authorization=A2A_AUTH
t "429" "$A" send http://127.0.0.1:3402 hi --binding jsonrpc
t "429 card" "$A" card http://127.0.0.1:3402
t "500 html body" "$A" send http://127.0.0.1:3403 hi --binding jsonrpc
t "200 not JSON" "$A" send http://127.0.0.1:3408 hi --binding jsonrpc
t "rest 401" "$A" send http://127.0.0.1:3401 hi --binding rest --header "Authorization=$SECRET"
t "userinfo in the URL (printed?)" "$A" card http://user:pw-in-url-123@127.0.0.1:3401
t "secret in the URL's query" "$A" card "http://127.0.0.1:3401/?api_key=qs-secret-456"
for f in out err; do :; done

echo "##### half-open peer (3404 never answers) and stalled / cut streams"
t "half-open, --timeout 3" "$A" send http://127.0.0.1:3404 hi --binding jsonrpc --timeout 3 --send-timeout 3
t "half-open card, --timeout 3" "$A" card http://127.0.0.1:3404 --timeout 3
t "stream: one event then a stall (3405), --timeout 3" "$A" stream http://127.0.0.1:3405 hi --timeout 3
t "stream: cut mid-event (3409)" "$A" stream http://127.0.0.1:3409 hi
t "credential to a card naming another host (3407)" "$A" send http://127.0.0.1:3407 hi --header "Authorization=$SECRET"
t "same, with --follow-card-origin" "$A" send http://127.0.0.1:3407 hi --header "Authorization=$SECRET" --follow-card-origin --timeout 3

echo "##### offline: card sign and verify, audit verify"
openssl genpkey -algorithm ed25519 -outform DER -out key.der 2>/dev/null
openssl pkey -inform DER -in key.der -pubout -outform DER -out pub.der 2>/dev/null
python3 -I - <<'PY'
import base64, json
pub = open("pub.der", "rb").read()[-32:]
x = base64.urlsafe_b64encode(pub).rstrip(b"=").decode()
json.dump({"keys": [{"kty": "OKP", "crv": "Ed25519", "x": x, "kid": "k1", "alg": "EdDSA", "use": "sig"}]}, open("jwks.json", "w"))
PY
"$A" card $U > card.json 2>/dev/null
t "card sign" "$A" card sign card.json --key key.der --alg eddsa --kid k1
cp "$W/out" signed.json
t "card verify (good)" "$A" card verify signed.json --jwks jwks.json
python3 -I -c 'import json; d=json.load(open("signed.json")); d["name"]="Tampered"; json.dump(d, open("tampered.json","w"))'
t "card verify (tampered)" "$A" card verify tampered.json --jwks jwks.json
t "card verify (unsigned card)" "$A" card verify card.json --jwks jwks.json
t "card verify (missing file)" "$A" card verify nope.json --jwks jwks.json
t "card verify (wrong key)" "$A" card verify signed.json --jwks <(python3 -I -c 'import json; print(json.dumps({"keys":[{"kty":"OKP","crv":"Ed25519","x":"A"*43,"kid":"k1","alg":"EdDSA"}]}))')
t "card sign (key file is not a key)" "$A" card sign card.json --key card.json --alg eddsa --kid k1
t "audit verify (missing file)" "$A" audit verify nope.jsonl
echo '{}' > bad-records.json
t "audit verify (not records)" "$A" audit verify bad-records.json

echo "##### telemetry: OTEL endpoint with nothing listening must not break or hang the command"
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 t "send with an unreachable collector" "$A" send $U "otel"
OTEL_TRACES_EXPORTER=bogus t "send with an unusable OTEL setting" "$A" send $U "otel"

echo "##### SIGINT during a stream"
( "$A" stream http://127.0.0.1:3405 hi --timeout 30 >"$W/sigint.out" 2>"$W/sigint.err" & echo $! >"$W/sigint.pid"; wait $! ; echo "exit $?" >"$W/sigint.rc" ) &
sleep 2; kill -INT "$(cat "$W/sigint.pid")"; sleep 1
echo "stdout:"; cat "$W/sigint.out"; echo "stderr:"; cat "$W/sigint.err"; echo "rc:"; cat "$W/sigint.rc" 2>/dev/null
echo "##### done"
