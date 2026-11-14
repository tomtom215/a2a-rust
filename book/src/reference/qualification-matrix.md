<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Qualification Matrix

What 0.16 was run against before release: real replica processes on a real PostgreSQL, with faults injected into
the replicas, the database and the model provider, and the invariants checked by SQL afterwards. Run on 2026-11-14 on
one 4-vCPU Linux host. The commands, the environment and every recorded output are in
[`docs/audits/2026-11-14-qualification/`](https://github.com/tomtom215/a2a-rust/blob/main/docs/audits/2026-11-14-qualification/README.md),
so each row can be run again.

The invariants checked after each replica and store fault: every task ends in one terminal state, with no terminal
event twice and none after the terminal one; the stored state equals the terminal event; no claim is left on a
finished task; the event log has no gap; nothing that was sent is missing; and no task's executor started twice.

## Replica faults

PostgreSQL 16, two to thirty replicas behind a round-robin proxy (`c1_chaos.py`).

| Fault | Outcome |
|---|---|
| A replica holding 20 of 60 claims killed with `SIGKILL` mid-turn | Held. All 60 tasks terminal within 40 s: 20 `failed` (the killed replica held 20 claims), 40 completed |
| A replica frozen with `SIGSTOP` 12 s past a 3 s claim time to live, then continued | Held. It wrote nothing once it woke |
| Every database connection terminated, repeatedly, over a run | Held. 2,000 of 2,000 parts stored |
| 300 concurrent blocking sends against a pool of 2 connections | Held. All 300 answered; the replica served again afterwards |
| The database link paused 8 s mid-turn, then cut for 3 s | Held. Both replicas alive; a send worked once the link was back |
| 30 replicas (pool 1) started at once on one database | Held. All 30 up; 300 tasks completed |
| 10,000 tasks over 3 replicas | Held. Resident memory of each replica afterwards about 33 MiB |
| A task cancelled on one replica while the replica running it shuts down | Held. All 10 `canceled`, one terminal event each |
| Steering through a replica that does not hold the claim, then after the holder is killed | Held. Handed off and read; after the kill, the task ended `failed` once the claim lapsed |
| A replica killed with `SIGKILL` at its push outbox, after the event is stored and before its delivery is armed | Held. The webhook was sent the event once, 30 s later, by the surviving replica |
| The same, after the delivery is reserved and before the event is stored | Held. No delivery for an event the task never had |

Not run: 60 replicas at once. At two connections each (task store and push-config store), they need 120 of the 97
connections PostgreSQL's defaults allow, which is the sizing rule
[Sharing the store](../deployment/horizontal-scaling/shared-stores.md) gives.

## Store faults

PostgreSQL 17 in Docker, the replica built in release mode (`scripts/live/`).

| Fault | Outcome |
|---|---|
| Every backend of the replica terminated mid-turn | Held. Nothing written was lost |
| The database stopped mid-turn and restarted (4.6 s) | Held. The turn went on writing once it was back; no part or log entry missing |
| The task's row locked for 2.1 s under a 200 ms statement timeout | Held. No part or log entry missing |
| One replica's database link cut for 5 s, under the claim's 15 s time to live | Held. The task stayed with its replica; another replica refused a continuation as already being processed; a cancel sent there stopped it in 0.86 s |
| One replica's link cut for 20 s, past the time to live | Held. The cut-off replica stopped its own turn at 13.1 s (watchdog, 12 s); another replica's reaper ended the task `failed`, class `transient`, at 16.0 s; a continuation was refused as terminal; the cut-off replica wrote nothing more |
| The database restarted under two replicas | Held. The turn resumed writing |

## Model-provider faults

| Fault | Outcome |
|---|---|
| HTML with a 200; a stream that never sends; one that sends a keep-alive a second; a JSON body at a byte a second; a chunked body that stalls | Held. Each task `failed` within the 6 s model timeout, or at once, with a status saying which; memory bounded |
| A 100 MB `data:` line; a 100 MB JSON body | Held. Refused at the 8 MiB cap; resident memory rose by at most 8 MiB |
| A body or a stream cut mid-JSON | Held. `failed`, class `transient` |
| A 429 with a `Retry-After` of a year, a date in the past, 0, -5, `abc`, `1e10` or twenty nines | Held. `failed`, class `transient`, without waiting a year |
| A 502 whose body carries a canary, a forged log line and ANSI codes | Held. The body is not in the task |
| 200 replies naming 200 different models | Held, after a fix in this release: 17 series per metric (the first 16 names and `_OTHER`); 200 before |
| A real `llama-server` not running when a task is sent | Held. `failed` at once, class `transient` |
| A real `llama-server` killed with `SIGKILL` while generating | Held. `failed`, class `transient`, by the first poll after the kill; the agent stayed up |
| The same `llama-server` started again | Held. A new task completed |

## Rolling upgrade

A replica built from v0.15.0 beside one of this release, on one PostgreSQL 17 database: 4 of 4 scenarios held (an old
replica cannot write a task a new one runs; a cancel sent to the old replica stops the new replica's executor, in
1.01 s; the new replica refuses a continuation while the old one runs the task; a task the old replica parked
continues on the new one with its artifacts). With the database's upgrade guard disabled, as a control, 1 of 4 held.
[Running More Than One Replica](../deployment/horizontal-scaling.md#upgrading-a-fleet-to-this-release) explains the
guard.

## Not covered

Hosts other than one Linux container, so no network between replicas; SQLite under these faults; a PostgreSQL
rollback from 0.16 to 0.15, and the audit and push-config stores across the upgrade; disk full, out-of-memory kills
and clock skew; and real providers other than llama.cpp with Qwen3-1.7B. The
[Support Matrix](./support-matrix.md) says what is supported regardless.
