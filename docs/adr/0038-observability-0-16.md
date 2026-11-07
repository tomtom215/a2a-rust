<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0038: Observability decisions for 0.16.0 (baggage, abandoned calls, the scrape endpoint, control-plane metrics, `Telemetry` ergonomics, console logs, self and process metrics)

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0, by the maintainer's instruction of 2026-11-04
("all open plan items are in 0.16.0; implement the best engineering option,
record the reasoning, the maintainer can veto in review"), for every section; each says what is built.
**Author:** Tom F.
**Amends:** ADR 0013 (observability).

---

## Context

The triage of 2026-11-01 (`docs/audits/2026-11-01-triage/c.md`, `DECISIONS.md`)
left six observability rows needing a choice: C-10 (baggage and
`OTEL_PROPAGATORS` on the A2A wire), C-09 (how a call the peer abandons is
recorded), C-16 (the Prometheus scrape listener), H11 (tenant and control-plane
metrics), G-14 (`Telemetry` ergonomics) and C-06 (structured console logs).
Fork F3 (decided 2026-11-01) already set the rule for configuration: a bad
`OTEL_*` value warns and falls back, and only a value the SDK cannot do
without fails `build()`. Where the OpenTelemetry specification or semantic
conventions speak, this ADR follows them; where they are silent, it says so.

The semantic-conventions text cited below was read from
`open-telemetry/semantic-conventions` `main` on 2026-11-04
(`docs/http/http-spans.md`, `docs/rpc/grpc.md`, `docs/rpc/rpc-spans.md`).

## 1. W3C baggage and `OTEL_PROPAGATORS` (C-10)

Options (from `DECISIONS.md#c-10`): (1) inject and extract everything through
the global propagator; (2) keep the hard-coded W3C trace context, add opt-in
baggage, honour `OTEL_PROPAGATORS` for "do not inject"; (3) document only.

**Decision, built: (2).** Baggage is a data channel to whoever the next hop
is, which for an A2A agent is often a third party; forwarding it by default
would send an upstream service's `user.id=` or `tenant=` entries to every
remote agent. The fork F3 position says the same. So it is **opt-in and
allow-listed by key**, on each side separately:

* `ClientBuilder::with_baggage_propagation(BaggagePolicy)` (and the same on
  `ClientConfig`) writes the call span's OpenTelemetry baggage as the W3C
  `baggage` header on every header-carrying binding (JSON-RPC, HTTP+JSON,
  gRPC), only the keys `BaggagePolicy::keys([...])` names, or every key with
  `BaggagePolicy::All`. `Off` by default. A `baggage` header the caller set on
  the request is kept.
* `RequestHandlerBuilder::with_baggage_propagation(BaggagePolicy)` puts the
  allowed entries of an inbound `baggage` header into the context the call's
  span runs in, so the executor and the calls it makes carry them, under
  `InboundTracePolicy::Continue` only (`Restart` and `Drop` discard it, as
  they discard the caller's trace). `Off` by default.
* Bounds are the W3C Baggage limits (§3.3.2): at most 64 whole members and
  8192 bytes; an inbound header over 8192 bytes is dropped whole, a malformed
  member skipped. Baggage is never a metric attribute.
* The codec is this crate's own (`a2a_protocol_types::trace_context::baggage`),
  not the global text-map propagator: the client crate depends on the
  OpenTelemetry API only, and a process with no SDK installed (the global
  propagator a no-op) would otherwise send nothing even when opted in.
* `OTEL_PROPAGATORS` is read by the client when its `ClientConfig` is made
  and by the server when the handler is built (`with_propagators` overrides
  it on either): without `tracecontext` the client writes no `traceparent` of
  its own and the server does not continue the caller's (`Continue` acts as
  `Restart`); without `baggage` no baggage is read or written; `none` turns
  off both. Names neither carries (`b3`, `jaeger`, `xray`, `ottrace`; the
  pinned crates ship none of them) are warned about and ignored, and a list of
  only such names gets the default (F3).
* WebSocket carries no per-request headers; unchanged. The opt-in
  `TracePropagationInterceptor` and the push sender's `traceparent` are not
  governed by `OTEL_PROPAGATORS`: the first is installed by choice, the second
  is a webhook, not an A2A peer.

## 2. A call the peer abandons (C-09)

The recording rules were fixed in 43e142b; the spans and the raw metric still
marked an abandoned call `ERROR` with `error.type=cancelled`, and gRPC recorded
`rpc.status_code=cancelled` (lower case, not a gRPC status name).

What the conventions say: `grpc.md`, server span, `rpc.status_code`: only
`UNKNOWN`, `DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE`,
`DATA_LOSS` "SHOULD be considered errors"; `rpc-spans.md`, `error.type`: "If the
RPC fails with an error before status code is returned, `error.type` SHOULD be
set to … a component-specific, low cardinality error identifier";
`http-spans.md`: a request "cancelled intentionally by the caller … SHOULD NOT
be treated as an error: the span status SHOULD be left unset" (written for
client instrumentation; the server side of the same cancellation is the same
event).

**Decision.** No binding marks an abandoned call's span `ERROR`. On gRPC the
call records `rpc.status_code=CANCELLED` and no `error.type`, exactly as
`grpc.md` has it. On JSON-RPC, HTTP+JSON and custom bindings, which have no
status for a call that never answered, `error.type=cancelled` stays on the span
and the metric as the component-specific identifier — which the shipped
recording rules already leave out of every error ratio and count in
`a2a:rpc_server_cancelled:rate5m` — with the span status unset, as a server
span's 4xx is. A dashboard that wants gRPC's abandoned calls too selects
`rpc_status_code="CANCELLED"` (the `obs-stack` package owns the rules).

## 3. The Prometheus scrape endpoint (C-16)

Options: (1) leave; (2) a handler for the A2A server's own port, and limits on
the separate listener; (3) bind `0.0.0.0` under Kubernetes.

**Decision: (2).** The default host stays the specification's `localhost`
(option 3 widens exposure silently). The separate listener gets a connection
cap (16) and closes each connection after one response, with the 10 s header
deadline it had and a 30 s bound on the whole exchange, so a client that opens
connections and sends nothing cannot hold the port. `Telemetry::prometheus()`
returns a `PrometheusScrape` handle that renders the same text, for an
application to serve at `/metrics` behind its own authentication on its own
port; `TelemetryBuilder::without_prometheus_listener()` keeps the registry and
binds nothing.

## 4. Tenant and control-plane metrics (H11)

Options (from `DECISIONS.md#h11`): (A) a bounded tenant attribute in the
OpenTelemetry implementation; (B) new default-no-op `Metrics` hooks for the
control plane; (C) derive everything from spans in a collector.

**Decision, built: B, and A opt-in with an allow-list.** Four hooks with
empty default bodies, so a `Metrics` implementation outside this crate
compiles unchanged, and forwarded by `Arc<T>`, `Tee` and the global
forwarder: `on_refusal(cause)` at each refusal before work (`stream_cap`,
`tenant_cap`, `push_config_cap`, `idempotency_store`, `event_log`,
`halt_store`), `on_lease("lapsed")` in the lease watcher, `on_halt(scope,
action, stopped)` at halt and resume, `on_claim("reaped")` when the reaper
ends a task. `OtelMetrics` counts them as `a2a.server.refusals`,
`a2a.server.lease.events`, `a2a.server.halts`,
`a2a.server.halt.stopped_tasks` and `a2a.server.claim.events`, in the
`a2a.server.*` family the existing instruments use (the other packages of this
release add hooks, `on_push_delivery` outcomes, but no instrument names of
their own). The label values are bounded constants (`metrics::control`).

Dimensions: `OtelMetrics::with_tenant_dimension(allowed)` labels the RPC,
executor and refusal metrics with `a2a.tenant`, `_OTHER` for a tenant not
allowed, so the series count is bounded by the list the operator wrote; the
call's tenant reaches `on_rpc_call` through `RpcCall::tenant`, carried out of
the handler's tenant scope by a per-call cell (`rpc_span/call_tenant.rs`).
`with_agent_name(name)` adds `gen_ai.agent.name`. Both off by default. No
task, root, job or message id is ever a metric label (ADR 0034 §4); a job
dimension is therefore not built, and a per-job view stays a trace query.
Budget settlement is not a hook: its meter names are chosen by delegators,
so they are not a bounded label set, and budget-exhausted endings are already
counted (`a2a.server.executor.duration{outcome="budget_exhausted"}`).

## 5. `Telemetry` ergonomics (G-14) and the legacy pipeline (C-12)

* `Telemetry` is `#[must_use]`: dropping it shuts export down, and
  `Telemetry::init()?;` alone on a line did that silently.
* `Telemetry::init()` keeps the specification's defaults (export every signal,
  to `localhost` when nothing names an endpoint): changing it would silently
  stop export for a deployment with a sidecar collector and no `OTEL_*`
  variable. `Telemetry::init_where_configured()` is added for the other case.
* `init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` are deprecated in
  0.16.0 and removed no earlier than 0.17.0 (`STABILITY.md` §3, "deprecate
  first"); the book gives the `Telemetry` equivalent.

## 6. Console logs with trace context (C-06)

The OpenTelemetry compatibility note "Trace Context in non-OTLP Log Formats"
names the fields `trace_id`, `span_id` and `trace_flags` (hex) for a log line
outside OTLP. **Decision:** `TelemetryBuilder::with_console_format(ConsoleFormat::Json)`,
or `A2A_LOG_FORMAT=json`, makes `install_subscriber` print one JSON object per
line on standard error with `timestamp`, `level`, `target`, `message`, the
event's fields and, inside a recorded span, those three. The text format is
unchanged; the `console` exporter's ` trace=` and ` span=` keys (log and span
lines) are renamed `trace_id=` and `span_id=`. The code's choice wins over
`A2A_LOG_FORMAT`; any other value is a notice (F3). The trace context is read
from the OpenTelemetry context `tracing-opentelemetry` activates on span entry
(its default), because a formatter cannot ask `tracing` for the current span
while the event is being delivered.

## 7. Telemetry self-metrics (C-13, part)

`a2a.telemetry.export.failed` (`{batch}`, attribute `signal`) is a cumulative
observable counter of exports that failed after the exporter's retries, fed
by the same wrapper that drives the warnings and `shutdown`'s error. It is
this crate's name, not `otel.sdk.exporter.*.exported`, because those count
items with an `error.type` and the wrapper sees batches.

Process metrics follow the conventions' `docs/system/process-metrics.md`
(`process.cpu.time`, `process.memory.usage`, `process.memory.virtual`,
`process.thread.count`, `process.unix.file_descriptor.count`), read from
`/proc/self` on Linux and absent elsewhere: no dependency (`sysinfo`, `procfs`)
and no `unsafe` for `sysconf`/`getrusage`, at the cost of no series on macOS
and Windows, which is what an absent reading should be rather than a wrong
one. `USER_HZ` is taken as 100, the kernel's user-space ABI value on every
Linux target this crate builds for. The Tokio runtime reports its three
stable `RuntimeMetrics` (workers, alive tasks, global queue depth) under
`tokio.runtime.*`; the per-worker and poll-time metrics need
`tokio_unstable` and are not reported. Both are on by default when metrics
are exported (a process's health is what an operator wants without asking;
eight small series), and `without_process_metrics()` turns them off.

## What is not built, and why

* **Exemplars (C-05, SDK half). C-05 is closed by the stack half plus this
  limit:** metrics-to-traces correlation comes from Tempo's span-metrics
  exemplars (`deploy/observability`, the `obs-stack` package), and the SDK
  half waits on upstream (ROADMAP "Waiting on upstream"). `opentelemetry_sdk` 0.33.1, the version
  pinned in `Cargo.lock`, builds every data point with `exemplars: vec![]` and
  does not read `OTEL_METRICS_EXEMPLAR_FILTER`; the field is `pub(crate)`, so
  neither this crate nor an exporter can add one. `Telemetry` notes the
  variable when it asks for exemplars, saying none are recorded. The stack half
  (span metrics in Tempo) is the `obs-stack` package's.
* **The SDK's own `otel.sdk.processor.*` metrics**, behind
  `opentelemetry_sdk`'s `experimental_metrics_bound_instruments` feature: they
  bind to the global meter provider when each processor is built, which is
  before `Telemetry` installs its provider, so in this crate they would record
  into the no-op meter. `a2a.telemetry.export.failed` (section 7) stands in
  for the export half.
* **Tokio's unstable runtime metrics** (per-worker busy time, poll counts,
  steals) need `--cfg tokio_unstable`; only the stable three are reported.
* **Process metrics outside Linux**: not read (see section 7).
