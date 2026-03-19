#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Cross-language benchmark: JavaScript/TypeScript A2A SDK
#
# Prerequisites:
#   1. Node.js 20+
#   2. The JS A2A SDK (@anthropic-ai/a2a-sdk or equivalent)
#   3. This script starts a Rust echo server for fair comparison
#
# Usage:
#   ./benches/scripts/cross_language_js.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="$SCRIPT_DIR/../results"
CROSS_LANG_DIR="$SCRIPT_DIR/../cross_language"
TIMESTAMP="$(date -u +%Y%m%d-%H%M%S)"
SERVER_PORT=13102

echo "=== JavaScript A2A SDK Benchmark ==="
echo "NOTE: Implement the JS benchmark in benches/cross_language/bench_js.mjs"
echo "      This script is a template for the expected workflow."
echo ""

# ── Start Rust echo server ───────────────────────────────────────────────────

echo "Starting Rust echo server on port $SERVER_PORT..."
A2A_BIND_ADDR="127.0.0.1:$SERVER_PORT" cargo run -p echo-agent --release &
SERVER_PID=$!
sleep 2

cleanup() {
    kill "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ── Run JS benchmarks ───────────────────────────────────────────────────────

JS_BENCH_FILE="$CROSS_LANG_DIR/bench_js.mjs"
if [ -f "$JS_BENCH_FILE" ]; then
    echo "Running JavaScript benchmarks..."
    node "$JS_BENCH_FILE" \
        --server "http://127.0.0.1:$SERVER_PORT" \
        --output "$RESULTS_DIR/javascript-$TIMESTAMP.json"
else
    echo "JavaScript benchmark not yet implemented."
    echo "Create: $JS_BENCH_FILE"
    echo ""
    echo "Expected output format: see benches/scripts/compare_results.sh"

    mkdir -p "$RESULTS_DIR"
    cat > "$RESULTS_DIR/javascript-$TIMESTAMP.json" <<EOF
{
  "language": "javascript",
  "sdk": "a2a-sdk-js",
  "timestamp": "$TIMESTAMP",
  "workloads": {},
  "note": "Template — implement bench_js.mjs"
}
EOF
fi

echo "=== JavaScript benchmark complete ==="
