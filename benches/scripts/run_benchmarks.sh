#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
#
# Run all a2a-rust criterion benchmarks and collect results.
#
# Usage:
#   ./benches/scripts/run_benchmarks.sh              # Run all benchmarks
#   ./benches/scripts/run_benchmarks.sh --save        # Run and save baseline
#   ./benches/scripts/run_benchmarks.sh --compare     # Run and compare to baseline
#   ./benches/scripts/run_benchmarks.sh --bench NAME  # Run a specific benchmark
#
# Prerequisites:
#   - Rust toolchain (stable)
#   - Optional: cargo-criterion (`cargo install cargo-criterion`)
#
# Output:
#   - Criterion HTML reports in target/criterion/
#   - Baseline data in target/criterion/*/base/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RESULTS_DIR="$SCRIPT_DIR/../results"
TIMESTAMP="$(date -u +%Y%m%d-%H%M%S)"

cd "$REPO_ROOT"

# ── Parse arguments ──────────────────────────────────────────────────────────

SAVE_BASELINE=false
COMPARE_BASELINE=false
SPECIFIC_BENCH=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --save)
            SAVE_BASELINE=true
            shift
            ;;
        --compare)
            COMPARE_BASELINE=true
            shift
            ;;
        --bench)
            SPECIFIC_BENCH="$2"
            shift 2
            ;;
        -h|--help)
            head -18 "$0" | tail -14
            exit 0
            ;;
        *)
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

# ── Benchmark list ───────────────────────────────────────────────────────────

BENCHMARKS=(
    transport_throughput
    protocol_overhead
    task_lifecycle
    concurrent_agents
    cross_language
    realistic_workloads
    error_paths
    backpressure
    data_volume
    memory_overhead
    enterprise_scenarios
)

if [[ -n "$SPECIFIC_BENCH" ]]; then
    BENCHMARKS=("$SPECIFIC_BENCH")
fi

# ── Run benchmarks ───────────────────────────────────────────────────────────

echo "=== a2a-rust Benchmark Suite ==="
echo "Timestamp: $TIMESTAMP"
echo "Benchmarks: ${BENCHMARKS[*]}"
echo ""

BASELINE_ARGS=""
if $SAVE_BASELINE; then
    BASELINE_ARGS="-- --save-baseline base"
    echo "Mode: Saving baseline"
elif $COMPARE_BASELINE; then
    BASELINE_ARGS="-- --baseline base"
    echo "Mode: Comparing to baseline"
else
    echo "Mode: Standard run"
fi

for bench in "${BENCHMARKS[@]}"; do
    echo ""
    echo "── Running: $bench ─────────────────────────────────────────"
    # shellcheck disable=SC2086
    cargo bench -p a2a-benchmarks --bench "$bench" $BASELINE_ARGS
done

# ── Collect results ──────────────────────────────────────────────────────────

echo ""
echo "── Collecting results ───────────────────────────────────────────"

mkdir -p "$RESULTS_DIR"

# Export a summary JSON with key metrics
SUMMARY_FILE="$RESULTS_DIR/rust-$TIMESTAMP.json"
cat > "$SUMMARY_FILE" <<EOF
{
  "language": "rust",
  "sdk": "a2a-protocol-sdk",
  "version": "0.3.0",
  "timestamp": "$TIMESTAMP",
  "rust_version": "$(rustc --version)",
  "platform": "$(uname -s)-$(uname -m)",
  "note": "See target/criterion/ for full HTML reports"
}
EOF

echo "Summary written to: $SUMMARY_FILE"
echo "HTML reports in:     target/criterion/"
echo ""
echo "=== Benchmark suite complete ==="
