<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# SCL: scale and qualification

Read [`../README.md`](../README.md) §6 first, then [`../target-sheet.md`](../target-sheet.md) and E05 §3
(what is measured, with dates and hosts; what the readiness bar requires and is unmeasured; the known
ceilings; the store bottlenecks read from the SQL). E05 §5 "Scale" holds items S-A to S-G.

## Rules for this workstream

1. **A measurement package changes no library code.** If a measurement finds a defect, it is a new row and
   a COR package. The exception is a harness under `tests/`, `benches/` or `scripts/live/`.
2. **Every figure names its command, its host, its store and three runs with the range.** A figure the
   container cannot measure meaningfully (throughput, a latency tail) is not recorded as a result from it.
3. **Results go into `docs/readiness-bar.md`'s `measured-limits` block** (the release evidence manifest
   copies it) and into the target sheet, in the same pull request.
4. **Disk.** A full workspace test build has filled the session's allowance before (`docs/handoff.md`,
   *Environment facts*). Delete `target/debug/{deps,build,incremental}` between large jobs and never build
   two worktrees at once.

---

## SCL-01: Adopt the target sheet

- **Goal.** The approved targets are in `docs/readiness-bar.md`, each row marked measured or not, with no
  row in both states.
- **Evidence.** `docs/handoff.md` *First, next* item 2; E05 §4; E06 §5 C-01 and C-02.
- **Gates.** DEC-03. **Depends.** FND-03. **Release.** documentation.
- **Do.** Merge the approved rows of the target sheet into the readiness bar's tables and its
  machine-readable block; add a check that fails when an id is listed as measured in prose and under
  `not_measured` in the block.
- **Touch.** `docs/readiness-bar.md`, `scripts/release_evidence.py`, `docs/program/target-sheet.md`.
- **Accept.** `python3 scripts/release_evidence.py --self-test` passes with a new case for the
  both-states defect.
- **Out of scope.** Measuring.

## SCL-02: The readiness bar on every binding and store it names (S-A)

- **Evidence.** E05 §5 S-A and §3.2; ROADMAP *Fleet* item 5 (register rows S5, X-15); targets F-2 to F-5 and
  L-5.
- **Gates.** DEC-03. **Depends.** SCL-01. **Release.** measurements; a test harness.
- **Read first.** `crates/a2a-protocol-server/tests/readiness_bar/`, `docs/readiness-bar.md` (how a row is
  printed before it is asserted), `scripts/live/README.md`.
- **Do.** Parameterise the readiness tests by binding (JSON-RPC, HTTP+JSON, gRPC, WebSocket) and store
  (memory, SQLite, PostgreSQL); add the rows the bar names and no test has; run on two replicas and on
  PostgreSQL.
- **Accept.** S-A's check: the `measured-limits` block lists no row under `not_measured`, or each remaining
  one says which host it needs.
- **Out of scope.** Changing a threshold to make a row pass (report the number).

## SCL-03: Sixty replicas with a stated connection plan (S-B)

- **Evidence.** E05 §5 S-B and summary (not run: 120 connections needed against 97 available);
  `book/src/reference/qualification-matrix.md`; target S-3.
- **Gates.** DEC-03. **Depends.** none. **Release.** a measurement and a book section.
- **Read first.** `docs/audits/2026-11-14-qualification/` (the driver and its invariants), ADR 0057 §6 (a
  listener needs a session connection), `book/src/deployment/horizontal-scaling/` (*Behind PgBouncer*).
- **Do.** As S-B: write the pool arithmetic down (connections per replica by role), then run 60 with a
  pooler or a raised `max_connections`, and say which.
- **Accept.** The qualification invariants hold; connections in use are recorded; the book states the
  arithmetic with the measured figure.
- **Out of scope.** One hundred replicas, until sixty is recorded.

## SCL-04: SQL write amplification, measured before anything changes (S-D)

- **Evidence.** E05 §5 S-D and §3.4 (8 round trips for a new task's send, 11 for a continuation; the task is
  one JSONB row rewritten on every change, under four secondary indexes); ROADMAP *Performance* item 4.
- **Gates.** DEC-03. **Depends.** none. **Release.** a measurement; then, separately, a change.
- **Do.** S-D's three steps in order, stopping after step 2 to report: a PostgreSQL arm of
  `statement_count` for a 20-event streaming turn; WAL bytes per status transition with and without one
  index; only then a batch of already-queued appends.
- **Accept.** S-D's check. `tests/statement_count.rs` needs `PGSSLMODE=disable` on a server with TLS on.
- **Out of scope.** Splitting `tasks.data` (README §7).

## SCL-05: Memory per idle stream and per parked task (S-E)

- **Evidence.** E05 §5 S-E; targets M-2, M-3, S-2.
- **Gates.** DEC-03. **Depends.** none. **Release.** a harness and numbers.
- **Do.** A harness that opens 10,000 idle subscriber streams per binding and one that parks tasks with a
  ten-message history; heap bytes each, by the counting allocator the byte-budget test uses; store
  statements per second while idle.
- **Accept.** Numbers with their command, three runs. `ulimit -n` stated.
- **Out of scope.** Reducing either (a result may motivate a package).

## SCL-06: Leases and audit appends at fleet size

- **Evidence.** ROADMAP *Performance* items 2 and 3 (audit appends at several replicas, E-F18; lease groups
  only after a measurement at 10^4 to 10^5 leased tasks per replica, H5; ADR 0044); target S-4.
- **Gates.** DEC-03. **Depends.** none. **Release.** measurements.
- **Read first.** ADR 0044; the lease cost tests under `crates/a2a-protocol-server/src/handler/lease/`;
  ADR 0015.
- **Do.** Renewals per second and lease-store reads per interval at 10^4 leased tasks on a paused clock
  (exact counts), then live on PostgreSQL; audit appends per second and chain contention at one, two and
  four replicas.
- **Accept.** The counts match ADR 0044's arithmetic or the difference is explained. A recommendation, in
  the findings, on whether lease groups are warranted.
- **Out of scope.** Building lease groups.

## SCL-07: The deployment example is a fleet member (S-F)

- **Evidence.** E05 §5 S-F and §3.8 (the example manifest has a disruption budget and no autoscaler; the
  ADR 0054 hand-off is not wired in `examples/deploy-agent/src`); ROADMAP *Developer experience* item 2.
- **Gates.** none. **Depends.** OBS-03 (the capacity gauge an autoscaler reads), DVX-03.
- **Release.** an example.
- **Do.** As S-F: the autoscaler on an exported utilization metric, a headless Service for client-side
  balancing, a NetworkPolicy, a PodMonitor; `with_load_reporting` and the hand-off wired in the binary.
- **Accept.** S-F's checks: the manifest validates with the tool CI already uses; `python3
  scripts/check_example_servers.py` exits 0; a two-process shutdown test shows the continuation landing on
  the other replica. A run on a real cluster stays NOT MEASURED until a host that can run one exists; say
  so in the README.
- **Out of scope.** A Helm chart (ADR 0039 declined one).

## SCL-08: What the qualification matrix lists as not covered

- **Evidence.** ROADMAP *Fleet* item 4; `book/src/reference/qualification-matrix.md` ("Not covered").
- **Gates.** DEC-03. **Depends.** SCL-03. **Release.** `[0.16.x]`, measurements.
- **Do.** One row at a time, each with the driver the qualification used: more than one host; SQLite under
  the same faults; a PostgreSQL rollback from 0.16 to 0.15 and the audit and push-config stores across the
  upgrade (N14); disk full, out-of-memory kills and clock skew (E-F26); a model provider other than
  llama.cpp.
- **Accept.** Each row moves from "Not covered" to a result with its evidence directory under
  `docs/audits/`, or stays with the reason this environment cannot run it.
- **Out of scope.** Multi-region.
