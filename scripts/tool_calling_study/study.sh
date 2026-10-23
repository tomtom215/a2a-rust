#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The whole study, two llama-servers at a time (four at once shared 15 GB
# with builds and slowed both), each wave's servers stopped before the
# next wave's start. Resumable: drive.sh skips finished cells and cell.py
# fills partial ones, so after a restart run this again.
#
#   study.sh LLAMA_SERVER MODELS_DIR OUT_DIR TOOL_LOOP_AGENT_BIN
#
# As run on 2026-10-20..22: LLAMA_SERVER=/opt/llama/src/build/bin/llama-server
# (llama.cpp 8466395, b11751), MODELS_DIR=/opt/models, the binary built from
# examples/tool-loop-agent in release (its SHA-256 is in each SDK cell).
set -euo pipefail
server=$1; models=$2; out=$3; bin=$4
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
serve() { nohup "$server" -m "$models/$1" --port "$2" -c 8192 -t 2 --jinja > "$out/server_$2.log" 2>&1 & echo $!; }
ready() { until curl -sf "localhost:$1/health" >/dev/null; do sleep 2; done; }
wave() {
  local a b
  a=$(serve "$1.gguf" "$2"); b=$(serve "$3.gguf" "$4"); ready "$2"; ready "$4"
  "$here/drive.sh" "$2" "$1" "$out" "$bin" > "$out/drive_$1.log" 2>&1 &
  "$here/drive.sh" "$4" "$3" "$out" "$bin" > "$out/drive_$3.log" 2>&1 &
  # The servers were started in command substitutions, so they are not
  # this shell's jobs: this waits for the two drivers alone.
  wait || true
  kill "$a" "$b"
}
wave qwen3.5-0.8b-q4_0 11440 qwen3.5-0.8b-q4_k_m 11441
wave qwen3-1.7b-q4_k_m 11442 qwen3.5-2b-q4_k_m 11443
echo "all waves done $(date +%T)"
