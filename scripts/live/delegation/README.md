<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live proof of the delegation hand-off

Audit group M (`docs/audits/2026-11-01-triage/PLAN.md`), ADR 0030's amendment:
"a supervisor killed with `kill -9` against a PostgreSQL journal with fifty
children ... is a run, not code". This directory is the run. The evidence of
the last one, its expectation and its limits are in
`docs/audits/2026-11-01-triage/evidence/m-live-proof/README.md`.

| File | What it is |
|---|---|
| `src/bin/worker.rs` | `a2a-live-worker`: JSON-RPC agent over `PostgresTaskStore`; `hold` works `$HOLD_SECS` (60) then **completes**, and logs every start, completion and cancel |
| `src/bin/supervisor.rs` | `a2a-live-supervisor`: `start N` (N journaled children), `recover` (the book's loop), `serve` (the supervisor as an agent behind `Server::serve_with_shutdown`); SIGTERM hands the children off with `wait_or_hand_off` |
| `src/bin/grpc_idle.rs` | `a2a-live-grpc-idle` (feature `grpc`): a quiet child over gRPC cut by the client's idle timeout |
| `delegation_live.py` | the driver for scenarios `a` (kill -9), `c` (SIGTERM), `c2` (SIGTERM to the agent), `cx` (a control: SIGTERM wired to cancel, expects every child cancelled) and `al` (`a` with leases: a prediction, not a requirement); fresh database per run; counts with `psql`; exit 1 on a failed expectation |
| `grpc_idle_live.py` | the driver for `grpc_idle` |
| `pglocal.py` | the local PostgreSQL 16 helpers (no Docker) |

## Run it

From the repository root. PostgreSQL 16 is the Debian cluster (`pg_ctlcluster 16
main start`; the driver starts it if it is down) with the `postgres` role's
password `postgres` (`su postgres -c "psql -c \"ALTER USER postgres PASSWORD
'postgres'\""`); `A2A_LIVE_PG` names another server. Debian's `ssl = on`
default needs `ALTER SYSTEM SET ssl = off` first on some images.

    cp Cargo.lock scripts/live/delegation/Cargo.lock     # the workspace's dependency versions
    CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j2 \
      --manifest-path scripts/live/delegation/Cargo.toml --target-dir target/live/delegation
    B=target/live/delegation/debug
    python3 scripts/live/delegation/delegation_live.py \
      --worker $B/a2a-live-worker --supervisor $B/a2a-live-supervisor \
      --scenarios a,c,c2,cx,al --runs 3 --children 50 --hold-secs 60 --kill-at 20 --seed 20261104

    CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j2 --features grpc \
      --manifest-path scripts/live/delegation/Cargo.toml --target-dir target/live/delegation-grpc \
      --bin a2a-live-grpc-idle
    python3 scripts/live/delegation/grpc_idle_live.py --bin target/live/delegation-grpc/debug/a2a-live-grpc-idle

A full run of all five scenarios is about thirteen minutes for `delegation_live.py`
and five for the gRPC probe. Add `--children 5 --hold-secs 15 --kill-at 5` for a smoke run of the
harness itself. Logs and per-run database snapshots go to `target/live/delegation`
(`A2A_LIVE_OUT`).
