#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Reproduces every measurement in the SDK comparison, in order.
# Prereqs (see README.md): Rust stable, python3, uv, protoc (vendored path
# below), llama.cpp llama-server + Qwen3-0.6B-Q8_0.gguf on :8080, oha 1.10.0,
# ghz 0.121.0, and checkouts at the pinned commits:
#   $BENCH/a2a-itk        a2aproject/a2a-itk   429945f
#   $BENCH/a2a-rs-acts    a2aproject/a2a-rs    365d056  (a2a-lf 0.3.1 / a2a-server-lf 0.4.4 / a2a-client-lf 0.2.5 / a2a-grpc 0.3.7)
#   $BENCH/a2a-rust-acts  tomtom215/a2a-rust   10f3435  (a2a-protocol-* 0.14.0)
set -u
H=$(cd "$(dirname "$0")" && pwd)
BENCH=${BENCH:-/opt/bench}
R=${RESULTS:-$BENCH/results}; export RESULTS=$R BENCH; mkdir -p "$R"/{acts,interop,probes,robust,perf}
export CARGO_TARGET_DIR=$H/target
export PROTOC=${PROTOC:-$(find ~/.cargo/registry/src -path '*protoc-bin-vendored-linux-x86_64-3.2.0/bin/protoc' | head -1)}
REPS=${REPS:-3}

for p in agent-rust agent-rs driver-rust driver-rs; do (cd "$H/$p" && cargo build --release); done
BIN=$CARGO_TARGET_DIR/release

# 1. ACTS (official conformance corpus) against each project's own ITK agent.
for rep in $(seq 1 "$REPS"); do
  for sdk in a2a-rust a2a-rs; do
    ( cd "$BENCH/a2a-itk" && uv run python run_acts.py --mount "$BENCH/$sdk-acts/itk" --sdk "$sdk" \
        --language rust --transport all --out "$R/acts/$sdk-rep$rep" ) > "$R/acts/$sdk-rep$rep.log" 2>&1
  done
done

# 2. Cross-SDK interop matrix, echo and real-model modes.
for rep in $(seq 1 "$REPS"); do
  "$H/run_matrix.sh" echo "$R/interop/echo-rep$rep.jsonl"
  LLM_MAX_TOKENS=48 "$H/run_matrix.sh" llm "$R/interop/llm-rep$rep.jsonl"
done

# 3. Raw-wire spec probes against the neutral agents.
for rep in $(seq 1 "$REPS"); do
  AGENT_MODE=echo "$BIN/agent-rust" 2>/dev/null & AGENT_MODE=echo "$BIN/agent-rs" 2>/dev/null & sleep 1.5
  { python3 "$H/probe.py" a2a-rust http://127.0.0.1:7101 http://127.0.0.1:7101/ http://127.0.0.1:7102
    python3 "$H/probe.py" a2a-rs http://127.0.0.1:7201 http://127.0.0.1:7201/jsonrpc http://127.0.0.1:7201/rest
  } > "$R/probes/probes-rep$rep.jsonl"
  pkill -f "$BIN/agent-"; sleep 0.5
done

# 4. Robustness battery, one server at a time.
AGENT_MODE=echo "$BIN/agent-rust" 2>/dev/null & P=$!; sleep 1
python3 "$H/robust.py" a2a-rust $P http://127.0.0.1:7101/; kill $P; sleep 0.5
AGENT_MODE=echo "$BIN/agent-rs" 2>/dev/null & P=$!; sleep 1
python3 "$H/robust.py" a2a-rs $P http://127.0.0.1:7201/jsonrpc; kill $P

# 4b. Isolated memory per held stream (fresh process per run).
for rep in 1 2; do for s in "rust http://127.0.0.1:7101/" "rs http://127.0.0.1:7201/jsonrpc"; do
  set -- $s; AGENT_MODE=echo "$BIN/agent-$1" >/dev/null 2>&1 & P=$!; sleep 1
  python3 "$H/streams_mem.py" "a2a-$1" $P "$2" 1000 >> "$R/robust/streams-mem.jsonl"; kill $P; wait $P 2>/dev/null
done; done

# 5. Model-in-the-loop latency (idle machine only).
AGENT_MODE=llm "$BIN/agent-rust" 2>/dev/null & AGENT_MODE=llm "$BIN/agent-rs" 2>/dev/null & sleep 1.5
python3 "$H/ttft.py" 24 > "$R/ttft.jsonl"; pkill -f "$BIN/agent-"

# 6. SDK overhead load test (idle machine only; pins CPUs 0-1 / 2-3).
REPS=$REPS BIN=$BIN "$H/perf.sh"
