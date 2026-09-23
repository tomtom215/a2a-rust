<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0013: Spans, metrics and one telemetry entry point

**Date:** 2026-09-23
**Status:** Proposed
**Author:** Tom F.

---

## Context

The 2026-09-22 adopter audit (`docs/adopter-audit-2026-09-22.md`, section 1)
found observability absent where adopters look for it first:

- **No spans exist in any crate** (O1). A grep for `span!`, `info_span`,
  `#[instrument]`, `.instrument(` or `Span::current` over `crates/` returned
  nothing.
- **The span id sent downstream belongs to no recorded span** (O2). The
  server mints a child span id (`handler/helpers.rs`, `fresh_span_id`) and
  propagates it, so every downstream agent's trace points at a parent no
  backend has.
- **Work runs on `tokio::spawn` with no span** (O4), so even a user's own
  outer span does not parent the executor.
- **The latency histogram records seconds into millisecond buckets** (O5),
  and its names are not the semantic conventions'.
- **OTLP setup covers part of `OTEL_*`**, is gRPC-only, lets the argument
  override `OTEL_SERVICE_NAME`, and nothing flushes on shutdown (O12).
- The client records nothing (O8), and outbound `traceparent` is opt-in
  twice over (O9).

Trace context *propagation* is correct and tested (W3C, all nine binding
pairs against a2a-go when opted in). What is missing is recording.

The semantic conventions this ADR commits to were read from the
`open-telemetry/semantic-conventions` repository at `838e414`
(2026-09-22), not from memory:

- `rpc.server.call.duration`: histogram, unit `s`, stability
  release-candidate, advisory buckets
  `[0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1, 2.5, 5, 7.5, 10]`.
- RPC server span: kind `SERVER`, named `{rpc.method}`; `rpc.system.name`
  required; `rpc.method` the fully-qualified logical method, or `_OTHER`
  for one the server does not recognise; `error.type` required on failure.
- `rpc.system.name` lists `grpc` and `jsonrpc` among its values and allows
  custom ones ("If one of them applies, then the respective value MUST be
  used; otherwise, a custom value MAY be used").

## Options

### 1. What records spans

**(a) `tracing` spans, exported through `tracing-opentelemetry`.** The
crates already log through `tracing` (behind the `tracing` feature); spans
become part of the same instrumentation, and an executor's own
`tracing::info!` inherits the request's span and trace id with no extra
work. `tracing-opentelemetry` 0.33.0 depends on `opentelemetry ^0.32`, the
version the workspace locks (checked against the crates.io index), with
`rust-version` 1.75, below the 1.88 MSRV.

**(b) The `opentelemetry` API directly.** No bridge, exact control of span
ids. But an executor's logs would not correlate with the spans, and every
adopter who already runs a `tracing` subscriber — the Rust default — would
have two unconnected systems.

**(c) Both, behind separate features.** Twice the instrumentation to keep
consistent, for no case (a) cannot serve.

**Chosen: (a).** Spans are `tracing` spans and compile to nothing without
the `tracing` feature, exactly as the log macros do today. The `otel`
feature turns on `tracing` and adds the bridge.

### 2. What the server span is on each binding

**(a) An RPC `SERVER` span per A2A method on every binding**, named
`lf.a2a.v1.A2AService/{Method}` — the fully-qualified method the gRPC
binding already serves — with `rpc.system.name` `grpc`, `jsonrpc` (the
JSON-RPC and WebSocket bindings), and the custom value `a2a_http_json` for
the REST binding, which the semantic conventions' RPC list does not cover.

**(b) An HTTP server span for REST**, with `http.*` attributes and a child
`INTERNAL` span for the A2A method. Truer to the HTTP conventions, but the
same A2A call then has a different span shape on each binding, and a
dashboard grouping by method has to know which.

**Chosen: (a)**, with `http.request.method` and `http.route` added on the
REST span so HTTP-oriented tooling still finds them. One span shape per A2A
call, whatever the binding, is what makes a cross-binding, cross-language
trace comparable.

### 3. Which span id goes downstream (O2)

The inbound `traceparent` becomes the server span's remote parent; the
downstream `traceparent` carries **the recorded span's own id**, read back
from the span, not a separately minted one. When no subscriber records spans
(the `tracing` feature off, or no OpenTelemetry layer installed), the
current minting stays: there is nothing to point at either way, and trace
ids still join the chain. `InboundTracePolicy` applies unchanged.

### 4. Metrics

`rpc.server.call.duration` is added with the advisory buckets above. The
existing `a2a.server.*` instruments stay, documented as deprecated, and are
removed no earlier than the next minor after the one that adds their
replacements — `STABILITY.md` §3's deprecate-first rule, applied to a
catalogue the book publishes as a contract. Task outcome, stream duration and
active streams are added as new instruments.

### 5. One entry point

`init_telemetry()` reads the `OTEL_*` environment as the OpenTelemetry
specification defines it — protocol (gRPC or HTTP/protobuf), endpoint,
headers, timeout, service name and resource attributes, with the
environment winning over defaults — installs tracer and meter providers,
and returns a guard whose drop, or explicit `shutdown`, flushes both. The
existing `init_otlp_pipeline*` functions stay, deprecated.

## Consequences

- The gate is built first: an end-to-end test with an in-memory span
  exporter and a manual metric reader that drives JSON-RPC, REST and gRPC
  calls and asserts the span tree (remote parent, executor child, downstream
  span id recorded) and every catalogued instrument. It fails on `main` on
  the day this ADR is written; each step of the work turns part of it green.
- `tracing` spans cost nothing when the feature is off and a disabled-level
  check when it is on without a subscriber; the cost with an exporter
  installed is measured before and after, not asserted.
- Adding dependencies: `tracing-opentelemetry`, and the `trace` and
  `http-proto` features of the OpenTelemetry crates already in use. Each is
  checked against `deny.toml` before it lands.

## Open questions

- ~~Whether `tracing` becomes a default feature of the server and SDK
  crates.~~ **Decided 2026-09-23 by the maintainer: yes, with `tracing`'s own
  default features.** Measured before deciding: `tracing` was already in every
  default build's dependency graph through the HTTP stack, so the feature adds
  `tracing-attributes` and `syn`. The client takes it by default too: its
  failure paths report through `tracing` like the server's, and a default
  client build that dropped them was O13's other half. The SDK now takes the
  client and server without their own defaults (audit K1), so
  `default-features = false` on any of the three removes it again.
