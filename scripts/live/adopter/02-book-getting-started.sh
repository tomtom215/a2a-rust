#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The book's Getting-started pages, typed as written into fresh projects that
# depend on the unpacked tarballs (01-package.sh):
#   installation-verify   Installation, "Verifying the Installation"
#   quickstart-hello      Quick Start, "The Smallest Agent" (dependency lines
#                         taken from the Installation page: the page gives none)
#   my-agent              Your First Agent: Steps 1-3 as src/main.rs, Step 4 as
#                         src/bin/client.rs; agent started, client run
#
# Needs: 01-package.sh done; crates.io; port 3000 free. One shared target dir
# (the three projects use the same dependencies), deleted by the caller.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export ADOPTER_SCRATCH
P="python3 -I $REPO/scripts/live/adopter/mkproject.py"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
unset RUSTFLAGS
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/_gs-target"

echo "##### installation-verify"
$P installation-verify --deps book/src/getting-started/installation.md:toml:0 \
  --main book/src/getting-started/installation.md:rust:0 || exit 1
(cd "$ADOPTER_SCRATCH/adopter-projects/installation-verify" && cargo run 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)'; echo "exit: ${PIPESTATUS[0]}")

echo "##### quickstart-hello"
$P quickstart-hello --deps book/src/getting-started/installation.md:toml:0 \
  --main book/src/getting-started/quick-start.md:rust:0 || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/quickstart-hello"
cargo build 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)'
"$CARGO_TARGET_DIR/debug/quickstart-hello" > run.out 2>&1 &
pid=$!
for _ in $(seq 1 50); do curl -s --noproxy '*' -o /dev/null http://127.0.0.1:3000/.well-known/agent-card.json && break; sleep 0.2; done
echo "--- curl from the page"
curl -sS --noproxy '*' -X POST http://127.0.0.1:3000 \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Tom"}]}}}'
echo
echo "--- without the A2A-Version header (the page says VERSION_NOT_SUPPORTED)"
curl -sS --noproxy '*' -X POST http://127.0.0.1:3000 -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m2","role":"ROLE_USER","parts":[{"text":"Tom"}]}}}'
echo
echo "--- SIGTERM"
kill -TERM $pid; wait $pid; echo "exit after SIGTERM: $?"; cat run.out

echo "##### my-agent"
$P my-agent --deps book/src/getting-started/first-agent.md:toml:0 \
  --main book/src/getting-started/first-agent.md:rust:0 \
  --main book/src/getting-started/first-agent.md:rust:1 \
  --main book/src/getting-started/first-agent.md:rust:2 \
  --bin client=book/src/getting-started/first-agent.md:rust:3 || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/my-agent"
cargo build --bins >build.log 2>&1; grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)' build.log
# A reader who types the three steps as written must get no compiler warning.
if grep -q '^warning' build.log; then echo "FAIL: the page's code, typed as written, warns"; fail=1; fi
"$CARGO_TARGET_DIR/debug/my-agent" > agent.out 2>&1 &
pid=$!
for _ in $(seq 1 100); do curl -s --noproxy '*' -o /dev/null http://127.0.0.1:3000/.well-known/agent-card.json && break; sleep 0.2; done
echo "--- cargo run --bin client"
cargo run --bin client 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)'
echo "exit: ${PIPESTATUS[0]}"
kill -TERM $pid 2>/dev/null; wait $pid 2>/dev/null; echo "agent exit: $?"
echo "--- agent stdout/stderr"; cat agent.out
exit "${fail:-0}"
