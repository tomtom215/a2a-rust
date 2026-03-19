#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Cross-language benchmark: Go A2A SDK
#
# Prerequisites:
#   1. Go 1.22+
#   2. The Go A2A SDK (github.com/a2aproject/a2a-go)
#   3. This script starts a Rust echo server for fair comparison
#
# Usage:
#   ./benches/scripts/cross_language_go.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="$SCRIPT_DIR/../results"
CROSS_LANG_DIR="$SCRIPT_DIR/../cross_language"
TIMESTAMP="$(date -u +%Y%m%d-%H%M%S)"
SERVER_PORT=13101

echo "=== Go A2A SDK Benchmark ==="
echo "NOTE: Implement the Go benchmark in benches/cross_language/bench_go.go"
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

# ── Run Go benchmarks ───────────────────────────────────────────────────────

GO_BENCH_FILE="$CROSS_LANG_DIR/bench_go.go"
if [ -f "$GO_BENCH_FILE" ]; then
    echo "Running Go benchmarks..."
    cd "$CROSS_LANG_DIR"
    go run bench_go.go --server "http://127.0.0.1:$SERVER_PORT" \
        --output "$RESULTS_DIR/go-$TIMESTAMP.json"
else
    echo "Go benchmark not yet implemented."
    echo "Create: $GO_BENCH_FILE"
    echo ""
    echo "Expected output format: see benches/scripts/compare_results.sh"

    mkdir -p "$RESULTS_DIR"
    cat > "$RESULTS_DIR/go-$TIMESTAMP.json" <<EOF
{
  "language": "go",
  "sdk": "a2a-go",
  "timestamp": "$TIMESTAMP",
  "workloads": {},
  "note": "Template — implement bench_go.go"
}
EOF
fi

echo "=== Go benchmark complete ==="
