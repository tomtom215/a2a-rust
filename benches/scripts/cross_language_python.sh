#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Cross-language benchmark: Python A2A SDK
#
# Prerequisites:
#   1. Start the Rust echo server:
#      cargo run -p echo-agent  (or use A2A_BIND_ADDR=127.0.0.1:3000)
#   2. Install the Python A2A SDK:
#      pip install a2a-sdk
#   3. Install benchmark dependencies:
#      pip install time-machine  # or just use stdlib timeit
#
# This script starts a Rust echo server, runs the canonical workloads
# against it from Python, and writes results to benches/results/.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="$SCRIPT_DIR/../results"
TIMESTAMP="$(date -u +%Y%m%d-%H%M%S)"
SERVER_PORT=13100

echo "=== Python A2A SDK Benchmark ==="

# ── Start Rust echo server ───────────────────────────────────────────────────

echo "Starting Rust echo server on port $SERVER_PORT..."
A2A_BIND_ADDR="127.0.0.1:$SERVER_PORT" cargo run -p echo-agent --release &
SERVER_PID=$!
sleep 2

cleanup() {
    echo "Stopping echo server (PID $SERVER_PID)..."
    kill "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ── Run Python benchmarks ────────────────────────────────────────────────────

PYTHON_BENCH=$(cat <<'PYEOF'
import json
import time
import statistics
import sys

# These would use the actual Python A2A SDK.
# For now, this is a template showing the expected structure.

SERVER_URL = f"http://127.0.0.1:{sys.argv[1]}"
WARMUP = 50
ITERATIONS = 500

def measure(fn, warmup=WARMUP, iterations=ITERATIONS):
    """Run fn with warmup, return median/p95/p99 in microseconds."""
    for _ in range(warmup):
        fn()
    times = []
    for _ in range(iterations):
        start = time.perf_counter_ns()
        fn()
        elapsed = time.perf_counter_ns() - start
        times.append(elapsed / 1000)  # to microseconds
    times.sort()
    n = len(times)
    return {
        "median_us": int(statistics.median(times)),
        "p95_us": int(times[int(n * 0.95)]),
        "p99_us": int(times[int(n * 0.99)]),
    }

print("NOTE: Python A2A SDK benchmarks require 'a2a-sdk' package.")
print("      Install it and uncomment the workload implementations below.")
print("      This template outputs placeholder values.")

results = {
    "language": "python",
    "sdk": "a2a-sdk-python",
    "timestamp": time.strftime("%Y%m%d-%H%M%S", time.gmtime()),
    "python_version": sys.version,
    "workloads": {
        "echo_roundtrip":        {"median_us": 0, "p95_us": 0, "p99_us": 0},
        "stream_events":         {"median_us": 0, "p95_us": 0, "p99_us": 0},
        "serialize_agent_card":  {"median_us": 0, "p95_us": 0, "p99_us": 0},
        "concurrent_50":         {"median_us": 0, "p95_us": 0, "p99_us": 0},
        "minimal_overhead":      {"median_us": 0, "p95_us": 0, "p99_us": 0},
    },
    "note": "Template — replace with actual SDK calls"
}

json.dump(results, sys.stdout, indent=2)
PYEOF
)

mkdir -p "$RESULTS_DIR"
OUTPUT_FILE="$RESULTS_DIR/python-$TIMESTAMP.json"

python3 -c "$PYTHON_BENCH" "$SERVER_PORT" > "$OUTPUT_FILE"

echo ""
echo "Results written to: $OUTPUT_FILE"
echo "=== Python benchmark complete ==="
