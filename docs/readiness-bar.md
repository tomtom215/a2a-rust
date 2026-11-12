<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Readiness bar

What "ready for production use" would have to mean for this SDK, written
down **before** anything was measured against it, so the thresholds could not
be tuned to the numbers. Written 2026-09-24. Measurements against it are
recorded in the section at the end, each with the commit, hardware, build
profile and command that produced it; a row with no measurement is unmet, not
assumed.

This is a bar for the **server and client crates as a library** inside a
deployment someone else operates. It says nothing about a particular agent's
executor, whose cost dominates every real latency figure.

## Scope

- **Bindings:** JSON-RPC over HTTP, HTTP+JSON (REST), gRPC, WebSocket.
- **Stores:** in-memory, SQLite, PostgreSQL.
- **Topologies:** one process; two replicas sharing PostgreSQL.
- **Out of scope:** the SLIMRPC binding (never published), TLS termination
  cost, and any executor that calls a model — a model's latency is the
  model's, and is recorded separately in the real-model run.

## Load profiles

| Id | Profile | Shape |
|---|---|---|
| L1 | Steady blocking | 8 concurrent clients, each sending blocking `SendMessage` to an executor that writes one artifact and completes; runs for the stated duration. |
| L2 | Streaming | 32 concurrent `SendStreamingMessage` streams, each receiving 20 events, re-opened as they end. |
| L3 | Churn | Clients connect, send one message, and drop the connection — half before the response arrives. |
| L4 | Overload | 4× the configured concurrency limit (`max_concurrent_streams`, rate limits, `max_connections`). |
| L5 | Continuations | Sequential multi-turn conversations: each turn answers `input-required` the moment it arrives (N21's shape). |

## Service levels

A requirement is met only when it holds on every binding it names. "Loopback"
means client and server in one host over 127.0.0.1.

| Id | Requirement | Threshold |
|---|---|---|
| R1 | No unexpected errors under L1, L2, L5 | 0 errors other than the refusals the profile provokes |
| R2 | Latency does not degrade with time under L1 | last-quarter p99 ≤ 1.5 × first-quarter p99 |
| R3 | Latency of a trivial blocking send, loopback, L1 | p99 < 25 ms on the reference host |
| R4 | Overload is refused, not queued into timeouts (L4) | every refusal is the binding's overload signal (HTTP 503 / gRPC `UNAVAILABLE` / JSON-RPC error), none is a client timeout |
| R5 | A continuation sent the moment `input-required` arrives is admitted (L5) | 0 refusals |
| R6 | Graceful shutdown with streams open | every open stream receives a terminal event or a clean close within `completion_grace` + 1 s |
| R7 | A peer that disconnects mid-stream releases server resources | queue, token and connection gone once the binding sees the peer gone: within one SSE keep-alive (30 s), at the end of the WebSocket read loop or its `idle_timeout`, when the gRPC response channel closes |

## Resource ceilings

| Id | Resource | Ceiling |
|---|---|---|
| C1 | Resident memory under L1 | ≤ 1 KiB growth per completed request (the soak test's existing bound) |
| C2 | File descriptors after L1–L3 end | back to the pre-load count ± 8 within 10 s |
| C3 | Tokio tasks after L1–L3 end | back to the pre-load count within 10 s |
| C4 | Tasks left non-terminal after L1 | 0 |
| C5 | Event queues and cancellation tokens after L3 | 0 left for tasks whose clients are gone, once the binding sees them gone (R7) |

## Failure modes that must be handled

| Id | Fault | Required behaviour |
|---|---|---|
| F1 | Push target slow or refusing | the send path is not delayed; delivery is retried within its budget and then dropped with a log line |
| F2 | PostgreSQL restarted under load | calls fail with a store error while it is down and succeed again without restarting the server |
| F3 | Replica killed mid-stream | a client resuming with `Last-Event-ID` on the other replica receives every event after that id, once |
| F4 | Client dropped mid-stream (every binding) | as R7 |
| F5 | Malformed, oversized or slow input (slowloris, partial frames, oversized headers, compression bombs) | refused within the binding's bound; no unbounded buffering |

## What would move this bar

The thresholds are judgements, and these are the judgements. R3's 25 ms is
about 135 times the 185.7 µs the benchmark page
(`book/src/reference/benchmarks.md`) records for one JSON-RPC send on the
benchmark runner — a different host, sequential rather than 8-way, and a
figure this bar did not re-measure — so that it fails only on a real
regression, not on a busy host. C1's 1 KiB is the soak test's existing bound, kept rather
than tightened. R2's 1.5× is tighter than the soak test's 3.0× because the
soak test must also pass on shared CI runners, and this bar is measured on an
idle host. A reviewer who thinks any of them wrong should say so before the
numbers are read, not after.

## Measurements

Rows R1 to R4 and C1 to C5 were first measured on 2026-11-12, over HTTP
JSON-RPC on loopback, under L1, L3 and L4 only (the last section of this page
records what each run was). **R5, R6, R7 and F1 to F5 have not been measured:
each is unmet, not assumed**, as is every row under L2 and L5, and every row on a
second replica or a real PostgreSQL. The measurement of 2026-10-24 compares a send's
CPU cost across commits; it is recorded here because it closes a question the
handoff carried open since 2026-10-14 (was a send slower at `2b0a3abd` than
at `6ce5dc9a`?). Each measurement states the commit, the host, the build
profile, the exact command, and the spread across runs.

### 2026-10-24: the CPU cost of a blocking send, across commits

**What.** `send_latency_breakdown`, arm `diag/send/echo_3_events/single_worker`
(`benches/benches/send_latency_breakdown.rs`): one blocking JSON-RPC
`SendMessage` over loopback to an executor that emits three events, client
and server in one process on a one-worker runtime. The control arm,
`diag/floor/get_task_404/single_worker`, is the same round trip with no
executor. `benches/src` and the bench file are identical at every commit
below except `775fe46`, where `coordinator.rs`, which these arms do not use,
differs by one line (`git diff`).

**Host and build.** Intel Xeon Processor @ 2.10 GHz, 4 cores, one socket, a
shared virtual machine (kernel 6.18.44); rustc 1.99.1 (58cf8f9c3
2026-10-13); the workspace `[profile.release]` (`lto = true`,
`codegen-units = 1`, `opt-level = 3`, `panic = "abort"`), whose section of
`Cargo.toml` has the same `md5sum` at every commit in the table. Default
features, except where stated.

**Command.** For each commit, `git worktree add --detach <dir> <commit>`,
and in it `cargo bench -p a2a-benchmarks --bench send_latency_breakdown
--no-run --locked`, keeping the binary. Each run was `taskset -c 2 <binary>
--bench <arm> --exact --warm-up-time 2 --measurement-time 5 --noplot`, and
its middle estimate was recorded. A round runs every binary once, and the
order rotates from round to round so that drift in the host reaches all of
them. Every thread of the process shares the one core, so the time per send
is the CPU time per send. Nothing else ran on the host (load average below
1.0 before each run).

**Results**, µs per send: the median of the rounds, then the lowest and
highest round. "Telemetry off" is HEAD with `default-features = false` on
the client and server dependencies of `benches/Cargo.toml`, which removes
`tracing` and `otel`; no subscriber or meter provider is installed in any
run.

| Commit | Run 1 (6 rounds) | Run 2 (6) | Run 3 (6) | Run 4 (12) | Run 5 (8) |
|---|---|---|---|---|---|
| `775fe46` (0.14.1) | | | | | 106.2 (102.0–114.0) |
| `6ce5dc9a` (2026-10-12) | 93.0 (90.6–103.4) | 88.9 (84.2–90.0) | 98.8 (95.6–109.4) | | 91.5 (84.6–95.3) |
| `2b0a3abd` | 97.9 (91.4–108.6) | | 103.6 (97.9–113.6) | | |
| `2a888a4` | | | 106.5 (97.0–110.0) | | |
| `c903593` | | | 102.3 (95.7–134.1) | | |
| `86c8fac` | | | 103.3 (96.0–113.8) | | |
| `6c90d0b` | | | 100.8 (96.5–110.2) | | |
| `4cadc2a` | | | 105.7 (97.2–123.2) | | |
| `2781bb6` (branch base) | | | 109.5 (101.7–116.3) | 105.9 (101.0–117.9) | |
| HEAD (`858b21e`) | 107.9 (103.5–114.3) | 103.1 (94.3–108.2) | 112.9 (106.6–123.8) | 107.4 (103.8–120.9) | 102.0 (101.0–115.9) |
| HEAD, telemetry off | | 98.6 (93.6–102.4) | | | 95.7 (87.4–106.0) |

Control arm (no executor), median µs: run 1: 40.8 at `6ce5dc9a`, 36.5 at
`2b0a3abd`, 38.6 at HEAD; run 2: 40.0, 38.8, 35.8 (telemetry off); run 5:
40.1 at `775fe46`, 39.5 at `6ce5dc9a`, 37.4 at HEAD, 34.8 (telemetry off).

**What the numbers support.** VALIDATED means the table shows it in the runs
named; the ranges of single runs overlap in several comparisons, so these
are directions and rough sizes, not exact percentages.

- VALIDATED (run 5): HEAD is not slower than 0.14.1: 102.0 against
  106.2 µs. The ranges overlap, so this is "not slower", not "faster".
- VALIDATED (runs 1, 2, 3, 5): HEAD is slower than `6ce5dc9a` by 16.0%,
  16.0%, 14.3% and 11.5%. Run 5, the only one that has both, has `6ce5dc9a`
  13.9% faster than 0.14.1, so that commit was an unusually fast point, not
  the baseline of a release.
- No single step between `6ce5dc9a` and HEAD stands out of the noise: run 3
  goes 98.8, 103.6, 106.5, 102.3, 103.3, 100.8, 105.7, 109.5, 112.9 through
  the nine commits, each with a spread of 13–38 µs between its lowest and
  highest round. The suggestion of
  2026-10-14 (a send 6–21% slower at `2b0a3abd` than at `6ce5dc9a`,
  `docs/handoff-history.md`) agrees: +5.2% and +4.9% here.
- This branch (`2781bb6` to HEAD, run 4, 12 rounds each): 105.9 to 107.4 µs,
  +1.4%, the ranges overlapping heavily: not distinguishable from no change.
- Default `tracing` and `otel` with nothing installed cost 4.6% (run 2) and
  6.6% (run 5) of a send, both in the same direction with overlapping
  ranges. The comment on the `default` feature of the server crate said
  "a level check" until this measurement.
- The control arm did not slow (40.8, 40.0 and 40.1 µs at the older
  commits against 38.6, 38.8 and 37.4 at HEAD), so the rise is in what a send
  does after dispatch, not in the transport.

**Not established.** What in the range from `6ce5dc9a` to HEAD causes the
rise (no profiler was run); how much of the telemetry cost is spans and how
much is the default metrics; whether any of it holds on another host, with
a different executor, or on another binding.

### 2026-11-11: what was measured, and a correction

**F1 is measured** (`crates/a2a-protocol-server/tests/push_hang_live_subscriber.rs`, run on this tree): a real TCP peer that reads the webhook request
and never answers; the streaming send that registered it ends in 0.23 s, the executor's own pacing being 0.2 s, and the same test fails (the stream does
not end in 10 s) with push delivery run inline. This meets F1's "the send path is not delayed"; its "retried within its budget and then dropped with a
log line" half is covered by `tests/push_sender_tests.rs` and was not re-measured here.

**Correction to the 2026-10-24 reading.** The section above reads "HEAD is not slower than 0.14.1: 102.0 against 106.2 µs" for one blocking send, and
that figure stands for what it measured: client and server in one process on one worker. Over HTTP between two processes the same comparison
reads differently: with the server on two cores this tree needed 232 µs of server CPU per request at one connection against 157 for 0.14.1 and was 18% to 29%
slower in throughput (two runs); with the server on one core the gap was 4 to 12%
(`docs/sdk-comparison-2026-11-11.md`, tables 2 and 3). The earlier line did not say the gap was absent, and it is now stated what it could not see: the
cross-thread wake-ups of a runtime with more than one worker.

**R1 to R7, C1 to C5, F2 to F5 are still unmeasured**; the throughput matrix above is not any of them (it is a closed-loop echo benchmark, not these load
profiles), and no claim here depends on them.

### 2026-11-12: L1, L3 and L4 over HTTP JSON-RPC (R1 to R4, C1 to C5)

**What.** `crates/a2a-protocol-server/tests/readiness_bar/` drives a real server
(`Server` and `JsonRpcDispatcher`, in-memory store capped at 2,000 tasks as the soak test
does) over real sockets, client and server in one process. L1: 8 clients sending
blocking `SendMessage` to an executor that writes one artifact and completes, for 30 s
after a 200-request warm-up. L3: 2,000 connections, each sending one `SendMessage`,
even-numbered ones closed before the response is read. L4: a handler with
`max_concurrent_streams(4)` and an executor that holds its slot for 300 ms, 16 concurrent
sends. Each row prints its number and verdict before anything is asserted.

**Host and build.** Intel Xeon Processor @ 2.30 GHz, 4 vCPUs, one socket, a shared
virtual machine (kernel 6.18.44); rustc 1.99.1 (58cf8f9c3 2026-10-13). Release profile
with the workspace's `lto = true` and `codegen-units = 1` **turned off** for build time
(`CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
CARGO_PROFILE_RELEASE_STRIP=false`): this is not the profile a published binary has, and
the difference was not measured.

**Commit.** `ba335f90`.

**Command.**

```bash
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
CARGO_PROFILE_RELEASE_STRIP=false A2A_BAR_SECS=30 \
  cargo test -p a2a-protocol-server --release --test readiness_bar -- --ignored --nocapture
```

**Results**, three consecutive runs of that command. Every row was met in every run.

| Row | Bar | Run 1 | Run 2 | Run 3 |
|---|---|---|---|---|
| R1 errors under L1 | 0 | 0 of 396,747 | 0 of 406,848 | 0 of 395,425 |
| R2 last-quarter p99 / first-quarter p99 | ≤ 1.5 | 1.09 | 1.10 | 1.01 |
| R3 p99 of a blocking send, 8 clients | < 25 ms | 1.155 ms | 1.083 ms | 1.163 ms |
| (R3 p50) | | 0.583 ms | 0.575 ms | 0.585 ms |
| C1 RSS growth per request | ≤ 1,024 B | 57.1 | 56.3 | 58.0 |
| C2 fds after L1 and after L3 | pre-load ± 8 | 11 and 11 (before: 11) | same | same |
| C3 tokio tasks after L1 and after L3 | pre-load | 1 and 1 (before: 1) | same | same |
| C4 non-terminal tasks after L1 | 0 | 0 | 0 | 0 |
| C5 live event queues after L3 | 0 | 0 | 0 | 0 |
| R4 refusals, 16 sends against a limit of 4 | all refused with the overload signal, none a timeout | 4 served, 12 refused, 0 other | same | same |

Throughput under L1 was 13,224, 13,562 and 13,181 requests per second.

**What these numbers are not.** One process holds client and server on four shared vCPUs,
over loopback, with an in-memory store and no TLS, so R3 is the cost of the SDK's path
and not of a deployment's. C1's figure includes the harness's own latency record, 16 bytes
per request, so the server's share is lower. A 5 s run gives a higher C1 (210 B per
request) because the store is still filling; the bar's figure needs a run long enough to
leave warm-up behind, and 30 s does. `num_alive_tasks` counted one task (the accept loop)
before and after. R4's refusal is an HTTP 200 carrying a JSON-RPC error of code `-32603`
and a `google.rpc.RetryInfo` of one second, with the text `server at maximum concurrent
stream capacity (4); retry later`: the bar's "a JSON-RPC error" is met, and the code is
InternalError, not a code of its own.

**What this found.** C4 is defined after L1 only. The harness also reads it after L3, for up
to 120 s, because a task a dropped client leaves behind is a different question. With 30,000
connections (half dropped early), **3 of 6 runs left one task `Submitted` for the whole read**,
and nothing ended it. Two windows after a new task's row is saved, both fixed in `ba335f90`
(CHANGELOG, Fixed): the commit guard learned a row existed only when the save returned, and the
send awaited the queue map between its commit and the task that stores the executor's events.
Deterministic tests drop the send at each window and failed on the unfixed code. After both
fixes 0 of 13 runs of the same churn left a task.

Not measured: L2 and L5; R5 (a continuation the moment `input-required` arrives), R6 (graceful
shutdown with streams open), R7 (a peer that disconnects mid-stream), F1 to F5; any binding other
than JSON-RPC; SQLite or PostgreSQL stores; two replicas.

<!-- measured-limits:begin -->
The rows above in machine-readable form, for `scripts/release_evidence.py`, which copies this block into a
release's evidence manifest and fails if it changes after the manifest was made. The prose is the
record; this block repeats its numbers and adds nothing.

```json
{
  "measured_at_commit": "ba335f90",
  "host": "Intel Xeon Processor @ 2.30 GHz, 4 vCPUs, shared VM, kernel 6.18.44",
  "profile": "release, LTO off (not the published profile)",
  "command": "A2A_BAR_SECS=30 cargo test -p a2a-protocol-server --release --test readiness_bar -- --ignored --nocapture",
  "runs": 3,
  "rows": [
    {"id": "R1", "bar": "0 errors under L1", "results": ["0 of 396747", "0 of 406848", "0 of 395425"], "met": true},
    {"id": "R2", "bar": "last-quarter p99 / first-quarter p99 <= 1.5", "results": [1.09, 1.10, 1.01], "met": true},
    {"id": "R3", "bar": "p99 of a blocking send < 25 ms", "results": ["1.155 ms", "1.083 ms", "1.163 ms"], "met": true},
    {"id": "R4", "bar": "overload refused with the signal, none a timeout", "results": ["12 of 16", "12 of 16", "12 of 16"], "met": true},
    {"id": "C1", "bar": "RSS growth <= 1024 bytes per request", "results": [57.1, 56.3, 58.0], "met": true},
    {"id": "C2", "bar": "fds back to pre-load +- 8", "results": ["11 of 11", "11 of 11", "11 of 11"], "met": true},
    {"id": "C3", "bar": "tokio tasks back to pre-load", "results": ["1 of 1", "1 of 1", "1 of 1"], "met": true},
    {"id": "C4", "bar": "0 non-terminal tasks after L1", "results": [0, 0, 0], "met": true},
    {"id": "C5", "bar": "0 live event queues after L3", "results": [0, 0, 0], "met": true}
  ],
  "not_measured": ["R5", "R6", "R7", "F1", "F2", "F3", "F4", "F5", "L2", "L5", "any second replica", "any PostgreSQL or SQLite store", "any binding but JSON-RPC"]
}
```
<!-- measured-limits:end -->
