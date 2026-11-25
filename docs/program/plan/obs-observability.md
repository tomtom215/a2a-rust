<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# OBS: observability

Read [`../README.md`](../README.md) §6 first, then E05 §1: the inventory of spans, instruments, propagation
per binding, dashboards and rules (§1.1 to §1.5), the open register rows re-checked against the code
(§1.6), and the fleet gaps (§1.7). §5 "Observability" holds items O-1 to O-8, which the packages below
follow, each with its evidence, files and acceptance check.

Abbreviations: `S/` is `crates/a2a-protocol-server/src/`, `C/` is `crates/a2a-protocol-client/src/`.

## Rules for this workstream

1. **No unbounded label.** Never a task id, a root id, a peer address or a tenant without an allow-list as a
   metric label. Lineage is a caller's claim (ADR 0034 decision 4): a span attribute, never a label.
2. **A new signal is a defaulted `Metrics` hook**, and `CONTRIBUTING.md` *Extending a public trait* applies
   whole: the doc paragraph on what the default costs, every implementation in the repository updated in
   the same commit, and `python3 scripts/check_otel_metrics_coverage.py` green.
3. **Every alert has a rule test** that fires and one that does not (`promtool test rules`), and every
   dashboard panel passes the stack check (`deploy/observability/scripts/`).
4. **A published series count** (targets T-1 and T-2) gates the workstream: a package that adds series says
   how many, and the count is a CI number, not prose.
5. **No second metrics facade.**

---

## OBS-01: The series budget, as a gate

- **Goal.** The number of active series one replica exports over the whole operation, binding and outcome
  matrix is a published number that cannot grow without a changelog line.
- **Evidence.** E05 §4 targets T-1, T-2, T-4; §1.7 new finding (`rpc.client.call.duration` labels
  `server.address` with no bound, `C/call_metric.rs`).
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`. **Do this first.**
- **Read first.** The operation-matrix tests under `crates/a2a-protocol-sdk/tests/operation_matrix/`, the Prometheus
  exporter under `S/otel/telemetry/`, `S/rpc_span/tool_metrics.rs` (the first-N-then-`_OTHER` scheme),
  ADR 0038 §4.
- **Do.**
  1. A test that drives the matrix, scrapes the in-process Prometheus exporter and counts series; commit
     the count as a baseline the test compares against.
  2. A hostile arm: 10^4 distinct methods, tool names, models, tenants and peers; the count is unchanged
     but for the documented `_OTHER` rows.
  3. Bound `server.address` on the client metric with the same scheme, or make it opt-in.
- **Touch.** Tests, `C/call_metric.rs`, `CHANGELOG.md` (*Fixed*: the unbounded label).
- **Accept.** The hostile arm fails before step 3 and passes after. The baseline count is in the test and
  in `docs/program/target-sheet.md` T-1.
- **Out of scope.** Adding signals.

## OBS-02: A failed telemetry export is visible out of band (S4)

- **Evidence.** E05 §5 O-2; ROADMAP *OpenTelemetry* item 1 (`a2a.telemetry.export.failed` exists and no
  rule or dashboard reads it).
- **Gates.** none. **Depends.** none. **Release.** `[0.16.x]`.
- **Do.** As O-2: the alert and an absence rule; one rate-limited WARN per failed batch class; an
  informational `telemetry` field in the `/ready` body that never makes it 503.
- **Touch.** `deploy/observability/prometheus/rules/`, their tests, `S/otel/telemetry/`, the generated
  Kubernetes objects under `deploy/kubernetes/`.
- **Accept.** O-2's checks: a rule test that fires and one that does not; target F-7 run on the container
  (the collector stopped for ten minutes: no failed request, bounded memory, the failure visible on the
  scrape).
- **Out of scope.** A second export path.

## OBS-03: The gauges and histograms a fleet operator is missing

- **Evidence.** E05 §5 O-3 and §1.7: no admission queue or wait metric, no database pool metric, no outbox
  depth or age, no event-log lag, no halt or lease propagation latency; client retries, ejections and
  hedges are logs only (register rows M10, M11, M12).
- **Gates.** none. **Depends.** OBS-01. **Release.** `[0.17]`, additive.
- **Read first.** O-3's list with its file anchors; ADR 0045 and ADR 0046 ("not built" sections); ADR 0050;
  `S/admission/`, `S/handler/load.rs`, the outbox worker under
  `S/handler/event_processing/background/push_delivery/`.
- **Do.** One pull request per group, in this order, each with its alert and panel: (1) admission (queue
  capacity, running, queued, wait, refusal causes); (2) database pools and store spans for the stores that
  have none; (3) push outbox depth and oldest age; (4) event-log lag; (5) halt and lease propagation
  latency; (6) client retries, hedges, ejections and endpoint states, and card, key-set and token-exchange
  durations. For (3), the sampling adds statements: state the count per poll per replica.
- **Touch.** Per group; `S/metrics/`, `S/otel/`, `deploy/observability/`.
- **Accept.** O-3's checks: the series gate of OBS-01 with each group's stated increase; the coverage
  script; one rule test per new alert; the dashboard check. `tests/statement_count.rs` constants unchanged
  for a send.
- **Out of scope.** Per-root or per-job metrics.

## OBS-04: Objectives and burn-rate alerts

- **Evidence.** E05 §5 O-4 (no burn-rate rule exists); ADR 0039 (what counts as a server-class failure, and
  the latency bucket).
- **Gates.** none. **Depends.** none. **Release.** `[0.17]` (alert names change).
- **Do.** As O-4: recording rules at four windows for availability and latency; the fast and slow pairs
  replace the two threshold pages; the objective is a rule-file variable.
- **Accept.** O-4's checks: a fast-burn series pages within the short window; a slow leak only tickets; the
  stack check passes.
- **Out of scope.** Choosing an adopter's objective.

## OBS-05: One job, one view, across a split trace

- **Goal.** Every span of a delegation tree can be found by its root, including across a WebSocket hop and
  a resumed turn.
- **Evidence.** E05 §5 O-5 and §1.4 (delegation is parent and child through `traceparent`; WebSocket hops
  are links only; lineage root and depth sit on the executor span alone); E03 §3 G22 (no delegation span,
  no client-side delegation metric).
- **Gates.** none. **Depends.** none; SWM-09 documents it. **Release.** `[0.17]`, additive.
- **Read first.** `S/rpc_span/` (the server span and the lineage attributes on the executor span),
  `C/call_span.rs`, `C/delegation/origin.rs`, `C/delegation/journal.rs`, ADR 0013, ADR 0034.
- **Do.** As O-5: lineage root and depth on the server and client RPC spans when the call carries the
  extension; a span link from a resumed or handed-off turn to the turn it continues (the journal entry
  keeps the previous trace context); a delegation span on the client; one trace query and one panel, "job
  by root".
- **Accept.** O-5's check: an in-memory exporter test over three hops, one over WebSocket and one resumed,
  asserts one root attribute on every span and one link on the resumed span. The series gate is unchanged
  (attributes are not labels).
- **Out of scope.** Storing a tree. `lineage::tree` stays a pure function over tasks the caller gathers.

## OBS-06: The register rows with a mechanical test

- **Evidence.** E05 §5 O-7 and §1.6: M6 (WebSocket upgrade refusals unmetered), H3 (no span or duration on
  SLIMRPC multicast), M8 (tonic's own refusals record nothing), store `error.type` as SQLSTATE class,
  W2-LIVE-8 (a provider's error body reaches a span the book says carries no content); ROADMAP
  *OpenTelemetry* item 2 and *Tool use* item 1.
- **Gates.** M7 only (an API decision, E06 §6 row 23); skip it. **Depends.** none. **Release.** `[0.16.x]`.
- **Do.** One commit per row, each with a row in the operation-matrix tests or the binding's telemetry
  tests that fails first. Re-check L1 and M15, which E05 did not.
- **Accept.** Each row's test fails before and passes after; each register row carries the commit.
- **Out of scope.** M7.

## OBS-07: Token accounting by tenant

- **Evidence.** E05 §5 O-6 (no tenant dimension on the GenAI token metrics).
- **Gates.** none (a quota, S3, is a separate decision). **Depends.** OBS-01. **Release.** `[0.17]`.
- **Do.** As O-6: the GenAI meter takes the same allow-listed tenant dimension the server metrics already
  have.
- **Accept.** Target T-3 (series growth at most (n + 1) times the base for n allow-listed tenants); a
  recording rule by tenant with its test.
- **Out of scope.** Enforcing anything.

## OBS-08: Refresh the GenAI conventions against upstream

- **Evidence.** E05 §5 O-8 and summary (the conventions are pinned by upstream commit; whether they are
  current was not checked).
- **Gates.** none. **Depends.** none. **Release.** an ADR amendment first; a rename is `[0.17]`.
- **Do.** Read the pinned commits against upstream today; record each attribute as kept, renamed or added,
  with the upstream commit, in an amendment to ADR 0038 before changing a name.
- **Accept.** The amendment lists every attribute; the stack check and the GenAI dashboard queries are
  updated in the same change as any rename.
- **Out of scope.** Following a convention that is not yet stable upstream without saying so.
