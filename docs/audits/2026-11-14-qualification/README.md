<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Qualification matrix of 2026-11-14: runs and evidence

The release criterion of the adversarial review of 2026-11-13 (PDF section 7, register row E5): replica kill, store
faults and provider faults, each a run against real processes with its evidence. The summary a reader wants is the
book's [Qualification Matrix](../../../book/src/reference/qualification-matrix.md); this file has how each row was run,
so it can be run again, and the files each row's evidence is in.

## Environment

| | |
|---|---|
| Host | Linux 6.18.44, 4 vCPU, 15 GiB, one container |
| Tree | branch `claude/dazzling-wozniak-xp79ii` at `f60a38aa` with the dependency updates of `117709b9` in the working tree (the binaries below were built after `cargo update`); the provider rows' agent also carries `5250527c` |
| Rust | 1.100.0 |
| PostgreSQL | 16.15 (local, for `c1_chaos.py`); 17.12 in Docker (`postgres@sha256:846a7dda…`, for `scripts/live/*.py`) |
| Network faults | toxiproxy 2.12.0 (`ghcr.io/shopify/toxiproxy@sha256:9378ed52…`), and `chaoslib.py`'s own TCP proxy |
| Model | llama.cpp `llama-server` 9daa0aa, Qwen3-1.7B Q4_K_M (SHA-256 `b139949c5bd74937…`), 4 threads |

## Runs

Binaries: `scripts/live/campaign/hostile/target` (debug, with the workspace `Cargo.lock` copied in), `scripts/live/replica`
(release, `target/live/build`), the same replica at v0.15.0 (`scripts/live/replica/v015`, `target/live/build-v015`),
and `scripts/live/campaign/live/agent` (debug, `target/campaign`).

| Row | Command | Result | Evidence |
|---|---|---|---|
| Replica faults | `A2A_HOSTILE_EV=… A2A_HOSTILE_BIN=target/hostile/debug/a2a-hostile-target python3 -I scripts/live/campaign/hostile/c1_chaos.py` | 9 scenarios held every check (26 checks); `many60` not run (below) | `evidence/c1/` |
| Store: backends ended | `python3 scripts/live/terminate_live.py $R rc-2026-11-14` | held: 3 backends ended mid-turn, ticks 0..12 stored, none missing | `evidence/live/terminate_live.txt` |
| Store: database restart | `python3 scripts/live/restart_live.py $R rc-2026-11-14` | held: 4.6 s down, parts 0..25 and log 1..27, none missing | `evidence/live/restart_live.txt` |
| Store: row lock | `python3 scripts/live/lock_live.py $R rc-2026-11-14` | held: 2.1 s lock under a 200 ms timeout, parts 0..20 and log 1..22, none missing | `evidence/live/lock_live.txt` |
| Store: link partitions | `python3 scripts/live/partition_live.py $R` (toxiproxy) | 4 of 4 | `evidence/live/partition.txt`, `partition-old-p2.txt` |
| Replica killed at the outbox | `python3 scripts/live/outbox_live.py $R rc-… arm` and `… reserve` | held, both | `evidence/live/outbox_*.txt` |
| Rolling upgrade | `python3 scripts/live/upgrade_live.py $OLD $R` and `… --no-guard` | 4 of 4; the control without the guard 1 of 4 | `evidence/live/upgrade*.txt` |
| Provider misbehaviour | `python3 -I scripts/live/campaign/live/06_provider.py` | 24 of 24 | `evidence/provider/06_provider.txt`, `06-provider-misbehaviour.json` |
| Provider-chosen metric labels | `… 06_provider.py cardinality` | 0 of 1 before the fix (200 series), 1 of 1 after (17) | `06_provider_cardinality-before-fix.txt`, `06_provider_cardinality.txt` |
| Real provider down, killed, back | `python3 -I scripts/live/campaign/live/13_provider_death.py` | 3 of 3 | `evidence/provider/13_provider_death.txt`, `13-provider-death.json`, `13-llama-server.log` |

`R=target/live/build/release/a2a-live-replica`, `OLD=target/live/build-v015/release/a2a-live-replica-v015`; the
`scripts/live` runs had `A2A_LIVE_PG=postgres://a2a:a2a@127.0.0.1:5433`, `A2A_LIVE_CONTAINER=pg` and
`A2A_LIVE_OUT=docs/audits/2026-11-14-qualification/evidence/live`; the campaign runs had `CAMPAIGN_EVIDENCE` set to
`evidence/provider`.

## What the runs changed

* **Partition P2's expectation was out of date, not the code.** It expected the other replica to run a continuation on
  a task whose replica was cut off past the claim's time to live. Since the renewal watchdog and the abandoned-task
  reaper (CHANGELOG, Fixed and Added), the contract is that the cut-off replica stops its own turn, the task is ended
  `failed`, class `transient`, a continuation is refused as terminal, and the cut-off replica writes nothing more. The
  first run (`partition-old-p2.txt`) showed exactly that and failed the old check; P2 now checks the contract, with the
  times measured: the watchdog stopped the turn 13.1 s after the cut (designed for 12 s), the reaper failed the task at
  16.0 s. This is the toxiproxy repetition `book/src/deployment/horizontal-scaling/turn-claims.md` listed as not done.
* **The v0.15.0 replica no longer built**: it shared `scripts/live/replica/src/main.rs`, which now uses 0.16-only APIs.
  It has its own copy of that file as of `420c3cb3`, when the upgrade harness was written.
* **W2-LIVE-9 is fixed** (`5250527c`): the cardinality row failed as the wave-2 campaign recorded, and passes after.
* **`13_provider_death.py` is new**: the wave-2 provider script uses a scripted provider only. The kill landed while
  llama-server was generating: its log shows task 0 started and never finished.

## Not covered

* `c1_chaos.py many60`: 60 replicas at a pool of 1 hold two connections each (task store and push-config store,
  measured as 60 for 30 replicas), 120 against PostgreSQL's 97 usable; the harness's own `psql` was refused. That is
  the sizing rule of `book/src/deployment/horizontal-scaling/shared-stores.md`, not a fault of the SDK; nothing was
  learned about the replicas.
* Hosts other than one Linux container: no macOS or Windows replica, no separate machines, so no real network between
  replicas, only between replicas and the database.
* A PostgreSQL rollback from 0.16 to 0.15, and the audit and push-config stores across the upgrade.
* SQLite under these faults: every row ran on PostgreSQL.
* Disk full, out-of-memory kills, clock skew between replicas, and a slow (rather than dead) database.
* Real providers other than llama.cpp; the provider rows use a scripted provider and one local model.
