<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Langfuse

[Langfuse](https://langfuse.com) ingests OpenTelemetry traces and has SDKs
for Python and JavaScript; for every other language it points at its OTLP
endpoint. An agent built on this SDK needs nothing Langfuse-specific to use
that endpoint — its spans already carry the conventions Langfuse maps — only
configuration, which `Telemetry` reads from the three variables Langfuse's own
SDKs read. A Rust agent is configured exactly like the Python agents next to
it.

## Setup

```toml
a2a-protocol-sdk = { version = "0.14", features = ["otel"] }
```

```bash
export LANGFUSE_PUBLIC_KEY=pk-lf-...
export LANGFUSE_SECRET_KEY=sk-lf-...
export LANGFUSE_BASE_URL=https://cloud.langfuse.com   # the default; or us.cloud…, or http://localhost:3000
```

```rust,no_run
use a2a_protocol_server::otel::{Langfuse, Telemetry};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent")
    .with_langfuse(Langfuse::from_env()?)
    .build()?;
tracing_subscriber::registry().with(telemetry.layer()).init();

// ... build the handler, serve ...

telemetry.shutdown()?;
# Ok(())
# }
```

`examples/langfuse-agent` is a runnable version: an orchestrator agent that
delegates to a worker agent, both traced into one Langfuse trace.

## What the preset sets

Every item below was read from Langfuse rather than assumed: the endpoint and
transports from its OpenTelemetry integration page, the rest from the
`langfuse/langfuse` source at `1a21a42` (2026-10-06).

* **Traces** go to `{LANGFUSE_BASE_URL}/api/public/otel/v1/traces` over
  OTLP/HTTP with protobuf bodies — Langfuse's documentation says "gRPC is not
  supported yet" — with `Authorization: Basic base64(public:secret)` and
  `x-langfuse-ingestion-version: 4`, the header Langfuse's documentation says
  keeps directly ingested spans from being delayed.
* **Metrics and logs are off.** Langfuse accepts OTLP metrics and discards
  them (`web/src/pages/api/public/otel/v1/metrics/index.ts` is a handler that
  does nothing), and has no OTLP logs route. To keep them, set
  `OTEL_METRICS_EXPORTER=otlp` / `OTEL_LOGS_EXPORTER=otlp` and point
  `OTEL_EXPORTER_OTLP_ENDPOINT` at a collector: traces still go to Langfuse,
  the other two to the collector.
* The preset's endpoint and headers are set in code, so for traces they win
  over `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_HEADERS`.

## What you see

Measured against a self-hosted Langfuse 4.53.0 (`docker compose up` from the
`langfuse/langfuse` repository), with the SDK's own spans and nothing added:

| Span | Langfuse shows | Because |
|---|---|---|
| `invoke_agent {name}` (the executor) | an **AGENT** observation | `gen_ai.operation.name = invoke_agent` (`ObservationTypeMapper.ts`, priority 3) |
| `lf.a2a.v1.A2AService/SendMessage`, server and client | a SPAN | no GenAI operation; the A2A attributes land under `metadata.attributes` |
| every span of a context | one **session**, the A2A `contextId` | `gen_ai.conversation.id` (`extractSessionId`) |
| `gen_ai.input.messages`, `gen_ai.output.messages` | the observation's input and output | the same JSON-string shape as Langfuse's own OpenTelemetry GenAI fixture |

The last row needs `with_span_content_capture(true)` on the handler builder —
it copies what users and agents say into Langfuse, so it is off by default.

A delegation chain is one trace. Measured: a caller's root span, the call to
an orchestrator agent, the orchestrator's server span and `invoke_agent`, its
call to a worker agent, the worker's server span and `invoke_agent` — seven
observations, each the child of the one before, in a single trace, with no
propagation code in either agent: the client sends its own span as the
`traceparent` (see [Observability](./observability.md#trace-context)).

## What was not verified

* **Langfuse Cloud.** Only the self-hosted 4.53.0 above; the preset's cloud
  URL and region names come from Langfuse's documentation.
* **A chain that crosses languages.** `traceparent` is W3C Trace Context, so
  a Python or JavaScript agent traced into the same Langfuse project should
  join the same trace; that has not been run. Note that Langfuse's Python SDK
  v4 exports only spans it recognises as LLM-related by default
  (`should_export_span`), which filters what *that* process sends, not what a
  Rust agent sends.
* **Langfuse's UI.** The table above is what Langfuse's public API returned
  (`/api/public/v2/observations`), not screenshots of its interface.

## When you already run a collector

Leave the preset off, point `OTEL_EXPORTER_OTLP_ENDPOINT` at the collector,
and let the collector forward traces to Langfuse with its `otlphttp`
exporter and the same two headers. Metrics and logs then go wherever the
collector sends them.
