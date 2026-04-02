#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Generate the interactive benchmark dashboard from criterion results.
#
# Usage:
#   ./benches/scripts/generate_dashboard.sh
#
# Reads:
#   - target/criterion/         (criterion JSON results)
#   - benches/dashboard/template.html (dashboard HTML template)
#
# Writes:
#   - book/src/reference/benchmark-dashboard.html
#
# The template contains a "__BENCHMARK_DATA__" placeholder that gets replaced
# with structured JSON extracted from the criterion output by
# extract_benchmark_json.py.
#
# Prerequisites:
#   - Run benchmarks first: cargo bench -p a2a-benchmarks
#   - python3 (for JSON extraction and template injection)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CRITERION_DIR="$REPO_ROOT/target/criterion"
TEMPLATE_FILE="$REPO_ROOT/benches/dashboard/template.html"
OUTPUT_FILE="$REPO_ROOT/book/src/reference/benchmark-dashboard.html"
EXTRACTOR="$SCRIPT_DIR/extract_benchmark_json.py"

# ── Validate prerequisites ───────────────────────────────────────────────

if [ ! -d "$CRITERION_DIR" ]; then
    echo "Error: No criterion results found at $CRITERION_DIR" >&2
    echo "Run benchmarks first: cargo bench -p a2a-benchmarks" >&2
    exit 1
fi

if [ ! -f "$TEMPLATE_FILE" ]; then
    echo "Error: Dashboard template not found at $TEMPLATE_FILE" >&2
    exit 1
fi

if [ ! -f "$EXTRACTOR" ]; then
    echo "Error: JSON extractor not found at $EXTRACTOR" >&2
    exit 1
fi

python3 --version > /dev/null 2>&1 || {
    echo "Error: python3 is required for JSON extraction" >&2
    exit 1
}

# ── Extract benchmark data ───────────────────────────────────────────────

echo "Extracting benchmark data from criterion results..."
TEMP_JSON=$(mktemp)
trap 'rm -f "$TEMP_JSON"' EXIT

python3 "$EXTRACTOR" --criterion-dir "$CRITERION_DIR" --output "$TEMP_JSON"

if [ ! -s "$TEMP_JSON" ]; then
    echo "Error: No benchmark data extracted (empty output)" >&2
    exit 1
fi

# Count benchmarks for logging
BENCH_COUNT=$(python3 -c "
import json, sys
with open('$TEMP_JSON') as f:
    data = json.load(f)
print(data['metadata']['total_benchmarks'])
")
echo "  Extracted $BENCH_COUNT benchmarks"

# ── Inject data into template ────────────────────────────────────────────

echo "Generating dashboard..."
mkdir -p "$(dirname "$OUTPUT_FILE")"

# Use python for reliable string replacement (avoids shell quoting issues
# with large JSON containing special characters).
python3 -c "
import sys

with open('$TEMPLATE_FILE') as f:
    template = f.read()

with open('$TEMP_JSON') as f:
    data = f.read().strip()

# Replace the placeholder string (including surrounding quotes)
output = template.replace('\"__BENCHMARK_DATA__\"', data)

# Verify replacement happened
if '__BENCHMARK_DATA__' in output:
    print('Error: placeholder replacement failed', file=sys.stderr)
    sys.exit(1)

with open('$OUTPUT_FILE', 'w') as f:
    f.write(output)
"

echo "Dashboard generated: $OUTPUT_FILE"
echo "  Benchmarks: $BENCH_COUNT"
echo "  Template:   $(basename "$TEMPLATE_FILE")"
echo "  Output:     $OUTPUT_FILE"
