#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The tool-loop page's "A minimal agent" as a stranger would build it:
#   attempt 1  only the dependency line the page gives (`features = ["tool-loop"]`),
#              with the rustdoc-hidden `fn main` lines restored (the page hides them);
#   attempt 2  plus the crates the code names and the page does not (tokio, serde_json).
# Then, attempt 2 is served on port 3000 against the llama-server on 11434 and
# asked a question through the `a2a` CLI.
#
# Needs: 01-package.sh done; 03-build-examples.sh done (the a2a binary);
# a llama-server on 127.0.0.1:11434 for the run; ports 3000.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export ADOPTER_SCRATCH
P="python3 -I $REPO/scripts/live/adopter/mkproject.py"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0; unset RUSTFLAGS
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/_gs-target"
PAGE=book/src/building-agents/tool-loop.md
echo "##### attempt 1: the page's dependency line alone"
$P tool-loop-page-1 --deps $PAGE:toml:0 --main $PAGE:rust:0 --show-hidden || exit 1
(cd "$ADOPTER_SCRATCH/adopter-projects/tool-loop-page-1" && cargo build 2>&1 | grep -E '^(error|warning)|-->|^\s+Finished' | head -20)
echo "##### attempt 2: plus tokio and serde_json"
$P tool-loop-page-2 --deps $PAGE:toml:0 --main $PAGE:rust:0 --show-hidden \
  --extra-dep 'tokio = { version = "1", features = ["rt-multi-thread", "macros"] }' \
  --extra-dep 'serde_json = "1"' || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/tool-loop-page-2" || exit 1
# The page points at llama-server's default port; this box's is 11434. The one edit.
sed -i 's|127.0.0.1:8080|127.0.0.1:11434|' src/main.rs
cargo build 2>&1 | grep -E '^(error|warning)|-->|^\s+Finished' | head -20
[ -x "$CARGO_TARGET_DIR/debug/tool-loop-page-2" ] || { echo "no binary"; exit 1; }
curl -s --noproxy '*' http://127.0.0.1:11434/health; echo
"$CARGO_TARGET_DIR/debug/tool-loop-page-2" >agent.out 2>&1 & pid=$!
for _ in $(seq 1 100); do curl -s --noproxy '*' -o /dev/null http://127.0.0.1:3000/.well-known/agent-card.json && break; sleep 0.2; done
echo "--- a2a card"; "$ADOPTER_SCRATCH/target-adopter/debug/a2a" card http://127.0.0.1:3000 2>&1 | head -12
echo "--- a2a send (what time is it?)"
s=$(date +%s)
NO_PROXY='*' "$ADOPTER_SCRATCH/target-adopter/debug/a2a" send http://127.0.0.1:3000 "What time is it in UTC? Use your tool." --send-timeout 900 2>&1 | head -60
echo "took $(( $(date +%s) - s )) s"
kill -TERM $pid; wait $pid; echo "agent exit: $?"; cat agent.out | head
