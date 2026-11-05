<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0038: Observability decisions for 0.16.0 (abandoned calls, the scrape endpoint, `Telemetry` ergonomics, console logs; baggage and control-plane metrics proposed)

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0, by the maintainer's instruction of 2026-11-04
("all open plan items are in 0.16.0; implement the best engineering option,
record the reasoning, the maintainer can veto in review"), for sections 2, 3,
5 and 6 and the C-13 counter, which are built. Sections 1 (C-10) and 4 (H11)
are **proposed and not built**: the `obs-sdk` work package did not reach
them, and they stay open for the maintainer.
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

**Proposed (not built): (2).** Baggage is a data channel to whoever the next hop is, which
for an A2A agent is often a third party; forwarding it by default would send
an upstream service's `user.id=` or `tenant=` entries to every remote agent.
The fork F3 position says the same. So:

* `ClientBuilder::with_baggage_propagation(true)` writes the current
  OpenTelemetry context's baggage as the W3C `baggage` header on every
  header-carrying binding (JSON-RPC, HTTP+JSON, gRPC). Off by default.
* `RequestHandlerBuilder::with_baggage_propagation(true)` reads an inbound
  `baggage` header into the context the call's span (and so the executor)
  runs in, under `InboundTracePolicy::Continue` only (`Restart` and `Drop`
  discard it, as they discard the caller's trace). Off by default.
* Both go through the **global text-map propagator** for the `baggage` field
  alone, so `OTEL_PROPAGATORS` decides the format: `Telemetry` installs
  `tracecontext,baggage` by default, and a process whose `OTEL_PROPAGATORS`
  leaves `baggage` out sends and accepts none even when opted in.
* `OTEL_PROPAGATORS=none`, or a list that names neither `tracecontext` nor an
  unknown name only, stops the client writing `traceparent`; a list that names
  only names this SDK lacks falls back to the default, as `Telemetry` does.
* WebSocket carries no per-request headers; unchanged.

None of this section is built in 0.16.0 so far: baggage still dies at every
A2A hop and `OTEL_PROPAGATORS` still changes only the global propagator.
B3 and Jaeger propagators stay out (reported as notices, as before).

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

**Proposed (not built): B, and A opt-in with an allow-list.** Every new hook has a default
empty body, so a `Metrics` implementation outside this crate compiles
unchanged. The label values are bounded constants. No task, root, job or
message id is ever a metric label (ADR 0034 §4). The tenant attribute is off
unless `OtelMetricsBuilder::with_tenant_dimension(allowed)` names the tenants
to label; every other tenant is recorded as `_OTHER`, so the series count is
bounded by the list the operator wrote. None of this is built: the hooks
touch the control plane's code, which other work packages own in this release.

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
items with an `error.type` and the wrapper sees batches. Process metrics (RSS,
CPU time, open descriptors) are not built.

## What is not built, and why

* **Exemplars (C-05, SDK half).** `opentelemetry_sdk` 0.33.1, the version
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
* **Process metrics** (RSS, CPU time, open descriptors; C-13's other half).
* **Tokio runtime metrics** need `--cfg tokio_unstable`; not built.
