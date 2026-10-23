#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# One llama-server's four cells (direct and SDK arms, thinking off and on),
# strictly in sequence: concurrent requests on one llama-server are batched
# together, which changes the arithmetic and breaks replay (measured: 0 of 8
# identical concurrently, 8 of 8 sequentially).
#
# A cell whose file holds its 60 runs is skipped, and cell.py runs only a
# partial cell's missing seeds, so this is safe to run again after a
# restart: it resumes.
#
#   drive.sh PORT LABEL OUT_DIR TOOL_LOOP_AGENT_BIN
set -euo pipefail
port=$1; label=$2; out=$3; bin=$4
here=$(cd "$(dirname "$0")" && pwd)
done60() {
  python3 -I -c "import json,sys; sys.exit(0 if len(json.load(open(sys.argv[1]))['runs'])>=60 else 1)" "$1" 2>/dev/null
}
for th in off on; do
  f=$out/direct_${label}_$th.json
  done60 "$f" && { echo "skip $f"; continue; }
  python3 -I "$here/cell.py" --out "$f" --n 30 --seed-base 1000 -- \
    python3 -I "$here/run.py" --port "$port" --label "$label" --thinking "$th"
done
for th in off on; do
  f=$out/sdk_${label}_$th.json
  done60 "$f" && { echo "skip $f"; continue; }
  (cd "$here" && python3 -I cell.py --out "$f" --n 30 --seed-base 1000 -- \
    python3 -I sdk_arm.py --bin "$bin" --port "$port" --label "$label" --thinking "$th")
done
echo "$label done $(date +%T)"
