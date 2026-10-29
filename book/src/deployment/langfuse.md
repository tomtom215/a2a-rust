<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Langfuse

[Langfuse](https://langfuse.com) ingests OpenTelemetry traces and has SDKs
for Python and JavaScript; for every other language it points at its OTLP
endpoint. An agent built on this SDK needs nothing Langfuse-specific to use
that endpoint — its spans already carry the conventions Langfuse maps — only
configuration, which `Langfuse::from_env()` reads from the three variables
Langfuse's own SDKs read and `Telemetry::builder().with_langfuse(..)` applies.
A Rust agent is configured exactly like the Python agents next to it. This
page runs a Langfuse, points an agent at it, says exactly what the preset
sets and what you see, and what has not been verified.

The preset defaults to a Langfuse you run yourself, at
`http://localhost:3000` (the constant `LANGFUSE_SELF_HOSTED`). That is
deliberate: Langfuse's own SDKs default to its cloud service, and an agent
configured from this page sends nothing off the machine unless
`LANGFUSE_BASE_URL` names somewhere else.

## Run Langfuse

Langfuse is open source and its repository ships a `docker-compose.yml`. From
a clone of `langfuse/langfuse`, these variables make it create an
organisation, a project with known keys and a user on first start;
`TELEMETRY_ENABLED=false` turns off Langfuse's own usage reporting:

```bash
cat > langfuse.env <<'ENV'
TELEMETRY_ENABLED=false
LANGFUSE_INIT_ORG_ID=local
LANGFUSE_INIT_PROJECT_ID=local
LANGFUSE_INIT_PROJECT_PUBLIC_KEY=pk-lf-local
LANGFUSE_INIT_PROJECT_SECRET_KEY=sk-lf-local
LANGFUSE_INIT_USER_EMAIL=you@example.com
LANGFUSE_INIT_USER_PASSWORD=change-me-please
ENV
docker compose --env-file langfuse.env up -d
```

The UI is then at `http://localhost:3000`. Before running it anywhere but a
laptop, follow Langfuse's self-hosting documentation
(<https://langfuse.com/self-hosting>): the compose file's own comments mark
the secrets to change. Langfuse's port 3000 is also the port the
[observability stack's](./observability/walkthrough.md) Grafana publishes, so
run one or the other on a machine, or remap one.

## Point an agent at it

```toml
a2a-protocol-sdk   = "0.15"   # `otlp`, which Langfuse needs, is a default feature
tokio              = { version = "1", features = ["full"] }
tracing-subscriber = "0.3"
```

```bash
export LANGFUSE_PUBLIC_KEY=pk-lf-local     # the project's keys
export LANGFUSE_SECRET_KEY=sk-lf-local
export LANGFUSE_BASE_URL=http://localhost:3000   # the default; set it for any other instance
```

```rust,no_run
use a2a_protocol_sdk::server::otel::{Langfuse, Telemetry};
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

`Langfuse::from_env()` fails with `TelemetryError::Config` naming
`LANGFUSE_PUBLIC_KEY` or `LANGFUSE_SECRET_KEY` when one is missing; the error
never contains a key's value, and the preset's `Debug` output redacts the
secret. To take the keys from somewhere other than the environment — a secret
store, a configuration file — build the preset with `Langfuse::new`:

```rust,no_run
use a2a_protocol_sdk::server::otel::{LANGFUSE_SELF_HOSTED, Langfuse, Telemetry};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let (public_key, secret_key) = ("pk-lf-local", "sk-lf-local");
// The base URL, then the project's public and secret keys.
let langfuse = Langfuse::new(LANGFUSE_SELF_HOSTED, public_key, secret_key);
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent")
    .with_langfuse(langfuse)
    .build()?;
telemetry.install_subscriber();
# Ok(())
# }
```

### The runnable example

`examples/langfuse-agent` is a runnable version: an orchestrator agent that
delegates to a worker agent, both traced into one Langfuse trace, with no
tracing code in either. From a checkout of this repository, with the Langfuse
above running:

```bash
LANGFUSE_PUBLIC_KEY=pk-lf-local LANGFUSE_SECRET_KEY=sk-lf-local \
cargo run -p langfuse-agent -- "summarise the quarterly report"
```

Without `LANGFUSE_PUBLIC_KEY` it exports to whatever `OTEL_EXPORTER_OTLP_*`
names, and exports nothing, without an error, when none is set: it builds
`Telemetry` with `with_export_where_configured()`, and the preset counts as
the traces endpoint. [The example's page](../examples/langfuse-agent.md) walks
through it.

## What the preset sets

Every item below was read from Langfuse rather than assumed: the endpoint and
transports from its OpenTelemetry integration page, the rest from the
`langfuse/langfuse` source at `1a21a42` (2026-10-06).

| Setting | Value |
|---|---|
| Traces endpoint | `{LANGFUSE_BASE_URL}/api/public/otel/v1/traces` (a trailing `/` on the base URL is ignored) |
| Transport | OTLP/HTTP with protobuf bodies — Langfuse's documentation says "gRPC is not supported yet" |
| Headers | `Authorization: Basic base64(public:secret)` and `x-langfuse-ingestion-version: 4`, the header Langfuse's documentation says keeps directly ingested spans from being delayed |
| Metrics and logs | Off by default |

* **Metrics and logs are off** because Langfuse accepts OTLP metrics and
  discards them (`web/src/pages/api/public/otel/v1/metrics/index.ts` is a
  handler that does nothing), and has no OTLP logs route. To keep them, set
  `OTEL_METRICS_EXPORTER=otlp` / `OTEL_LOGS_EXPORTER=otlp` and point
  `OTEL_EXPORTER_OTLP_ENDPOINT` at a collector: traces still go to Langfuse,
  the other two to the collector.
* **The preset's endpoint is set in code**, so for traces it wins over
  `OTEL_EXPORTER_OTLP_ENDPOINT`. Its two headers are added to those in
  `OTEL_EXPORTER_OTLP_HEADERS`; a header of the same name in the variable (say
  `Authorization`) replaces the preset's, so leave those out.
* To send traces somewhere else, do not apply the preset.

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
The spans and their attributes are listed in the
[Spans reference](./observability/spans.md#the-executors-run).

A delegation chain is one trace. Measured: a caller's root span, the call to
an orchestrator agent, the orchestrator's server span and `invoke_agent`, its
call to a worker agent, the worker's server span and `invoke_agent` — seven
observations, each the child of the one before, in a single trace, with no
propagation code in either agent: the client sends its own span as the
`traceparent` (see [Distributed tracing across agents](./observability/trace-context.md)).

## Langfuse Cloud

The same preset works with Langfuse's hosted service: set `LANGFUSE_BASE_URL`
to the region's address (`https://cloud.langfuse.com`,
`https://us.cloud.langfuse.com`) and use that project's keys, or pass the same
URL to `Langfuse::new`. Nothing else changes. As noted below, this SDK has not
been run against it.

## When you already run a collector

Leave the preset off, point `OTEL_EXPORTER_OTLP_ENDPOINT` at the collector,
and let the collector forward traces to Langfuse with its `otlphttp`
exporter and the same two headers. Metrics and logs then go wherever the
collector sends them.

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
