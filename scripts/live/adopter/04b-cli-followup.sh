#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Follow-ups to 04-cli.sh that need their own process handling:
#   * SIGINT during a stream (a background `&` job ignores SIGINT in a
#     non-interactive shell, so 04-cli.sh's attempt proved nothing),
#   * how long a stalled stream holds the CLI (the library's idle timeout),
#   * a hard OTEL configuration error (the README says exit 2),
#   * a 100 KB message argument and a 20 MiB body POSTed straight to the server,
#   * the `--send-timeout` message.
#
# Needs: 03-build-examples.sh done, python3, curl; ports 3111 and 3401-3409 free.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
T="$CARGO_TARGET_DIR/debug"; A="$T/a2a"
W="$ADOPTER_SCRATCH/cli-work2"; rm -rf "$W"; mkdir -p "$W"; cd "$W"
export NO_PROXY='*' no_proxy='*'
env -i PATH="$PATH" A2A_BIND_ADDR=127.0.0.1:3111 "$T/echo-agent" >"$W/echo.log" 2>&1 & ECHO=$!
STUB_LOG="$W/stub.log" python3 -I "$REPO/scripts/live/adopter/cli_stub.py" 3400 >"$W/stub.out" 2>&1 & STUB=$!
trap 'kill $ECHO $STUB 2>/dev/null' EXIT
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:3111/.well-known/agent-card.json && break; sleep 0.2; done
for _ in $(seq 1 100); do curl -s -o /dev/null http://127.0.0.1:3405/.well-known/agent-card.json && break; sleep 0.2; done

echo "##### SIGINT during a stream (python spawns it with the default SIGINT disposition)"
python3 -I - "$A" <<'PY'
import signal, subprocess, sys, time
a = sys.argv[1]
p = subprocess.Popen([a, "stream", "http://127.0.0.1:3405", "hi", "--timeout", "30"],
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
time.sleep(2)
p.send_signal(signal.SIGINT)
try:
    out, err = p.communicate(timeout=10)
    print("exit:", p.returncode, "(a death by SIGINT is -2; 130 is the shell's rendering)")
except subprocess.TimeoutExpired:
    p.kill(); out, err = p.communicate()
    print("DID NOT EXIT within 10 s of SIGINT; killed")
print("stdout:", out.strip()[:300])
print("stderr:", err.strip()[:300])
PY

echo "##### SIGINT during a blocking send to a half-open peer"
python3 -I - "$A" <<'PY'
import signal, subprocess, sys, time
p = subprocess.Popen([sys.argv[1], "send", "http://127.0.0.1:3404", "hi", "--binding", "jsonrpc"],
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
time.sleep(2); p.send_signal(signal.SIGINT)
try:
    out, err = p.communicate(timeout=10); print("exit:", p.returncode)
except subprocess.TimeoutExpired:
    p.kill(); out, err = p.communicate(); print("DID NOT EXIT within 10 s of SIGINT; killed")
print("stderr:", err.strip()[:300])
PY

echo "##### a hard OTEL error: the README says exit 2 with one error line"
for v in "OTEL_EXPORTER_OTLP_PROTOCOL=carrier-pigeon" "OTEL_EXPORTER_OTLP_ENDPOINT=not-a-url" "OTEL_EXPORTER_OTLP_TIMEOUT=abc" "OTEL_EXPORTER_OTLP_HEADERS=broken" "OTEL_SDK_DISABLED=maybe"; do
  echo "--- $v"
  env "$v" "$A" card http://127.0.0.1:3111 >"$W/o" 2>"$W/e"; echo "exit $? ; stdout $(wc -c <"$W/o") bytes"; head -c 400 "$W/e"; echo
done

echo "##### 100 KB argument (under the 128 KiB single-argument limit) and above it"
t100=$(head -c 100000 /dev/zero | tr '\0' 'x')
"$A" send http://127.0.0.1:3111 "$t100" >"$W/o" 2>"$W/e"; echo "100 KB: exit $? ; stdout $(wc -c <"$W/o") bytes"; head -c 300 "$W/e"
t200=$(head -c 200000 /dev/zero | tr '\0' 'x')
"$A" send http://127.0.0.1:3111 "$t200" >"$W/o" 2>"$W/e"; echo "200 KB: exit $? ; $(head -c 200 "$W/e")"
"$A" send http://127.0.0.1:3111 - >"$W/o" 2>"$W/e" <<<"from stdin"; echo "text '-' : exit $? ; $(grep -o 'Echo: [^"]*' "$W/o" | head -1)"

echo "##### the server's body limit: 20 MiB POSTed straight to the echo agent (REST and JSON-RPC)"
python3 -I -c 'import sys; sys.stdout.write("{\"message\":{\"messageId\":\"big\",\"role\":\"ROLE_USER\",\"parts\":[{\"text\":\"" + "x"*(20*1024*1024) + "\"}]}}")' > big.json
curl -s -o "$W/o" -w 'REST  : HTTP %{http_code} in %{time_total}s\n' -X POST http://127.0.0.1:3111/message:send -H 'content-type: application/json' -H 'A2A-Version: 1.0' --data-binary @big.json; head -c 300 "$W/o"; echo
curl -s -o "$W/o" -w 'JSONRPC: HTTP %{http_code} in %{time_total}s\n' -X POST http://127.0.0.1:3111/ -H 'content-type: application/json' -H 'A2A-Version: 1.0' --data-binary @big.json; head -c 300 "$W/o"; echo

echo "##### a stalled stream: how long until the CLI gives up? (library idle default: 5 min)"
s=$(date +%s)
timeout 400 "$A" stream http://127.0.0.1:3405 hi --timeout 3 >"$W/o" 2>"$W/e"; rc=$?
echo "exit $rc after $(( $(date +%s) - s )) s"; echo "stdout:"; cat "$W/o"; echo "stderr:"; head -c 600 "$W/e"
