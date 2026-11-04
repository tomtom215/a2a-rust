<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live proof of the delegation hand-off (audit group M)

Run on 2026-11-04 from `main` at `3685b8f9`, by `scripts/live/delegation/`. Nothing under `crates/` was changed.

Status words: **PROVEN** is a count read from `psql` and from the processes' own
logs in every run of a scenario, in this setup only. **VALIDATED** says by what
second means. **CONJECTURED** is reasoning that was not run. Every number below
is copied from `logs/driver-stdout.log` (the driver's stdout, whose
tables come from `logs/results.jsonl`) or from the file named beside it.

## Outcome

- **(a) kill -9: PROVEN** (3 of 3 runs, 50 children each). Killing the supervisor at 20 s cancelled nothing. The book's recovery loop, run as a new process 5.4 to 7.5 s after the kill, found 50 open records with task ids and ended with 50 `completed` journal records, 0 `canceled`, 0 open, and 50 `completed` tasks. The worker log has 0 cancel lines.
- **(c) graceful redeploy detaches: PROVEN** (3 of 3 for a bare supervisor, 3 of 3 for a supervisor that is an agent behind `Server::serve_with_shutdown`). SIGTERM at 20 s: the supervisor exited 0 within 0.07 s with 50 `HandedOff`, the 50 records stayed open, the 50 tasks stayed `working`, nothing was cancelled, and `recover` then completed all 50. **VALIDATED** by a control (`cx`) in which the same SIGTERM is wired to the cancel future, as the book's pattern did before the fix: 50 of 50 cancelled, 50 records settled `canceled`, 50 cancel hooks at the worker (3 of 3). So the harness can see the failure the hand-off rules out.
- **(b) gRPC idle child survives: PROVEN at scaled timers** (3 of 3): a child silent for 90 s under a 10 s client idle timeout was resumed 8 times and ended `completed`, 0 `CancelTask`. Not the old probe's 7 minutes and 60 s.
- **Leased children (`al`), not asked for by the task, from the h-s0 probe:** the book's loop loses them. Predicted (CONJECTURE, written before): 1 completes, 49 are stopped. Observed: 1 completed and 49 canceled in 2 of 3 runs, 0 completed and 50 canceled in the third (the restart was late enough that even the first child's lease lapsed before its first renewal was due). See Findings.
- **Defects in `crates/`: none found.** One documentation gap (finding 1) and one design edge (finding 2) are in Findings. The worker in `scripts/live/replica` could not stand in: its `hold` never completes (VALIDATED, below), so this directory has its own worker.

## Environment

From `logs/environment.txt` (commands: `uname -sr`, `nproc`, `free -m`, `rustc -V`, `psql --version`, `SELECT version()`):
Linux 6.18.44-fc-v233, 4 CPUs, 16091 MB RAM, rustc 1.99.1 (58cf8f9c3 2026-10-13), psql 16.15 and server PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1)
with `max_connections = 100` and `ssl = off`, local on 127.0.0.1:5432, no Docker, loopback only; the harness starts each process with an environment of `PATH` and its own settings, so no proxy variable reaches the client.
Build profile: dev (opt-level 0), `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j2`, with the workspace's `Cargo.lock` copied next to the harness manifest. SHA-256 of the binaries behind the tables are in `logs/environment.txt`.
Worker limits that could have touched 50 concurrent children, from the source: `DEFAULT_MAX_CONCURRENT_STREAMS` is 1024 (`crates/a2a-protocol-server/src/builder.rs:39`), the task-store pool is 10 connections (`crates/a2a-protocol-server/src/store/mod.rs:43`; `PostgresTaskStore::new`), the supervisor's journal pool was set to 10. The snapshots saw 11 connections to the run's database (`connections` in `results.jsonl`). **0 rejected sends in every run** (the `rej` column, the supervisor's `START_ERR` lines), and the 50 sends of a run had all started within 158 to 481 ms (first to last `start` line at the worker). Whether the 10-connection pool made some of them wait internally was not measured.

## Commands (from the repository root)

    cp Cargo.lock scripts/live/delegation/Cargo.lock
    CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j2 \
      --manifest-path scripts/live/delegation/Cargo.toml --target-dir target/live/delegation
    B=target/live/delegation/debug
    python3 scripts/live/delegation/delegation_live.py --worker $B/a2a-live-worker \
      --supervisor $B/a2a-live-supervisor --scenarios a,c,c2,cx,al --runs 3 --children 50 \
      --hold-secs 60 --kill-at 20 --lease-secs 15 --seed 20261104

    CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j2 --features grpc \
      --manifest-path scripts/live/delegation/Cargo.toml --target-dir target/live/delegation-grpc \
      --bin a2a-live-grpc-idle
    python3 scripts/live/delegation/grpc_idle_live.py \
      --bin target/live/delegation-grpc/debug/a2a-live-grpc-idle --runs 3 --quiet-secs 90 --idle-secs 10

Each run uses a fresh database (`a2a_deleg_<scenario><run>_<random>`), the worker on a free port, and is
independent. The seed fixes the restart delay of each run (uniform 2 to 10 s, per run
`<seed>-<scenario>-<run>`); the kill is at 20.00 s after the supervisor was launched, and the
youngest child is then 19.4 to 19.7 s old (the `age_s` column of the driver's table, over all 15 runs).
`logs/driver-stdout.log` ends with the driver's own table. Runs: 3 of each of `a`, `c`, `c2`, `cx` and `al`, 15 in all, 13 minutes (`start` and `end` lines of `logs/driver-stdout.log`).

## What the harness is

- `a2a-live-worker`: JSON-RPC agent over `PostgresTaskStore`. `hold` works 60 s then **completes**, or stops when cancelled, and logs `start`, `completed`, `cancel-hook` (the server calling the executor's cancel) and `token-fired` (the executor seeing its token).
- `a2a-live-supervisor`: `start N` = N `Delegation::start_with(.. DelegationOptions::new().with_journal(..))` children, each message text `hold`, owner from `$A2A_OWNER`, journal `PostgresDelegationJournal::from_pool`, each waited with `wait_or_hand_off`; `recover` = the book's loop, `journal.open(owner)`, `reattach_with(.. with_journal_record(..))`, `wait`, one record after the other, as `book/src/client/delegation.md:166` writes it (it prints a line per record); `serve` = the same supervisor as an agent behind `Server::serve_with_shutdown`, the executor passing `ctx.cancellation_token` as `cancel` and the SIGTERM as `hand_off`. SIGTERM is received by a real signal handler in all modes.
- The driver reads `a2a_delegations` and `tasks` with `psql` and the processes' logs.

Two points where this run differs from the plan:
the worker is not `scripts/live/replica`, and `recover` does not re-send a record without a task id (none occurred: `no_task_id` is 0 in every snapshot).
**The replica's `hold` never completes. VALIDATED by run** (`logs/replica-hold-check.log`): after `hold`, the task was `TASK_STATE_WORKING` at 10 to 70 s, the replica logged `hold ended, cancelled=false`, and the task was `TASK_STATE_FAILED` by 80 s (by the reaper, according to `terminal.rs`; the log does not say). The source says the same (`scripts/live/replica/src/main.rs:42-50` returns after 60 s without a terminal state; `crates/a2a-protocol-server/src/handler/messaging/terminal.rs:221-227` logs "the task stays that way and is not completed"). With the replica as the worker, "50 children end Completed" could not have been expected.

## Expectation, written before the full-size runs

Recorded 2026-11-04, after a 5-child, 15-second smoke run of each scenario
that only showed the harness works (all three passed at that size; those
numbers are not used anywhere below) and before any 50-child run. The sha256
of this section's text is printed into the raw log when the runs start, and
the same text is in this file unchanged.

Setup: one worker (`a2a-live-worker`, JSON-RPC, `PostgresTaskStore`), one
supervisor with a `PostgresDelegationJournal` in the same database, 50
children that each hold 60 s then complete, a fresh database per run.

**(a) kill -9 at 20 s, restart as `recover` within 10 s.**

1. At the kill all 50 children are named and 50 journal records are open with
   a task id; the worker shows 50 tasks `working` and no cancel request: the
   death of the supervisor does not cancel a child.
2. `recover` lists 50 open records, reattaches each, and every outcome is
   `completed`.
3. Afterwards: 50 journal records `completed`, 0 `canceled`, 0 open; 50 tasks
   `completed`, 0 non-terminal; the worker log has 0 `cancel-hook` and 0
   `token-fired` lines and 50 `completed` lines; no rejected send.
4. `recover` exits when the youngest child completes: about 60 s after the
   children started, so 25 to 45 s after it is launched.

**(c) SIGTERM at 20 s (bare supervisor `start`) and (c2) SIGTERM to the
supervisor as an agent behind `Server::serve_with_shutdown`.**

1. The supervisor exits 0 within 10 s of the signal (expected: under 1 s),
   every child's outcome is `handed_off`.
2. Right after the exit: 50 journal records still open, none settled
   `canceled`; 50 tasks still `working`; 0 cancel lines at the worker.
3. A following `recover` completes all 50, with the same end state as (a).
4. (c2 only) the executor's `cancellation_token` is not yet fired when the
   children hand off (`token_cancelled=false`), because the hand-off signal is
   raised by the same SIGTERM before the server fires the tokens.

**What would make me wrong:** the worker or the client cancelling a child on
a closed SSE stream; `reattach_with` failing on a terminal task or an
event id; the 10-connection task-store pool starving 50 concurrent children
(rejected sends); a record written late so that a child is named but its
record has no task id at the kill.

(sha256 of this section's text, printed into the log when the runs started: `26a34de7873cefc30ef3e1f28c7adc72ae6d54f92966958353c00e407c8005f4`)

## Expectation for (b), written before the gRPC runs

Recorded 2026-11-04 after (a), (c) and (c2) had been run and before
`a2a-live-grpc-idle` was compiled or run. Its sha256 is printed into the (b)
log when the runs start.

The probe scales the old one down: the child emits `working` and is then
silent for 90 s (the old probe: 7 minutes), and the client's
`stream_idle_timeout` is 10 s (the old probe: 60 s), directly over this
repository's gRPC server and client, no proxy. The cut-to-silence ratio is 9,
the old probe's was about 7.

1. The delegation ends `completed` after about 90 s, with no `CancelTask`
   reaching the server.
2. The server logs at least 7 `SubscribeToTask` calls (one per idle cut; the
   old limit was six cuts before the delegation was given up on and the
   child cancelled). I expect 8 or 9.
3. Without the rule of 2026-11-03 (a stream that delivered an event and stayed
   open for `healthy_after` has worked) this would have ended `Lost` with a
   `CancelTask`. That counterfactual is not run here: it would need the code
   from before #193.

**What would make me wrong:** the client's idle timeout not applying to gRPC
streams (then there are no cuts and the run proves nothing, which the
`SubscribeToTask` count shows); the server's snapshot on resume not counting
as a delivered event.

(sha256 of this section's text, printed into the log when the runs started: `c3b9cfbc783a0ae6559a347b75f7b1917162dbbf1d355c4cc00d6d8a9febcb53`)

## Prediction for (a) with leases (`al`), written before it was built or run

Recorded 2026-11-04, after the runs of (a), (c), (c2), the control and the
gRPC probe had been made and before any leased run. The h-s0 probe asked for
"many Canceled with start_leased at TTL 15 s". This is a CONJECTURE from
reading the code, to be confirmed or refuted by the run.

Setup as (a), with the worker granting leases (`LEASE=1`) and the supervisor
asking each child for a lease of 15 s (renewed every 5 s by the handle) and
giving `reattach_with` the same lease in `recover`.

1. At the kill: 50 records open, 50 tasks working, 0 cancels. The death alone
   cancels nothing; a lease lapses only a TTL after its last renewal, which is
   30 to 35 s into the run, after the restart (3 to 10 s after the kill).
2. `recover` is the book's loop: it reattaches the first record and `wait`s on
   it, about 35 s, before the second record's handle exists. A handle renews
   its child's lease, so child 1 is renewed and the other 49 are not. Their
   leases lapse, the worker stops them (`Canceled`), and the loop later
   reattaches 49 children that are already `Canceled`.
3. Expected end state: journal `completed` 1 and `canceled` 49 (I will accept
   "many" as 25 or more canceled), tasks `COMPLETED` 1 and `CANCELED` 49,
   worker `token-fired` and `cancel-hook` lines for the stopped children, no
   open record.

This would not be a defect in `crates/`: the loop in the book is sequential and
the book's example does not pass a lease. It would show that the book's loop
loses leased children, which the book does not say.

(sha256 of this section's text, printed into the log when the runs started: `a73eb2b94ab932a287e81afd11444e24acb01bd2825f4ae4ca1521a06f8db48b`)


## Results

All tables are `logs/driver-stdout.log` through `logs/results.jsonl`. "min / median / max" is over the 3 runs.

### (a) kill -9 at 20 s, `recover` within 10 s

| run | restart after (s) | before recover: records open / tasks working / cancels | recover exit (s) | exit after last child done (s) | after: journal completed / canceled / open | tasks completed / non-terminal | worker cancel-hook / token-fired / completed lines | rejected | result |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 5.4 | 50 / 50 / 0 | 37.9 | 2.03 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 2 | 6.5 | 50 / 50 / 0 | 34.3 | 0.28 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 3 | 7.5 | 50 / 50 / 0 | 33.2 | 0.38 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |

- restart after the signal (s), min / median / max: 5.4 / 6.5 / 7.5
- recover launch to exit (s): 33.2 / 34.3 / 37.9
- recover exit after the last child's completion line in the worker log (s): 0.28 / 0.38 / 2.03
- time for the children to be sent (first to last `start` line at the worker, ms): 267 / 275 / 337

Before the restart (taken right before `recover` was launched): 50 records open, all 50 with a task id (`recover` printed `open=50 with_task_id=50`), 50 tasks `TASK_STATE_WORKING`, 0 cancel lines at the worker. Every expectation of (a) held (the driver makes 12 checks per run of `a`, all `ok` in `results.jsonl`). Recovery is sequential, as the book writes it: the first record's wait takes 32.7 to 36.2 s, then the other 49 are found already settled, in 0.29 to 1.69 s together (from the timestamps of `recover.log` in each run).

### (c) SIGTERM at 20 s, bare supervisor

| run | restart after (s) | supervisor exit code, after signal (s) | outcomes | before recover: records open / tasks working / cancels | recover exit (s) | exit after last child done (s) | after: journal completed / canceled / open | tasks completed / non-terminal | worker cancel-hook / token-fired / completed lines | rejected | result |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 5.2 | 0, 0.03 | handed_off 50 | 50 / 50 / 0 | 35.4 | 0.24 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 2 | 7.2 | 0, 0.07 | handed_off 50 | 50 / 50 / 0 | 33.5 | 0.29 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 3 | 6.0 | 0, 0.03 | handed_off 50 | 50 / 50 / 0 | 34.6 | 0.27 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |

- supervisor exit after SIGTERM (s): 0.03 / 0.03 / 0.07
- restart after the signal (s), min / median / max: 5.2 / 6 / 7.2
- recover launch to exit (s): 33.5 / 34.6 / 35.4
- recover exit after the last child's completion line in the worker log (s): 0.24 / 0.27 / 0.29
- time for the children to be sent (first to last `start` line at the worker, ms): 158 / 248 / 257

### (c2) SIGTERM at 20 s, supervisor as an agent behind `Server::serve_with_shutdown`

| run | restart after (s) | supervisor exit code, after signal (s) | outcomes | before recover: records open / tasks working / cancels | recover exit (s) | exit after last child done (s) | after: journal completed / canceled / open | tasks completed / non-terminal | worker cancel-hook / token-fired / completed lines | rejected | result |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 3.7 | 0, 0.07 | handed_off 50 | 50 / 50 / 0 | 36.8 | 0.27 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 2 | 6.5 | 0, 0.03 | handed_off 50 | 50 / 50 / 0 | 34.1 | 0.32 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 3 | 3.4 | 0, 0.05 | handed_off 50 | 50 / 50 / 0 | 37.3 | 0.37 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |

- supervisor exit after SIGTERM (s): 0.03 / 0.05 / 0.07
- restart after the signal (s), min / median / max: 3.4 / 3.7 / 6.5
- recover launch to exit (s): 34.1 / 36.8 / 37.3
- recover exit after the last child's completion line in the worker log (s): 0.27 / 0.32 / 0.37
- time for the children to be sent (first to last `start` line at the worker, ms): 201 / 209 / 310

In all three, `EXECUTOR_DONE token_cancelled=false` (the driver checks it): the SIGTERM raised the hand-off signal before the server fired the executor tokens. The server's report in c2-1 was `ServeReport { accepted: 2, drained: true, abandoned: 0, tasks: Some(InFlightReport { completed: 1, cancelled: 0, still_running: 0, finished: true }) }` (`logs/c2-1/supervisor.log`).

### Control `cx`: the same SIGTERM wired to the cancel future (the old pattern)

| run | supervisor exit after SIGTERM (s) | outcomes | journal after (3 s later) | tasks | worker cancel-hook / token-fired | result |
|---|---|---|---|---|---|---|
| 1 | 0.17 | canceled 50 | {'canceled': 50} | {'TASK_STATE_CANCELED': 50} | 50 / 50 | PASS |
| 2 | 0.17 | canceled 50 | {'canceled': 50} | {'TASK_STATE_CANCELED': 50} | 50 / 50 | PASS |
| 3 | 0.17 | canceled 50 | {'canceled': 50} | {'TASK_STATE_CANCELED': 50} | 50 / 50 | PASS |

### (b) gRPC idle child, scaled

`grpc_idle_live.py` over `a2a-live-grpc-idle`: this repository's `GrpcDispatcher` and client over gRPC in one process, in-memory task store, a child that says `working` and is then silent for 90 s, client `stream_idle_timeout` 10 s (the old probe: 7 min, 60 s). The server's interceptor counts the calls it receives.

```
run  outcome    elapsed_s  SubscribeToTask  CancelTask  result
  1  completed       90.0                8           0  PASS
  2  completed       90.0                8           0  PASS
  3  completed       90.0                8           0  PASS
elapsed_s min 90.0 / median 90.0 / max 90.0; resumes min 8 / median 8 / max 8
```

The 8 `SubscribeToTask` calls are the resumes (one per idle cut, `logs/grpc-run1.log`); about six idle cuts were the old limit (ADR 0030, amendment, item 3). There was no `CancelTask`. The counterfactual (this run on the code before pull request #193) was **not run**.

### Leased children, `al`: the book's loop with `start_with(.. with_lease(15 s))` and `reattach_with(.. with_lease(15 s))`

| run | restart after kill (s) | recover launch to exit (s) | journal after | tasks after | worker token-fired lines | prediction |
|---|---|---|---|---|---|---|
| 1 | 3.3 | 37.4 | {'canceled': 49, 'completed': 1} | {'TASK_STATE_CANCELED': 49, 'TASK_STATE_COMPLETED': 1} | 49 | held |
| 2 | 7.3 | 4.4 | {'canceled': 50} | {'TASK_STATE_CANCELED': 50} | 50 | part 3 failed: prediction: at least one child completed: {'canceled': 50} |
| 3 | 3.3 | 37.5 | {'canceled': 49, 'completed': 1} | {'TASK_STATE_CANCELED': 49, 'TASK_STATE_COMPLETED': 1} | 49 | held |

Table notes: `al` is 15 s leases, kill -9 at 20 s, the worker granting them (`LEASE=1`, `InMemoryLeaseStore`). In runs 1 and 3 `recover` started at +23.3 s after the supervisor's launch and the first record was kept alive by its renewals; the other 49 leases lapsed between +30.3 and +31.4 s and the worker stopped them (`token-fired` 49). In run 2 `recover` started at +27.4 s; the leases lapsed at +30.1 s and the first renewal of the new handle was not due until +32.4 s (`ttl / 3` after the handle exists, `crates/a2a-protocol-client/src/delegation/renewal.rs:34-36`), so all 50 were stopped (`token-fired` 50). Computed from `logs/al-1`, `logs/al-2` (BEGIN, recover start and `token-fired` timestamps).

## Findings

**No defect in `crates/` was found by these runs.** What was found, none of it changed:

1. **The book's recovery loop loses leased children (documentation gap; PROVEN in 3 of 3 runs for "most are lost", CONJECTURE confirmed in 2 of 3 for the exact count).** `book/src/client/delegation.md:166-175` reattaches a record and `wait`s on it before it looks at the next, so the next handle (the thing that renews a lease) does not exist while the first child runs. With 15 s leases and 60 s children, 49 or 50 of 50 were stopped by the worker and settled `canceled`. The book says a restarted supervisor "must give `reattach_with` the lease" but its loop gives it to one child at a time. Reproduction: `delegation_live.py --scenarios al --runs 3 --seed 20261104` (`logs/results.jsonl`, `scenario: "al"`). A fix is a different loop (reattach every record first, then wait on all) or text saying so; neither was made.
2. **A restart too long after the kill loses every leased child, including the first (PROVEN: kept at 3.3 s, lost at 7.3 s; a design edge, not a contradiction of the text).** `Renewal::spawn` sleeps `ttl / 3` before its first renewal (`renewal.rs:34-36`), so a reattached child is renewed 5 s after its handle exists, and its lease lapses `ttl` after the last renewal the dead supervisor made (INFERRED, not logged: its tick at about +15 s, since the next would have been at about +20.3 s, after the kill at 20.0 s; the leases lapsed from +30.1 s). A restart at +27.4 s was too late: the new handle's first renewal was due at +32.4 s. The boundary is where `restart + ttl/3` passes `ttl` minus the age of the last renewal, about 5 s here (INFERRED from those two numbers, no run between 3.3 s and 7.3 s). An immediate first renewal on `reattach_with` would close it. Minimal reproduction: `--scenarios al --runs 3 --seed 20261104`, run 2 (restart delay 7.3 s).
3. **In (a) the journal records carry no position (`last_event_id=None`, all 50).** The children emit nothing after being named, so the stream position never moved; `reattach_with` follows from the current state. Correct for this child, and it means (a) does not exercise a replay from a recorded position. In (c) and (c2) the hand-off forces a write and the position is `Some("1")` (`logs/c-1/recover.log`). Not a defect.
4. **The replica's `hold` ends `failed` after 60 s** (VALIDATED above). Not a defect of the SDK (the executor guide says an executor must write a terminal state); a trap for a harness.

## Not run, and why

- **(b) as the old probe had it** (7 min silent, `stream_idle_timeout` 60 s, or through Envoy or nginx with `grpc_read_timeout 60s`): scaled to 90 s and 10 s, direct, no proxy; no Envoy or nginx was set up. The old behaviour (Lost plus `CancelTask`) was not reproduced, because that needs the code from before #193.
- **The reverse order of signal and token** (the server fires the executor token before the supervisor raises its hand-off): in (c2) the hand-off always came first. `wait_or_hand_off` is `biased` towards the hand-off when both are ready (`mod.rs:350`); a supervisor that raises it later than the server's shutdown would cancel. Not run.
- **The `serve`-mode supervisor killed with `kill -9`**: only the bare supervisor was killed.
- **A variant of (a) in which the worker or the database is also restarted, a network partition, or a kill of the worker with the supervisor**: the worker stayed up in every run.
- **Records without a task id** (a kill between `begin` and the first event) and the idempotency-key resend: no run produced one; the book says to resend, `recover` here does not.
- **Children that emit events while the supervisor is away**: the children are silent, so event replay from a recorded position was not tested.
- **A `Delegation` over WebSocket or REST**: JSON-RPC only; gRPC only for (b).
- **Release build, other PostgreSQL versions (17), more than one worker replica**: not run.
- **`cargo clippy` / `cargo test` of the harness, and `rustfmt`**: not run as gates; the harness is outside the workspace and has no tests. `rustfmt --edition 2024 --check` reports 30 differences in the three Rust files; they were left, because formatting them changed the binaries' SHA-256 (checked) and the runs were made on the unformatted source. It builds with 0 warnings (`touch` of the sources and `cargo build | grep -c '^warning'` printed 0 for both builds; the three rebuilt binaries have the same SHA-256 as the ones run).
- **Statistics beyond 3 runs per scenario**: min / median / max of 3 are the whole distribution.

## Strongest case against this result

A hostile reviewer would say:

1. **SIGKILL on loopback is the easiest crash.** The kernel closes the supervisor's sockets on SIGKILL, so the worker sees the connections end at once (what the worker did on seeing them was not logged; only that no cancel followed). A node loss or a partition leaves the worker with open streams to a dead peer, which this run does not show.
2. **The children are trivial.** They hold a task and emit nothing, so no event is lost in the gap, no replay is needed (finding 3), in (a) no journal progress is written after the child is named, and the 60 s hold ends everywhere at once. A real child streaming artifacts is the case ADR 0030's `last_event_id` exists for.
3. **One worker process, kept alive**, in-memory event queues, one database. The reattach hits the same process that owns the task. With several replicas a reattach could land on one that does not own the task's queue (CONJECTURED, not run).
4. **The worker is the harness's own** (finding 4): the replica's `hold` could not complete. The worker's `hold` is simpler than a real executor and the behaviour under test is the client's, but "50 completed" is partly the worker's doing.
5. **The recovery loop is `recover` here, not an operator following the book** and it prints a line per record; the book's `?` aborts the loop at the first error, and none occurred.
6. **Dev build, 3 runs, one machine**, 4 CPUs shared with the worker, the supervisor and PostgreSQL. The timing of "exits promptly" (0.03 to 0.07 s) is a dev build on a quiet machine.
7. **(c2) cannot show the race that matters**: the hand-off signal and the server's shutdown are one SIGTERM, with the hand-off first by construction (`token_cancelled=false`). The documented pattern is that wiring; a supervisor that wires it otherwise is not tested.
8. **The expectations were written by the person who built the harness**, after a 5-child smoke run of each scenario (not used in any number here), and the harness was extended twice after the first 9-run pass (a control, then leases): the earlier passes in `logs/earlier-passes.txt` (pass 1: `a`, `c`, `c2`, 9 of 9 runs passed; pass 2: also `cx`, 12 of 12 passed) are not the evidence; the final binaries' run is, and `a`, `c`, `c2`, `cx` passed 12 of 12 in it.
9. **(b) is a scaling of the old probe** with a ratio of 9, not the real timers, and without a proxy in the path.
10. **`al` shows the book's loop with one lease TTL and one kill time**; the numbers (49 of 50, 50 of 50) depend on 15 s and 60 s, and a longer lease or a parallel loop would change them.

## Files

- `logs/driver-stdout.log`: the final run's full stdout, ending with the table. `logs/results.jsonl`: every measurement of every run, one line each.
- `logs/<scenario>-<run>/`: `supervisor.log` (the `start` or `serve` process), `recover.log`, `worker.log`, `db-final.json` for a-1, c-1, c2-1, cx-1, al-1, al-2. The others are in `target/live/delegation/final` and were not copied.
- `logs/grpc-driver-stdout.log`, `logs/grpc-run1.log`; `logs/replica-hold-check.log`; `logs/environment.txt`; `logs/earlier-passes.txt`.
- The harness: `scripts/live/delegation/` (see its `README.md`).

## Cost

- Wall clock: the first build started at 14:54 UTC (file timestamps in `target/live/delegation`), the final 15-run set ran 15:28:38 to 15:41:38 UTC (its `start` and `end` lines), the gRPC runs 15:22:35 to 15:27:05 (its `start` and `end` lines), and this file was generated at 15:50 UTC. Reading the sources before the first build is not timed. The two earlier full passes ran 14:58:21 to 15:07:33 and 15:09:47 to 15:20:10.
- Disk: `df -h /home/user` showed 19G available before the first build and 14G after the last (other work shares the disk). `du -sh target`: 1.8G (`target/live/delegation` 550M, `delegation-grpc` 714M, `replica-build` 484M); deleted after the evidence was copied.
- Builds: `cargo build -j2` of the worker and supervisor, 1 min 48 s; of the gRPC probe, 1 min 24 s (dev profile, from the build output).

