#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The hand mutants of the push outbox's reservation (ADR 0037, amendment 2026-11-11; audit H14):
# each JSON file is the mutants of one source file, run with scripts/hand_mutate.py against the
# tests that must catch them. Every one must be CAUGHT.
#
#   A2A_TEST_POSTGRES_URL=postgres://USER:PASS@127.0.0.1:5432/DB scripts/mutants/push-outbox-h14/run.sh
#
# Needs a PostgreSQL for the `postgres.rs` mutants (the role should not be named like the schema,
# `a2a`: see docs/handoff.md). Each line below takes about a minute per mutant (one test build).
set -u
cd "$(dirname "$0")/../../.."
here=scripts/mutants/push-outbox-h14
status=0
run() {  # FILE MUTANTS TEST_FILTERS [EXTRA_TEST_ARGS]
  python3 scripts/hand_mutate.py "$1" "$here/$2" \
    "cargo test -p a2a-protocol-server --features 'sqlite postgres' --lib -- $3 $4" --timeout 600 || status=1
}
run crates/a2a-protocol-server/src/handler/claims/reaper/mod.rs handler__claims__reaper__mod.rs.json "reserve_tests reservation::tests tests::outbox::lease reaper::tests::outbox" ""   # 4
run crates/a2a-protocol-server/src/handler/event_processing/background/processor.rs handler__event_processing__background__processor.rs.json "reserve_tests reservation::tests tests::outbox::lease reaper::tests::outbox" ""   # 7
run crates/a2a-protocol-server/src/handler/event_processing/background/push_delivery/lanes.rs handler__event_processing__background__push_delivery__lanes.rs.json "reserve_tests reservation::tests tests::outbox::lease reaper::tests::outbox" ""   # 1
run crates/a2a-protocol-server/src/handler/event_processing/background/push_delivery/outbox_worker.rs handler__event_processing__background__push_delivery__outbox_worker.rs.json "reserve_tests reservation::tests tests::outbox::lease reaper::tests::outbox" ""   # 5
run crates/a2a-protocol-server/src/handler/event_processing/background/push_delivery/settling.rs handler__event_processing__background__push_delivery__settling.rs.json "reserve_tests reservation::tests tests::outbox::lease reaper::tests::outbox" ""   # 5
run crates/a2a-protocol-server/src/handler/event_processing/background/push_delivery/reservation.rs handler__event_processing__background__push_delivery__reservation.rs.json "reserve_tests reservation::tests tests::outbox::lease reaper::tests::outbox" ""   # 16
run crates/a2a-protocol-server/src/handler/event_processing/sync_collector.rs handler__event_processing__sync_collector.rs.json "sync_collector::reserve_tests" ""   # 3
run crates/a2a-protocol-server/src/push/outbox/memory.rs push__outbox__memory.rs.json "push::outbox::tests::the_in_memory_outbox_keeps_the_contract" ""   # 8
run crates/a2a-protocol-server/src/push/outbox/postgres/record.rs push__outbox__postgres__record.rs.json "push::outbox::postgres_tests" "--include-ignored"   # 2
run crates/a2a-protocol-server/src/push/outbox/postgres.rs push__outbox__postgres.rs.json "push::outbox::postgres_tests" "--include-ignored"   # 5
run crates/a2a-protocol-server/src/push/outbox/sqlite/record.rs push__outbox__sqlite__record.rs.json "push::outbox::sqlite_tests" ""   # 3
run crates/a2a-protocol-server/src/push/outbox/sqlite.rs push__outbox__sqlite.rs.json "push::outbox::sqlite_tests" ""   # 5
exit $status
