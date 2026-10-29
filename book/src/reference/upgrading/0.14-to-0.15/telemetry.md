<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Telemetry, dashboards and alerts

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## `a2a.server.latency` is gone; units are UCUM

Dashboards and alerts on `a2a.server.latency` (Prometheus
`a2a_server_latency_seconds`) read `a2a.server.request.duration`
(`a2a_server_request_duration_seconds`) instead: the same measurement under
the conventions' name, with the conventions' current buckets, 0.1 ms to 100 s
(`a2a.server.latency` used 5 ms to 10 s; the other two call-duration
histograms moved too, see
[Dashboards, alerts and rate-limit tables](#dashboards-alerts-and-rate-limit-tables)).
A stored series' `le` labels and quantiles mix both sets until the old series
ages out. It was deprecated in 0.14.0. Units of counts are brace-annotated
(`{request}`); the other Prometheus names are unchanged.

## `Telemetry` is the `otlp` feature; `otel` is on by default

On `a2a-protocol-server` directly, `Telemetry`, the `Langfuse` preset and
`init_otlp_pipeline` need `features = ["otlp"]`; `otel` now means the
instrumentation alone (`OtelMetrics`, the span bridge) and is a default
feature of the server and the client. On `a2a-protocol-sdk` both are default
and nothing changes. A build that must not link the OpenTelemetry API sets
`default-features = false`.

## `init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` take `Option<&str>`

Wrap the service name in `Some`. It now names the service only when
`OTEL_SERVICE_NAME` (or `service.name` in `OTEL_RESOURCE_ATTRIBUTES`) does
not; a deployment that set both used to get the argument and now gets the
environment's value. `None` leaves the name to the environment and the SDK's
`unknown_service` default.

## Dashboards, alerts and rate-limit tables

- **Error labels.** `a2a.server.errors`' `error` label and the audit trail's
  `call` records said `protocol` for every A2A error a binding sent as it was.
  They now say `unauthenticated` or `permission_denied` for a refused
  credential, and the error code's label (`version_not_supported`,
  `content_type_not_supported`, ...) for any other.
- **Histogram buckets.** `rpc.server.call.duration`, `rpc.client.call.duration`
  and `a2a.server.request.duration` use the semantic conventions' current
  buckets, 0.1 ms to 100 s, where they used 5 ms to 10 s. The boundaries are
  part of a series' identity, so a stored histogram's quantiles across the
  upgrade mix the two sets until the old series ages out.
- **The executor's span** is `invoke_agent {card name}` where it was
  `a2a.execute`. An alert or query on the old name matches the new one, or the
  handler keeps the old span:

  ```rust
  use a2a_protocol_server::RequestHandlerBuilder;

  fn old_span_name(builder: RequestHandlerBuilder) -> RequestHandlerBuilder {
      builder.with_agent_span_conventions(false)
  }
  ```

- **`error.type`** on `rpc.server.call.duration` and the server span marks a
  server failure only, by each binding's conventions: every JSON-RPC error
  code; a 5xx but not a 4xx on HTTP+JSON; `UNKNOWN`, `DEADLINE_EXCEEDED`,
  `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE` and `DATA_LOSS` on gRPC. It was
  set for every status but success. `rpc.status_code` still carries every
  status, so a dashboard counting client errors reads that instead.
- **Rate-limit keys.** `RateLimitInterceptor` keys are `id:<identity>`,
  `ip:<address>`, `tenant:<id>` and `anon`; an identity was the bare key. A
  caller no interceptor identified shared one `"anonymous"` bucket, and with no
  trusted proxy hops it is now keyed by its peer address (an IPv6 address by
  its /64). A `RateLimitCounter` table sees the new spellings, and rows keyed
  the old way age out with their window.
- **Trace propagation.** With `otel` and a recording `tracing-opentelemetry`
  layer, the client sends a `traceparent` on every call, naming its own span,
  so the server span a call causes is the child of the client's. Opt out with
  `ClientBuilder::with_trace_propagation(false)`.
