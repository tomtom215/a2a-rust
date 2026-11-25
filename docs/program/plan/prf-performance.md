<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# PRF: performance

Read [`../README.md`](../README.md) §6 first, then E05 §2 (the measured gap, the profiles, the hot path hop
by hop) and §5 "Performance" (items P-1 to P-8, which the packages below follow one for one, each with its
evidence, files and acceptance check).

Abbreviations: `S/` is `crates/a2a-protocol-server/src/`.

## Rules for this workstream

1. **A performance claim is a measurement**: the harness named, before and after with the range over at
   least three runs, the host and the build profile. "Should be faster" is not a result.
2. **Gate on counts.** On a session container the meaningful quantities are wake-up writes per request
   (target L-2), instructions per request under callgrind (L-3), allocations per request (L-4) and
   statements per send (L-6). Server CPU per request (L-1) is reported, not gated, there. Throughput and
   latency tails need a dedicated host; say which host produced any such number.
3. **Every cost candidate in E05 §2.5 is CONJECTURED.** Measure it with PRF-01's harness before changing
   code. If the count does not move, the package ends with that result recorded; do not land the change.
4. **Two of these touch a security control or a guarantee** (the header-read bound, the terminal gate). The
   tests of the control pass unchanged, or the package stops.
5. **What was already tried and rejected is recorded** in `docs/swarm-orchestration.md` (G4) and
   `docs/sdk-comparison-2026-11-11.md`. Read both before proposing a restructuring: running the executor in
   the collector's task on every runtime, the collection in the request's own future, the SSE body reading
   the queue directly, coalescing SSE frames and boxing every large future were each measured and not
   landed.

The figures to beat, from `docs/sdk-comparison-2026-11-11.md` (not re-measured here): blocking send at one
connection, a2a-rs 7,757 requests per second at 107 µs of server CPU; this tree 4,228 at 228 µs; 0.14.1
6,090 at 157 µs. Wake-up writes per request: this tree 2.2, 0.14.1 0.11, a2a-rs 0.04.

---

## PRF-01: Deterministic gates that see what the comparison saw

- **Goal.** A regression in wake-ups, instructions or allocations per request fails CI, on any runner.
- **Evidence.** E05 §5 P-6; §2.7 (the gate compares two in-process benchmarks at a 50% threshold and "hides
  the cost"; gRPC, WebSocket, PostgreSQL, the client, delegation fan-out, cold start and idle memory are not
  benchmarked).
- **Gates.** DEC-03. **Depends.** none. **Release.** internal. **Do this first.**
- **Read first.** `.github/workflows/benchmarks.yml`, `benches/scripts/check_regression.py`,
  `benches/sdk-comparison/README.md` and `benches/sdk-comparison/scripts/` (`run_perf.sh`,
  `callgrind_per_request.sh`), `benches/benches/memory_overhead.rs` (the counting allocator),
  `crates/a2a-protocol-server/tests/statement_count.rs`.
- **Do.**
  1. One script per count, each printing a single number with its command: instructions per blocking send
     and per streamed event (callgrind), allocations and bytes per send and per event (the counting
     allocator), eventfd writes per request (`strace -f -c`; the recipe is in E05 §2.3).
  2. Record the baseline at HEAD: three runs each, the range, the host.
  3. Add each to `benchmarks.yml` with a band of plus or minus 5% and a committed baseline file; the
     comparison against a2a-rs moves to a nightly job that gates nothing and publishes medians with ranges.
  4. A probe in `scripts/prove_workflow_gates_fail.py` for each: a planted extra `serde_json::to_value` on
     the send path turns the gate red.
- **Touch.** `benches/`, `.github/workflows/benchmarks.yml`, `scripts/prove_workflow_gates_fail.py`,
  `scripts/lib/workflow_inventory.py`, `docs/program/target-sheet.md` (L-2, L-3, L-4 gain their first
  figures).
- **Accept.** Each script's three runs agree within 1% for instructions and exactly for allocation counts
  (they are deterministic; if they are not, find out why before gating). The probes report PROVEN.
- **Out of scope.** Changing any library code.

## PRF-02: No timer armed per request for the header read (P-1)

- **Goal.** The slowloris bound holds with one timer per connection, not one per request.
- **Evidence.** E05 §5 P-1 (76 of 257 sampled wake-up writes come from hyper's per-request `Sleep`); ROADMAP
  *Performance* item 1.
- **Gates.** none. **Depends.** PRF-01. **Release.** `[0.16.x]`.
- **Read first.** `S/serve/graceful/connection.rs`, `S/serve/graceful/idle/mod.rs`,
  `S/serve/graceful/config.rs`, ADR 0042, the slowloris tests the P-1 text names.
- **Do.** As P-1 describes: extend the per-connection idle timer to bound an incomplete request head and
  pass no per-request header timeout to hyper when it does.
- **Touch.** The three files; tests.
- **Accept.** P-1's (a), (b) and (c) as written. (b) is the control: the existing tests pass unchanged and a
  new test closes a head trickled at one byte per second within the bound.
- **Out of scope.** gRPC and WebSocket listeners (ADR 0052: they do not apply these two `ServeConfig`
  bounds; that is ROADMAP *Developer experience* item 3).

## PRF-03: No timer in `settled()` (P-2)

- **Goal.** Waiting for the consumer to apply the last position arms no timer on the common path.
- **Evidence.** E05 §5 P-2 (50 of 257 wake-ups under the `timeout_at` in
  `S/streaming/event_queue/applied.rs`).
- **Gates.** none. **Depends.** PRF-01. **Release.** `[0.16.x]`.
- **Read first.** `S/streaming/event_queue/applied.rs`, `S/streaming/event_queue/in_memory.rs`, the doc
  comment of `settled` (which names the tests of 2026-10-13 it exists for).
- **Do.** As P-2 describes: check the position synchronously first; arm a timer only when the first poll is
  pending.
- **Accept.** P-2's checks as written, including the new test that a consumer which never applies still
  releases the turn within `write_timeout`. State cancellation safety for the wait.
- **Out of scope.** The terminal gate's design (kept by decision; `docs/swarm-orchestration.md` G4).

## PRF-04: Serialize the JSON-RPC envelope and parse the params once (P-3)

- **Evidence.** E05 §5 P-3 (CONJECTURED cost; allocation is about a quarter of self time in the profile).
- **Gates.** none. **Depends.** PRF-01. **Release.** `[0.16.x]`; no public type changes.
- **Read first.** `S/dispatch/jsonrpc/response.rs`, `S/dispatch/jsonrpc/mod.rs`, the WebSocket equivalent,
  the golden response tests.
- **Do.** Measure first (rule 3). Then as P-3: a private borrowing envelope serialized straight to bytes;
  raw params on the server's private request type. The public `JsonRpcRequest` stays.
- **Accept.** P-3's checks: allocations and instructions per send before and after; golden responses byte
  for byte unchanged; the unknown-params warning test passes.
- **Out of scope.** A zero-copy request model through the handler.

## PRF-05: One serde pass per streamed event (P-4)

- **Evidence.** E05 §5 P-4 (CONJECTURED: the size check, the SSE frame and the SQL log each serialize).
- **Gates.** none. **Depends.** PRF-01. **Release.** `[0.16.x]`.
- **Read first.** `S/streaming/event_queue/mod.rs`, `S/streaming/event_queue/in_memory.rs`,
  `S/streaming/sse.rs`, ADR 0047 (the byte budget the retained bytes must be charged to).
- **Do.** Measure first. Then as P-4, with the retained bytes charged to the budget.
- **Accept.** P-4's checks: `python3 scripts/check_streaming_linearity.py` passes; counts per streamed
  event before and after; `crates/a2a-protocol-server/tests/event_byte_budget.rs` bound unchanged; target
  M-4 (peak heap with a stalled subscriber) not worse.
- **Out of scope.** Changing the event's public type.

## PRF-06: Metric recordings on the send path that allocate nothing (P-5)

- **Evidence.** E05 §5 P-5 (four recordings per send, each allocating; default telemetry measured at 4.6 to
  6.6% of a send).
- **Gates.** none. **Depends.** PRF-01. **Release.** `[0.16.x]`; a metric deprecation is `[0.17]`.
- **Read first.** `S/otel/mod.rs`, `S/rpc_span/mod.rs`, `S/store/traced.rs`,
  `scripts/check_otel_metrics_coverage.py`, ADR 0038.
- **Do.** As P-5: static attribute values, a small fixed table of pre-built attribute sets. Deprecating
  `a2a.server.request.duration` in favour of `rpc.server.call.duration` is a separate, labelled step.
- **Accept.** P-5's checks: target T-5 by instruction count; `python3
  scripts/check_otel_metrics_coverage.py` and the operation-matrix tests pass; `promtool test rules`
  unchanged.
- **Out of scope.** Removing a metric.

## PRF-07: Re-measure against a2a-rs and 0.14.1, and publish

- **Goal.** A dated comparison of the tree after PRF-02 to PRF-06, with the limits of the host stated.
- **Evidence.** E05 §2.1; E02 §2 item 2 and "Staleness" (the 2026-11-11 document predates 0.16.0);
  ROADMAP *Performance* item 1 (X-14).
- **Gates.** none. **Depends.** PRF-02, PRF-03. **Release.** a document.
- **Read first.** `docs/sdk-comparison-2026-11-11.md` whole (the method, the variance section, the
  interleaving), `benches/sdk-comparison/README.md`.
- **Do.** The same matrix, the same method, a2a-rs at its current release (record the versions and the
  commit), at least five interleaved repetitions per cell, on an otherwise idle machine. If the only host is
  a session container, publish counts and CPU per request and say that throughput and tails are not
  claimed.
- **Touch.** A new dated document under `docs/`, `benches/sdk-comparison/results/`, `docs/README.md`.
- **Accept.** Every number names its results file. Where a2a-rs is faster the document says so.
  `./scripts/check_benchmark_prose.sh` exits 0.
- **Out of scope.** Tuning a2a-rs.

## PRF-08: The header map, and the third task (P-7, P-8)

- **Goal.** Two leads that are either measured into a package or closed.
- **Evidence.** E05 §5 P-7 (a per-request `HashMap` of every header: CONJECTURED, and the type runs through
  every handler method, so a redesign is the over-engineering risk) and P-8 (121 of 257 wake-ups are
  scheduler hand-offs; one core closes most of the gap to 0.14.1; the in-request collection was rejected at
  minus 21%).
- **Gates.** P-8 needs a dedicated host and a design the maintainer approves. **Depends.** PRF-07.
- **Do.** P-7: measure with PRF-01's instruction count; if confirmed, build the map lazily behind the same
  signature, or do nothing. P-8: measure the one candidate P-8 names; propose nothing without the number.
- **Accept.** For each: a before and after count, and either a landed change with unchanged tests or a
  recorded "measured, not landed" with the figures, in the style of `docs/swarm-orchestration.md` G4.
- **Out of scope.** Changing a public handler signature.
