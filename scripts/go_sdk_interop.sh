#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Runs this SDK against the official Go SDK (a2a-go, pinned in the two go.mod
# files below) in BOTH directions, over JSON-RPC, HTTP+JSON and gRPC:
#
#   1. a2a-go CLIENT  -> this repo's SERVER  (itk/interop/go-sdk-client
#                                              against examples/echo-agent)
#   2. this repo's CLIENT -> a2a-go SERVER   (the harness's go_sdk_interop
#                                              against itk/agents/go-sdk)
#
# Why this exists: before it, CI ran a2a-go only as a server, driven by the
# in-repo TCK, which deliberately does not use a2a-protocol-client. The
# client a Rust coordinator calls Go agents with, the server a Go client
# calls, and every gRPC path between the two had never met in CI. The
# 2026-09-22 audit then found five defects that each side's own tests passed
# (T1, S2, S7, C8, C9, C10 in its numbering) — the class of defect only a
# real peer finds.
#
# Exits 0 only if both directions pass. Each side prints its own [ok]/[FAIL]
# lines; this script adds only the start/stop plumbing and the verdict.
#
# Usage: scripts/go_sdk_interop.sh
# Requires: cargo, go (the version go.mod names, or GOTOOLCHAIN=auto).
# Env: INTEROP_PROFILE=release|debug (default release, what CI builds).
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PROFILE=${INTEROP_PROFILE:-release}
case "$PROFILE" in
    release) CARGO_PROFILE_FLAG=--release ; TARGET_SUBDIR=release ;;
    debug)   CARGO_PROFILE_FLAG=         ; TARGET_SUBDIR=debug ;;
    *) echo "INTEROP_PROFILE must be release or debug, got $PROFILE" >&2; exit 2 ;;
esac
TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
BIN="$TARGET_DIR/$TARGET_SUBDIR"

# Fixed loopback ports: a CI runner is single-tenant, and a port that is
# already taken fails loudly at bind rather than being mistaken for a peer.
RUST_HTTP=127.0.0.1:19310
RUST_GRPC=127.0.0.1:19311
GO_HTTP_PORT=19312
GO_GRPC_PORT=19313

WORK=$(mktemp -d "${TMPDIR:-/tmp}/a2a-go-interop.XXXXXX")
PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

# Polls an agent card until it answers, so a slow start is waited for and a
# dead peer is reported as such rather than as a protocol failure.
await_card() {
    local url=$1 name=$2 log=$3
    for _ in $(seq 1 240); do
        if curl -fsS -o /dev/null "$url/.well-known/agent-card.json"; then
            return 0
        fi
        sleep 0.5
    done
    echo "::error::$name never served its agent card at $url; its log follows" >&2
    cat "$log" >&2
    return 1
}

echo "── build ───────────────────────────────────────────────────────────────"
cargo build $CARGO_PROFILE_FLAG -p echo-agent -p a2a-example-harness --bin echo-agent --bin go_sdk_interop
( cd itk/interop/go-sdk-client && go build -o "$WORK/go-sdk-client" . )
( cd itk/agents/go-sdk && go build -o "$WORK/go-sdk-agent" . )

verdict=0

echo "── 1. a2a-go client -> Rust server ─────────────────────────────────────"
A2A_BIND_ADDR=$RUST_HTTP A2A_GRPC_BIND_ADDR=$RUST_GRPC A2A_INTEROP_CARD=1 \
    "$BIN/echo-agent" >"$WORK/rust-server.log" 2>&1 &
PIDS+=($!)
await_card "http://$RUST_HTTP" "echo-agent" "$WORK/rust-server.log"
if ! "$WORK/go-sdk-client" "http://$RUST_HTTP" "JSONRPC,HTTP+JSON,GRPC"; then
    echo "::error::a2a-go client against the Rust server failed" >&2
    verdict=1
fi

echo "── 2. Rust client -> a2a-go server ─────────────────────────────────────"
PORT=$GO_HTTP_PORT GRPC_PORT=$GO_GRPC_PORT INTEROP_CARD=1 \
    "$WORK/go-sdk-agent" >"$WORK/go-server.log" 2>&1 &
PIDS+=($!)
await_card "http://127.0.0.1:$GO_HTTP_PORT" "go-sdk agent" "$WORK/go-server.log"
if ! "$BIN/go_sdk_interop" "http://127.0.0.1:$GO_HTTP_PORT"; then
    echo "::error::the Rust client against the a2a-go server failed" >&2
    verdict=1
fi

if [ "$verdict" -ne 0 ]; then
    echo "── server logs ─────────────────────────────────────────────────────────"
    echo "--- echo-agent ---"; tail -n 50 "$WORK/rust-server.log"
    echo "--- go-sdk agent ---"; tail -n 50 "$WORK/go-server.log"
    echo "Go SDK interop: FAILED"
else
    echo "Go SDK interop: both directions passed"
fi
exit "$verdict"
