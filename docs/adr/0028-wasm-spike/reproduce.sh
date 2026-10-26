#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Reproduces ADR 0028's measurements from a clean worktree of the base commit.
#
#   usage: reproduce.sh [WORKDIR]      (default: a fresh temporary directory)
#   needs: git, cargo with the wasm32-unknown-unknown target, clang, python3, curl, pkill,
#          a free port 8787, worker-build 0.8.7, node with wrangler 4.152.0 (WRANGLER overrides
#          the command), and network access to crates.io and npm.
#   env:   SKIP_NATIVE_TESTS=1 skips the two native runs of the ~1,545-test lib suite.
#          CARGO_TARGET_DIR reuses a target directory (the default is under WORKDIR).
#
# Prints PASS/FAIL lines and exits non-zero if any check failed. It builds about
# 1.5 GB of artifacts under WORKDIR and removes nothing it did not create.
set -uo pipefail
BASE=6a82edcde4d5b2c84fe7e273e03738a6bec0f678
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(git -C "$HERE" rev-parse --show-toplevel)
WORK=${1:-$(mktemp -d)}
WRANGLER=${WRANGLER:-npx --yes wrangler@4.152.0}
fail=0
say() { printf '\n== %s\n' "$*"; }
ok()  { if "$@" >/dev/null 2>&1; then echo "PASS  $*"; else echo "FAIL  $*"; fail=1; fi; }
# expect_fail PATTERN CMD...: the command must fail and its output must contain PATTERN.
# Output is captured first: `grep -q` on a pipe under pipefail reports the writer's SIGPIPE as failure.
expect_fail() { local pat=$1; shift; local out; out=$("$@" 2>&1); local rc=$?
  if [ $rc -ne 0 ] && grep -q -- "$pat" <<<"$out"; then echo "PASS  (expected failure: $pat) $*"
  else echo "FAIL  expected failure containing '$pat' (rc=$rc): $*"; fail=1; fi; }

say "clean worktree of $BASE in $WORK/tree, patch applied"
git -C "$REPO" worktree add --detach "$WORK/tree" "$BASE" >/dev/null || exit 2
T=$WORK/tree; D=$T/docs/adr/0028-wasm-spike
mkdir -p "$T/docs/adr" && cp -r "$HERE" "$D"
( cd "$T" && git apply --check "$D/server-wasm32.patch" && git apply "$D/server-wasm32.patch" ) \
  && echo "PASS  patch applies to $BASE" || { echo "FAIL  patch"; exit 1; }

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$WORK/target} CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
cd "$T"

BASE_TREE=$WORK/base
git -C "$REPO" worktree add --detach "$BASE_TREE" "$BASE" >/dev/null || exit 2
say "unpatched $BASE on wasm32-unknown-unknown"
( cd "$BASE_TREE"
  ok cargo check -p a2a-protocol-types --target wasm32-unknown-unknown
  expect_fail "unsupported by mio" cargo check -p a2a-protocol-server --target wasm32-unknown-unknown
  expect_fail "unsupported by mio" cargo check -p a2a-protocol-client --target wasm32-unknown-unknown
  expect_fail "getrandom" cargo check -p a2a-protocol-types --features signing --target wasm32-unknown-unknown
  exit $fail )
[ $? -eq 0 ] || fail=1

say "patched tree on wasm32-unknown-unknown"
ok cargo check -p a2a-protocol-types -p a2a-protocol-server --target wasm32-unknown-unknown
# Still not wasm32 targets with the patch: sockets, through `mio`.
expect_fail "unsupported by mio" cargo check -p a2a-protocol-client --target wasm32-unknown-unknown
expect_fail "unsupported by mio" cargo check -p a2a-protocol-server --features otlp --target wasm32-unknown-unknown

say "native: every crate still builds on its own, and the workspace with all targets"
# With and without default features: with them, `hyper-rustls` re-enables features the patch drops from
# the workspace dependency lines, which once hid the client failing to build without them.
for c in a2a-protocol-types a2a-protocol-client a2a-protocol-server a2a-protocol-sdk; do
  ok cargo check -p $c; ok cargo check -p $c --no-default-features
done
ok cargo check --workspace --all-targets

say "trap-API lint on the wasm32 build (expect 0 hits)"
mkdir -p "$WORK/clippy" && cp clippy.toml "$WORK/clippy/clippy.toml" && cat "$D/clippy-wasm-traps.toml" >> "$WORK/clippy/clippy.toml"
touch crates/a2a-protocol-server/src/lib.rs
hits=$(CLIPPY_CONF_DIR=$WORK/clippy cargo clippy -p a2a-protocol-server --target wasm32-unknown-unknown \
  -- -A clippy::all -W clippy::disallowed_methods 2>&1 | grep -c 'use of a disallowed method')
echo "disallowed-method hits: $hits"; [ "$hits" = 0 ] && echo "PASS  lint clean" || { echo "FAIL  lint"; fail=1; }

if [ -z "${SKIP_NATIVE_TESTS:-}" ]; then
  say "native regression: cargo test -p a2a-protocol-server --lib, patched against unpatched, by test name and outcome"
  for which in tree base; do
    ( cd "$WORK/$which" && cargo test -p a2a-protocol-server --lib 2>&1 | tee "$WORK/$which.lib.log" | grep -E '^test result' )
    grep -E '^test .* \.\.\. ' "$WORK/$which.lib.log" | sort > "$WORK/$which.names"
  done
  if diff -q "$WORK/tree.names" "$WORK/base.names" >/dev/null; then echo "PASS  $(wc -l < "$WORK/tree.names") test lines identical, patched and unpatched"
  else echo "FAIL  per-test results differ: diff $WORK/tree.names $WORK/base.names"; fail=1; fi
fi

say "build handler-worker (with signing) and probe-worker"
( cd "$D/handler-worker" && worker-build --release >"$WORK/handler-build.log" 2>&1 ) && echo "PASS  handler-worker builds" || { echo "FAIL  handler-worker build, see $WORK/handler-build.log"; exit 1; }
W=$D/handler-worker/build/index_bg.wasm
echo "handler wasm: $(stat -c %s "$W") bytes raw, $(gzip -9 -c "$W" | wc -c) bytes gzip -9"

serve() { # start `wrangler dev` in $1 and wait for it; the session leader's pid is in $WORK/dev.pid
  stop_server
  # Nothing else may be answering on the port, or the checks below would measure that server.
  for _ in 1 2 3 4 5 6; do curl -s --noproxy '*' -m 2 -o /dev/null http://127.0.0.1:8787/ || break; sleep 1; done
  if curl -s --noproxy '*' -m 2 -o /dev/null http://127.0.0.1:8787/; then echo "FAIL  port 8787 is already answering; stop that server first"; return 1; fi
  cd "$1" || return 1
  # The wrapper records its own pid and execs wrangler, so the pid file names the session leader
  # whether or not `setsid` forks (it does under job control, not in a script).
  NO_COLOR=1 CI=1 WRANGLER_SEND_METRICS=false setsid bash -c 'echo $$ >"$0"; exec "$@"' "$WORK/dev.pid" \
    $WRANGLER dev --local --port 8787 --ip 127.0.0.1 >"$WORK/wrangler.log" 2>&1 </dev/null &
  cd "$T" || true
  for _ in $(seq 1 120); do sleep 1; curl -s --noproxy '*' -o /dev/null http://127.0.0.1:8787/.well-known/agent-card.json && return 0; done
  echo "server did not start"; tail "$WORK/wrangler.log"; return 1
}
# `setsid` makes wrangler its own session leader, so its pid is the session id: kill the whole session
# (wrangler, its node child and the workerd processes), and only that.
stop_server() { [ -f "$WORK/dev.pid" ] && { pkill -s "$(cat "$WORK/dev.pid")" 2>/dev/null; rm -f "$WORK/dev.pid"; sleep 1; }; true; }
trap stop_server EXIT

say "battery.py against the handler Worker (workerd, wrangler dev --local)"
serve "$D/handler-worker" || exit 1
python3 "$D/battery.py" || fail=1
echo "panics in the workerd log: $(grep -c panicked "$WORK/wrangler.log")"

say "wait_until A/B, one request per fresh isolate (expect COMPLETED, WORKING, COMPLETED)"
for arm in hold1 hold0 hold1; do serve "$D/handler-worker" >/dev/null && python3 "$D/hold_probe.py" $arm; done

say "probe-worker: what runs on workerd"
( cd "$D/probe-worker" && worker-build --release >"$WORK/probe-build.log" 2>&1 ) || { echo "FAIL  probe build"; exit 1; }
serve "$D/probe-worker" || exit 1
for p in sync std-instant tokio-sleep tokio-spawn wasmtimer tokio-with-wasm send-fetch; do
  printf '%-16s ' "$p"; curl -s --noproxy '*' -m 15 -w ' [HTTP %{http_code}]\n' "http://127.0.0.1:8787/probe/$p" | head -c 140 | tr '\n' ' '; echo
done
printf '%-16s ' "types-timestamp"; curl -s --noproxy '*' -m 15 -w ' [HTTP %{http_code}]\n' http://127.0.0.1:8787/std-time | head -c 140 | tr '\n' ' '; echo

say "types crate clock, unpatched $BASE (the probe-worker's /std-time must fail)"
mkdir -p "$BASE_TREE/docs/adr" && cp -r "$HERE" "$BASE_TREE/docs/adr/0028-wasm-spike"
( cd "$BASE_TREE/docs/adr/0028-wasm-spike/probe-worker" && worker-build --release >"$WORK/probe-unpatched-build.log" 2>&1 ) || { echo "FAIL  unpatched probe build"; exit 1; }
serve "$BASE_TREE/docs/adr/0028-wasm-spike/probe-worker" || exit 1
printf '%-16s ' "types-timestamp"; curl -s --noproxy '*' -m 15 -w ' [HTTP %{http_code}]\n' http://127.0.0.1:8787/std-time | head -c 110 | tr '\n' ' '; echo
sleep 1
panic_text=$(sed 's/\x1b\[[0-9;]*m//g' "$WORK/wrangler.log" | grep -A2 'panicked at')
if grep -q 'time not implemented on this platform' <<<"$panic_text"; then
  echo "PASS  unpatched types crate panics: time not implemented on this platform"
else echo "FAIL  expected the unpatched types crate to panic on its first timestamp"; fail=1; fi

say "done: $([ $fail = 0 ] && echo 'all checks passed' || echo 'SOME CHECKS FAILED')"
echo "worktrees left behind: $WORK/tree and $WORK/base (git worktree remove --force <path>)"
exit $fail
