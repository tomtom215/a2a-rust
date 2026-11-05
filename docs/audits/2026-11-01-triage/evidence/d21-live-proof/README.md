<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live proof of the turn hand-off on a worker's shutdown (audit D-21, ADR 0054)

Run on 2026-11-05 from branch `claude/inspiring-mccarthy-rgpntj-rest-claims` at `db4d2902`
(the server's hand-back and `Delegation`'s continuation) plus the harness changes committed with this evidence
(`scripts/live/delegation/src/bin/worker.rs`: `RESUMABLE=1`; `handoff_live.py`: the driver).
`m-live-proof/` proved the supervisor's side of a redeploy; this proves the worker's.

Status words as in `m-live-proof`: **PROVEN** is a count read from `psql` and the processes'
own logs in every run, in this setup only; **VALIDATED** names its second means.

## Outcome

- **(h) A worker redeployed mid-turn hands its turns back, and the supervisor's delegations
  run them to completion on the other worker: PROVEN** (3 of 3 runs, 5 children each). Worker
  A received SIGINT 8 s after the supervisor started; after its 5 s completion window it
  stopped the 5 turns (5 `token-fired`), ran no cancel hook (0 `cancel-hook`) and completed
  nothing (0). Each delegation saw the hand-off status, waited 1 s and sent its message again
  to the same task; the proxy sent those connections to B (5 to A, 5 to B), whose executor
  started each turn with the hand-off mark on its stored task (5 `rerun=true`) and completed
  it (5). The supervisor reported 5 `completed` and exited 0; the database held 5 tasks
  `TASK_STATE_COMPLETED` and 5 journal records `completed`. Each run took 34.3 to 34.5 s
  (predicted: 34 s).
- **(x) Control, no opt-in: VALIDATED** that the harness sees the failure the hand-off
  removes (3 of 3): the same shutdown ended the 5 turns `failed`/transient after the cancel
  hook (5 `cancel-hook`), B received nothing, the supervisor reported 5 `failed`, the
  database held 5 `TASK_STATE_FAILED`. Each run took 13.1 to 13.2 s.
- **Defects found: none.** The smoke run before the counted runs (1 run, 2 children, 10 s
  hold, signal at 4 s, not used below) passed the same way.

The table, from `logs/results.jsonl` (also printed in `logs/driver-stdout.log`):

| run | exit | s | outcomes | A start | A token-fired | A cancel-hook | A completed | B rerun starts | B completed | conns A/B | tasks |
|---|---|---|---|---|---|---|---|---|---|---|---|
| h1 | 0 | 34.5 | 5 completed | 5 | 5 | 0 | 0 | 5 | 5 | 5/5 | 5 COMPLETED |
| h2 | 0 | 34.4 | 5 completed | 5 | 5 | 0 | 0 | 5 | 5 | 5/5 | 5 COMPLETED |
| h3 | 0 | 34.3 | 5 completed | 5 | 5 | 0 | 0 | 5 | 5 | 5/5 | 5 COMPLETED |
| x1 | 0 | 13.1 | 5 failed | 5 | 5 | 5 | 0 | 0 | 0 | 5/0 | 5 FAILED |
| x2 | 0 | 13.2 | 5 failed | 5 | 5 | 5 | 0 | 0 | 0 | 5/0 | 5 FAILED |
| x3 | 0 | 13.1 | 5 failed | 5 | 5 | 5 | 0 | 0 | 0 | 5/0 | 5 FAILED |

## Setup

- Two `a2a-live-worker` processes, A and B, JSON-RPC on loopback, one `PostgresTaskStore`
  database (so turn claims are on), `HOLD_SECS=20` (a `hold` works 20 s, then completes),
  `RESUMABLE=1` in (h) and `0` in (x). Default shutdown timings (completion window 5 s).
- A TCP proxy in the driver sends each new connection to the first worker that accepts it,
  A then B: a load balancer during a rolling deploy, where the stopping replica stops
  accepting first. So the 5 children land on A, and once A stops accepting, every new
  connection lands on B. Nothing in the proxy reads HTTP.
- `a2a-live-supervisor start 5` through the proxy, with a `PostgresDelegationJournal`, default
  `DelegationOptions` (so the default `HandOffPolicy`: up to 3 continuations, 1 s, 2 s, 4 s).
- A fresh database per run (`a2a_handoff_<run>_<random>`), dropped afterwards.

Environment (`logs/environment.txt`): Linux 6.18.44-fc-v236, 4 CPUs, 16 GB, rustc 1.99.1,
PostgreSQL 16.15 local, dev profile. SHA-256 of the two binaries are in the same file.

## Commands (from the repository root)

    cp Cargo.lock scripts/live/delegation/Cargo.lock
    CARGO_TARGET_DIR=<shared target> CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
      cargo build -j2 --manifest-path scripts/live/delegation/Cargo.toml \
      --bin a2a-live-worker --bin a2a-live-supervisor
    python3 -I scripts/live/delegation/handoff_live.py --worker <bin>/a2a-live-worker \
      --supervisor <bin>/a2a-live-supervisor --scenarios h,x --runs 3 --children 5 \
      --hold-secs 20 --signal-at 8 --out docs/audits/2026-11-01-triage/evidence/d21-live-proof/logs

The binaries were copied out of the shared target directory before the runs, so another
build could not replace them in between.

## Expectation, written before the runs

The text is `EXPECTATIONS` in `scripts/live/delegation/handoff_live.py`, unchanged; its
sha256, printed into `logs/driver-stdout.log` when the runs started, is
`dbe49c881be4beb4e43a5a35b4af2159839788c0ca32eaa1df5ee824c5101b5a`. Every numbered point
held in every run.

## What this does not show

- A crash (SIGKILL): no hand-off is written by design; the reaper ends the task as before.
- More than one replica behind a real load balancer, gRPC or WebSocket, TLS, or a client
  that is not this crate's `Delegation`.
- The bound: the continuation succeeded on its first try in every run, so the second and
  third tries and the fall-back to a lost stream were not exercised live
  (`delegation::continuation::tests` covers them with a paused clock).
- A supervisor that reattached after a restart (it has no message to send again).
