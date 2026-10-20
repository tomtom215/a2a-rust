#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Each SQL store feature of a2a-protocol-server compiles only its own driver
# (release audit F25): `postgres` alone must not link SQLite, and `sqlite`
# alone must not link the PostgreSQL driver. Until 2026-10-18 the workspace's
# `sqlx` entry enabled `sqlite` for everyone, so a PostgreSQL deployment
# compiled and linked libsqlite3 too.
#
# `cargo tree -i` exits 0 whether or not the package is in the graph, so
# the check is on its output.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
absent() {
  local feature=$1 package=$2
  local found
  found=$(cargo tree -q -p a2a-protocol-server --no-default-features \
    --features "$feature" -e normal -i "$package" 2>/dev/null || true)
  if [ -n "$found" ]; then
    echo "FAIL: --features $feature links $package:"
    echo "$found" | head -5
    fail=1
  else
    echo "ok: --features $feature does not link $package"
  fi
}
present() {
  local feature=$1 package=$2
  if [ -z "$(cargo tree -q -p a2a-protocol-server --no-default-features \
    --features "$feature" -e normal -i "$package" 2>/dev/null || true)" ]; then
    echo "FAIL: --features $feature does not link $package; the check above proves nothing"
    fail=1
  fi
}

absent postgres libsqlite3-sys
absent sqlite sqlx-postgres
# Controls: each driver is found where it belongs, so an empty result above
# means absent rather than a broken query.
present sqlite libsqlite3-sys
present postgres sqlx-postgres
exit $fail
