<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Target sheet

Approved by the maintainer on 2026-11-25 (DEC-03 in [`README.md`](README.md)): the quantities and the
procedures are approved; every threshold stays PROPOSED until its first measurement. It is section 4 of the
 observability, performance and scale audit
([`evidence/05-observability-perf-scale.md`](evidence/05-observability-perf-scale.md)), moved here so it can
be edited as targets are approved and measured. Paths in the Procedure columns are abbreviated as in that
audit: `tests/…` is under `crates/a2a-protocol-server/tests/`, `srv/…` under
`crates/a2a-protocol-server/src/`; a bare `:NNN` cites the document the row's rationale names.

How to read it:

- **Every threshold is PROPOSED. None is a measurement.** A threshold becomes a result when a package in [`plan/scl-scale.md`](plan/scl-scale.md) or
  [`plan/prf-performance.md`](plan/prf-performance.md) measures it and records the command, the host and
  three runs.
- **"Container"** is the 4-vCPU, 15-GB shared cloud container a session runs in. It can measure counts,
  ratios within one run, correctness under faults and memory. It cannot measure absolute throughput or a
  latency tail: the 2026-11-11 comparison records up to 20% variance per cell and a factor of two between
  consecutive runs of one build.
- A row marked "new" needs a harness that does not exist; the package that builds it is named in
  `plan/scl-scale.md`.
- When a target is measured, write the result into `docs/readiness-bar.md`'s `measured-limits` block, which
  the release evidence manifest copies, and replace PROPOSED here with the date and the figure.

## Scale

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Hardware | Container? |
|---|---|---|---|---|---|
| S-1 turns per replica | concurrent running turns admitted with 0 non-overload errors | 1,024 (the shipped cap) on every binding and on PostgreSQL | extend `tests/readiness_bar/` with an L4-style arm at the cap; `scripts/live/burst_live.py` for PostgreSQL | 4 vCPU | Yes (count/correctness) |
| S-2 idle streams per replica | open SSE subscribers idle for 10 min, RSS and store statements/s | 10,000 (the shipped `max_subscribers`); statements/s ≤ 10,000 × 2 / 4 s from ADR 0057's "one poll of two statements every 4 s", and ≤ 1/s per replica with `LogListen` | new harness (none opens 10k streams); count statements with `scripts/live/pgcount.py` | 4 vCPU, `ulimit -n` ≥ 32k | Yes for memory and statement counts |
| S-3 replicas on one PostgreSQL | replicas started at once, tasks completed, connections used | 60 replicas (the unrun row) with a stated pooler or `max_connections` setting; then 100 | `c1_chaos.py` from the qualification evidence (`docs/audits/2026-11-14-qualification/`) with pool 1 + PgBouncer, or raised `max_connections` | 4 vCPU suffices for start-up correctness; ≥ 16 vCPU for throughput | Partly (30 ran here; 60 needs the connection plan, not more CPU) |
| S-4 leased children per supervisor | renewals/s and server CPU at 10^4 leased tasks | renewals/s = children × 3 / ttl exactly (ADR 0044), lease-store reads = 1 per interval per replica | extend `handler::lease::cost_tests` (paused clock) to 10^4; live arm in `scripts/live/delegation/` | any | Yes (counts) |
| S-5 fan-out per task | subscribers per task receiving every event of a turn that outlasts the reattach interval | 256 (shipped cap), 0 gaps | `tests/swarm_scale/` tail arm, add gRPC and WebSocket | 4 vCPU | Yes |
| S-6 event-log size | rows and bytes per live task under `max_events_per_live_task` after N sweeps | exactly `max` rows after each sweep (ADR 0047 C contract) at 10^5 live tasks; sweep duration recorded | new: populate + `purge_expired` timing on PostgreSQL | 4 vCPU, 20 GB disk | Partly (disk allowance) |

## Failure detection and recovery

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| F-1 replica SIGKILL mid-turn | time until every held task is terminal or resumed | ≤ 3 × claim TTL = 45 s (the register's "three claim lifetimes", ROADMAP `:78-79`; measured 40 s) | `c1_chaos.py` kill row | Yes |
| F-2 PostgreSQL restart under load (bar F2) | calls failing during, succeeding after, events lost | 0 lost events for outages < `store_write_retry` (30 s); for longer, a *counted and alerted* loss until X-10 is fixed | `scripts/live/restart_live.py`, `terminate_live.py` | Yes |
| F-3 resume on the other replica (bar F3) | events after `Last-Event-ID` delivered exactly once | 0 missing, 0 duplicated, on SSE/gRPC/WebSocket | `tests/multi_replica.rs` + `scripts/live/replica/` | Yes |
| F-4 peer gone mid-stream (bar R7/F4) | time until queue, token and connection are released | ≤ 30 s SSE keep-alive; ≤ `idle_timeout` WebSocket; immediate on gRPC channel close (the bar's own wording) | `tests/readiness_bar/` L3 extended per binding | Yes |
| F-5 graceful shutdown (bar R6) with ADR 0054 | streams receiving a terminal event or hand-off mark within `completion_grace` + 1 s; continuations succeeding elsewhere | 100% of streams; continuation after 1, 2, 4 s back-off (ADR 0054 `:105-106`) | `tests/graceful_shutdown_bindings/`, `scripts/live/upgrade_live.py` | Yes |
| F-6 halt propagation | time from halt call on replica A to executor stopped on replica B | ≤ 2 × halt poll interval = 2 s without NOTIFY; p99 ≤ 500 ms with it (rationale: measured cross-replica cancel 0.86–1.01 s) | `tests/halt_e2e.rs` on two processes + a new histogram (work item O-3) | Yes for the bound; No for p99 |
| F-7 telemetry outage | request success rate and RSS with the collector down or black-holed for 10 min | 0 failed requests, RSS growth ≤ exporter queue bound, `a2a.telemetry.export.failed` visible out of band | `deploy/observability/scripts/drive.sh` + stop the collector container | Yes |
| F-8 slowloris and malformed input (bar F5) | refusal within the binding's bound, RSS bounded | header read ≤ 30 s; body ≤ configured limit; RSS growth ≤ limit × connections | `scripts/adversarial/`, campaign `hostile/` | Yes |

## Latency

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| L-1 server CPU per blocking send, echo, 2 server cores, 1 conn | µs CPU / request, median of ≥ 6 interleaved repetitions, with range | ≤ 157 µs (0.14.1's own figure, table 4) as the first gate; ≤ 120 µs (a2a-rs tuned, table 2 after) as the stretch | `benches/sdk-comparison/scripts/run_perf.sh` with the pin moved | **CPU time per request: marginally** (the document calls it "good to a few percent"); throughput and p99: **No** — needs a dedicated or bare-metal host, or CI's `ubuntu-latest` only for relative A/B |
| L-2 wake-ups per request | eventfd `write` calls per request under `strace -f -c` | ≤ 0.2 (0.14.1 measured 0.11, a2a-rs 0.04, this tree 2.2) | the strace recipe of §2.3 | **Yes** (a count, not a time) |
| L-3 instructions per request | callgrind Ir per blocking send, memcpy share reported separately | ≤ 320,000 (0.14.1) | `benches/sdk-comparison/scripts/callgrind_per_request.sh` | **Yes** (deterministic) |
| L-4 allocations per request | allocations and bytes per blocking send and per streamed event | PROPOSED after a first count; gate at +5% (the tolerance `memory_overhead.rs` already uses) | new bench arm using the counting allocator already in `benches/` | **Yes** |
| L-5 R3 on every binding and store | p99 of a trivial blocking send, 8 clients | the bar's < 25 ms, kept; add PostgreSQL < 100 ms (rationale: 22.9 ms p50-class cost at 300 posts in finding 7, ×4 headroom) | `tests/readiness_bar/` parameterised | Only as pass/fail against a loose bar |
| L-6 PostgreSQL round trips per send | statements per new task / continuation / streamed event | ≤ 8 / ≤ 11 (current constants), then lower as work lands; per event ≤ 2 | `tests/statement_count.rs` (needs `PGSSLMODE=disable`, handoff `:76-79`) | **Yes** |
| L-7 time to first event, streaming | request sent → first SSE frame, p50 | PROPOSED after measurement; with a real model the SDK overhead must stay inside the model's variance (`:210-212`) | `scripts/llm_e2e.py` with ≥ 100 samples | p50 marginal; tails No |

## Memory

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| M-1 per completed request | RSS growth per request (bar C1) | ≤ 1 KiB kept; report the measured 56–58 B | `tests/readiness_bar/` | Yes |
| M-2 per parked task (in-memory store) | bytes per `input-required` task with a 10-message history | PROPOSED after first measurement | new arm with the counting allocator from `tests/event_byte_budget.rs` | Yes |
| M-3 per idle subscriber stream, per binding | heap bytes per open idle SSE / WebSocket / gRPC stream | PROPOSED after first measurement; budget 10,000 × value ≤ 25% of the pod limit | new | Yes |
| M-4 buffered events | peak heap with a stalled subscriber, budget set | ≤ budget + one event (ADR 0047 A) | `tests/event_byte_budget.rs` | Yes |
| M-5 replica at rest and after 10^4 tasks | RSS | ≤ 64 MiB at rest (the manifest's request), ≤ 128 MiB after 10^4 (measured ~33 MiB) | `benches/sdk-comparison/scripts/run_mem.sh`; qualification row | Yes |

## Cold start

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| C-1 process start to first 200 on `/ready`, in-memory | wall time, median of 20 | PROPOSED after measurement | new script: exec `deploy-agent`, poll `/ready` | Median only |
| C-2 the same on PostgreSQL with an existing schema, and 30/60 replicas at once | wall time; schema-check statements | all replicas ready within the startup probe budget 90 s (`deployment.yaml` 2 s × 45) | `c1_chaos.py` 30-replica row, timed | Yes for the bound |
| C-3 fleet first contact | card fetch p95 for 512 fresh clients | ≤ 1 s (measured 1.04 s p95) with a shared pool / cache (ADR 0051) | `tests/swarm_scale/` card arm | Bound only |

## Telemetry cardinality

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| T-1 server series per replica | active series from one replica after the full operation × binding × outcome matrix | a fixed, published number with a CI gate at +0 without a changelog line | drive `tests/operation_matrix`, scrape `Telemetry::prometheus()`, count lines | **Yes** |
| T-2 hostile inputs | series after 10^4 distinct methods, tool names, models, tenants, peers | unchanged from T-1 except documented `_OTHER` rows (17 per model-labelled metric, measured) | campaign `hostile/` + a new client arm for `server.address` | **Yes** |
| T-3 per-tenant dimension | series growth with `with_tenant_dimension(n)` | ≤ (n + 1) × base of the three labelled metrics (ADR 0038 §4) | unit test in `srv/otel/control_tests.rs` | **Yes** |
| T-4 spans per request | spans exported per blocking send / per streamed event / per store call | published count; no per-event span | in-memory span exporter test | **Yes** |
| T-5 telemetry overhead | CPU per send with default features and nothing installed vs `default-features = false` | ≤ 5% (measured 4.6% and 6.6%) by callgrind Ir, not time | `send_latency_breakdown` under callgrind | **Yes** via Ir |


