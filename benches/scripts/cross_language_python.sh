#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Cross-language benchmark: this Rust SDK's server vs the OFFICIAL Python SDK's
# server, answering one question — what does the A2A layer itself cost per
# request, and is that cost large enough to matter?
#
# Design, and why it is built this way:
#
#   * ONE client drives both servers. It is a raw socket sending pre-serialized,
#     byte-identical HTTP requests (benches/scripts/cross_language_bench.py).
#     Using either SDK's own client would measure that SDK on both sides of the
#     comparison and make the two legs incomparable.
#
#   * BOTH servers run the same echo contract — `Echo: <text>` returned as a
#     completed task artifact — one from examples/echo-agent (this SDK), one
#     from itk/agents/python-sdk/agent.py (the official `a2a-sdk`). Those two
#     agents already exist for interoperability testing; neither was written
#     for this benchmark.
#
#   * The Python server runs uvicorn's FAST path. `uvicorn[standard]` pulls in
#     uvloop and httptools, and uvicorn's default loop/http setting is "auto",
#     which selects them when present. Measuring plain asyncio + h11 would
#     understate the official SDK and make the result a strawman.
#
#   * TWO CPU configurations are measured, because either one alone invites a
#     misreading:
#       pinned   - each server confined to a single distinct core. Removes the
#                  "Rust used four cores, one uvicorn worker used one" objection.
#                  This is the like-for-like number.
#       default  - nothing pinned; each server as its own docs say to run it.
#                  This is the out-of-the-box number.
#
#   * A `floor` target (a server returning a constant, doing no A2A work) is
#     measured so the report can separate SDK cost from client-and-kernel cost.
#     It is a floor for SEQUENTIAL LATENCY ONLY; see the harness docstring.
#
# Usage: benches/scripts/cross_language_python.sh
# Env:   TRIALS, ITERATIONS, WARMUP, CONCURRENCY, PER_CONN to override sizing.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RESULTS_DIR="$REPO_ROOT/benches/results"
VENV_DIR="${A2A_BENCH_VENV:-$REPO_ROOT/target/bench-venv}"
REQ_FILE="$REPO_ROOT/benches/requirements-cross-language.txt"

TRIALS="${TRIALS:-3}"
ITERATIONS="${ITERATIONS:-2000}"
WARMUP="${WARMUP:-300}"
CONCURRENCY="${CONCURRENCY:-50}"
PER_CONN="${PER_CONN:-40}"

RUST_PORT="${RUST_PORT:-19120}"
PY_PORT="${PY_PORT:-19110}"

PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT

command -v taskset >/dev/null || { echo "taskset required (util-linux)"; exit 1; }
NCPU="$(nproc)"
[ "$NCPU" -ge 4 ] || { echo "need >= 4 logical CPUs, found $NCPU"; exit 1; }

# ── Build both servers ──────────────────────────────────────────────────────

echo "==> building echo-agent (release)"
cargo build -p echo-agent --release --quiet

echo "==> preparing Python environment"
if [ ! -x "$VENV_DIR/bin/python" ]; then
    python3 -m venv "$VENV_DIR"
fi
"$VENV_DIR/bin/pip" install -q --disable-pip-version-check -r "$REQ_FILE"
FREEZE_FILE="$(mktemp)"
"$VENV_DIR/bin/pip" freeze > "$FREEZE_FILE"

# Fail loudly rather than silently benchmarking uvicorn's slow path.
grep -qi '^uvloop==' "$FREEZE_FILE" || { echo "uvloop missing: would understate the Python SDK"; exit 1; }
grep -qi '^httptools==' "$FREEZE_FILE" || { echo "httptools missing: would understate the Python SDK"; exit 1; }

wait_ready() {
    local port="$1" name="$2"
    curl -sS --retry 40 --retry-connrefused --retry-delay 1 --max-time 60 \
        -o /dev/null -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' \
        -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"configuration":{"historyLength":0},"message":{"messageId":"ready","role":"ROLE_USER","parts":[{"text":"hi"}]}}}' \
        "http://127.0.0.1:$port/" || { echo "$name never became ready on $port"; exit 1; }
}

run_config() {
    local label="$1" rust_pin="$2" py_pin="$3" client_pin="$4"
    echo ""
    echo "==> configuration: $label"

    # shellcheck disable=SC2086
    A2A_BIND_ADDR="127.0.0.1:$RUST_PORT" \
        $rust_pin "$REPO_ROOT/target/release/echo-agent" >/dev/null 2>&1 &
    PIDS+=("$!")
    # shellcheck disable=SC2086
    PORT="$PY_PORT" $py_pin "$VENV_DIR/bin/python" \
        "$REPO_ROOT/itk/agents/python-sdk/agent.py" >/dev/null 2>&1 &
    PIDS+=("$!")

    wait_ready "$RUST_PORT" "rust echo-agent"
    wait_ready "$PY_PORT" "python-sdk agent"

    local out="$RESULTS_DIR/cross-language-$label.json"
    # shellcheck disable=SC2086
    $client_pin "$VENV_DIR/bin/python" "$SCRIPT_DIR/cross_language_bench.py" \
        --target "rust-a2a-protocol-server=127.0.0.1:$RUST_PORT" \
        --target "official-python-a2a-sdk=127.0.0.1:$PY_PORT" \
        --warmup "$WARMUP" --iterations "$ITERATIONS" --trials "$TRIALS" \
        --concurrency "$CONCURRENCY" --concurrent-per-conn "$PER_CONN" \
        --pinned "$label: rust='$rust_pin' python='$py_pin' client='$client_pin'" \
        --note "uvicorn loop/http = auto (uvloop + httptools installed and asserted present)" \
        --env-file "$FREEZE_FILE" \
        --out "$out"

    cleanup
    PIDS=()
}

mkdir -p "$RESULTS_DIR"

run_config "pinned"  "taskset -c 0" "taskset -c 1" "taskset -c 2,3"
run_config "default" ""             ""             ""

rm -f "$FREEZE_FILE"
echo ""
echo "=== cross-language benchmark complete ==="
echo "Results: $RESULTS_DIR/cross-language-pinned.json"
echo "         $RESULTS_DIR/cross-language-default.json"
echo "Render:  benches/scripts/generate_cross_language_page.sh"
