<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Configuring `Telemetry`

This page is the code side of exporting: which Cargo features you need, how
to build `Telemetry` and install its subscriber, what each builder method
does, how flushing and shutdown report a loss, and how a handler's metrics
reach the exporter. The environment side — every `OTEL_*` variable and which
setting wins — is [Environment variables](./environment.md).

## Turning them on

```toml
a2a-protocol-sdk = "0.16"   # `tracing`, `otel` and `otlp` are default features
```

* **`tracing`** — on by default in all three crates; with
  `default-features = false` the logging calls and spans compile to nothing.
  With it, your binary still has to install a subscriber; the library emits
  events and spans and does not decide where they go.
* **`otel`** — on by default in the client, the server and the SDK:
  `OtelMetrics`, and each span's OpenTelemetry bridge, so the server's
  `SERVER` span takes the caller's `traceparent` as its parent and each client
  call's `traceparent` names the call's own span (see
  [Spans reference](./spans.md)). The OpenTelemetry API only: recorded to the
  provider your process installs, or to nothing.
* **`otlp`** — makes `Telemetry` available: OTLP export of traces, metrics and
  logs, configured by the standard `OTEL_*` environment. On by default in the
  SDK only; on `a2a-protocol-server` directly, ask for it
  (`features = ["otlp"]`).
* **`otel-prometheus`** — off by default, in the SDK as well: serves the
  metrics at `/metrics` for a Prometheus that scrapes
  (`OTEL_METRICS_EXPORTER=prometheus`; see
  [Prometheus](./prometheus.md#scrape-the-agent-no-collector)).
* **`otlp-json`** — off by default: OTLP/HTTP with JSON bodies
  ([below](#json-bodies)).

## Building it and installing the subscriber

`Telemetry` (ADR 0013, option 5) builds one OTLP exporter per signal, on the
transport the environment names, installs the tracer and meter providers and
the W3C `tracecontext` and `baggage` propagators as the process-global ones,
and returns a guard whose drop — or explicit `shutdown` — flushes all three.
`telemetry.layer()` is the `tracing` layer that turns spans into OpenTelemetry
spans and events into OpenTelemetry log records; add it next to whatever other
layers you run, or call `telemetry.install_subscriber()` to install it as the
process's only subscriber (it never replaces one already set, and returns
`false` when one was). The subscriber it installs also prints warnings and
errors to standard error and logs `telemetry.describe()` once; an application
with a subscriber of its own gets the same two things from
`telemetry.describe()` and `telemetry.notices()`.

A binary that must run as well without a collector as with one builds it with
`with_export_where_configured()`: each signal is then built only when an
endpoint names where it goes — `OTEL_EXPORTER_OTLP_ENDPOINT`, the signal's own
`OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` / `_METRICS_` / `_LOGS_ENDPOINT`, or
`with_otlp_endpoint` in code — and a signal with none has no exporter and no
export thread. Every agent and CLI binary in this repository does this, except
the few that start no telemetry at all, which `scripts/check_otel_wiring.py`
lists with the reason for each (`examples/langfuse-agent` does it too: with
`LANGFUSE_PUBLIC_KEY` set the preset names the traces endpoint, and without it
and without an endpoint nothing is built):

```rust,no_run
use a2a_protocol_sdk::server::otel::Telemetry;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent")
    .with_export_where_configured()
    .build()?;
telemetry.install_subscriber();
# Ok(())
# }
```

With a subscriber of your own, add the layer instead:

```rust,no_run
use a2a_protocol_sdk::server::otel::Telemetry;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent") // OTEL_SERVICE_NAME wins over it
    .build()?;
tracing_subscriber::registry().with(telemetry.layer()).init();

// Serve, and on the way out:
telemetry.shutdown()?; // or let it drop: both flush
# Ok(())
# }
```

Built plainly, as here, `Telemetry` exports every signal — to `localhost`
(port 4318, or 4317 over gRPC) when no endpoint is named — so an export with
nothing listening where it goes is lost. `shutdown` returns an error naming the
signal and the count, and a warning says so as it happens wherever a layer
that prints warnings is installed, as `install_subscriber` does (the registry
above has none; see [Debugging telemetry](./debugging.md#when-something-is-wrong)).
`Telemetry::init()` is the same as `Telemetry::builder().build()`.

Each process also reports a `service.instance.id`, a random UUID unless
`OTEL_RESOURCE_ATTRIBUTES` names one: it is what tells replicas of one service
apart (Prometheus turns it into the `instance` label). See
[Resource attributes](./resource.md).

To send traces to Langfuse instead of a collector, see
[Langfuse](../langfuse.md).

## The builder

Every `TelemetryBuilder` method, what it sets and what it defaults to. Where an
`OTEL_*` variable sets the same thing, [Environment variables](./environment.md#which-setting-wins)
says which one wins.

| Method | What it does | Default |
|---|---|---|
| `with_default_service_name(name)` | The `service.name` when the environment names none | `unknown_service` (the OpenTelemetry SDK's) |
| `with_traces(bool)`, `with_metrics(bool)`, `with_logs(bool)` | Whether the signal is exported when its `OTEL_*_EXPORTER` is unset | On; metrics and logs off with `with_langfuse` |
| `with_export_where_configured()` | Builds a signal only where an endpoint is configured for it | Off: every signal is built |
| `with_log_level(Level)` | The least severe event exported as a log record; spans are not filtered by it | `INFO` |
| `with_otlp_endpoint(url)` | The collector's base URL for every signal; OTLP/HTTP appends `/v1/traces`, `/v1/metrics`, `/v1/logs` | From the environment, else `localhost` |
| `with_otlp_protocol(OtlpProtocol)` | `Grpc`, `HttpProtobuf`, or `HttpJson` with `otlp-json` | From the environment, else `HttpProtobuf` |
| `with_otlp_header(name, value)` | A header sent with every export, alongside `OTEL_EXPORTER_OTLP_HEADERS` | None |
| `with_compression(OtlpCompression::Gzip)` | gzip every export body | From the environment, else none |
| `with_proxy(url)`, `without_proxy()` | One `http://` proxy for every destination, ignoring `NO_PROXY`; or no proxy at all | The `*_PROXY` variables, as curl reads them |
| `with_ca_certificate_pem(pem)` | CA certificates trusted alongside the bundled Mozilla roots | Mozilla roots only |
| `with_client_certificate_pem(chain, key)` | A client certificate and key for mutual TLS | None |
| `without_resource_detection()` | Leaves `host.*`, `os.type` and `process.*` out of the resource | Detected |
| `without_startup_summary()` | Leaves the startup line out of what `install_subscriber` logs; warnings still print | Logged |
| `with_langfuse(Langfuse)` | Traces to Langfuse ([Langfuse](../langfuse.md#what-the-preset-sets)) | Off |
| `install_globally(bool)` | Whether the providers and the W3C propagators become the process-global ones | On |
| `build()` | Builds the exporters; fails on a value it cannot act on | — |

## Without global providers

`install_globally(false)` builds the exporters and changes nothing global: no
global tracer or meter provider, no global propagator. It is for a test, or
for a process that installs providers of its own and wants this one only for
some of its signals. `telemetry.layer()` still records spans and log records
through this telemetry's own providers, and `telemetry.otel_metrics()` gives
an `OtelMetrics` recording through its meter provider, for a handler:

```rust,no_run
use a2a_protocol_sdk::server::RequestHandlerBuilder;
use a2a_protocol_sdk::server::otel::Telemetry;

fn wire(
    builder: RequestHandlerBuilder,
) -> Result<(RequestHandlerBuilder, Telemetry), Box<dyn std::error::Error>> {
    let telemetry = Telemetry::builder()
        .with_default_service_name("my-agent")
        .install_globally(false)
        .build()?;
    // Keep `telemetry` alive as long as the handler: dropping it flushes and
    // shuts its providers down.
    Ok((builder.with_metrics(telemetry.otel_metrics()), telemetry))
}
```

Only the instruments `OtelMetrics` records take that route — the catalogue on
the [landing page](../observability.md#the-catalogue). Everything else records
to the global meter provider as it stands at each measurement:
`rpc.client.call.duration`, the agent card's `http.server.request.duration`,
the webhook's `http.client.request.duration`, the task store's
`db.client.operation.duration` and the tool loop's `gen_ai.*` instruments
(`ToolLoop::with_meter` sends the last to a meter you choose). With
`install_globally(false)` and no global provider of your own, those record to
OpenTelemetry's no-op meter. With metrics off, `otel_metrics()` records into a
no-op meter too.

The providers themselves are `tracer_provider()`, `meter_provider()` and
`logger_provider()`, each `None` when its signal is off.

## How metrics reach the exporter

`Metrics` is a trait with a **default no-op implementation for every method**.
What a handler records to by default is `OtelMetrics` on the global meter
provider; with no provider installed that records nothing and reports no
problem. That is correct — a library should not force a metrics backend — but
it means "no data" and "misconfigured exporter" look identical from the
outside. [Debugging telemetry](./debugging.md#is-the-sdk-recording-at-all)
shows how to tell them apart.

With `otel`, `OtelMetrics` records the catalogue, and with `otlp` `Telemetry`
exports it over OTLP. `otel` is a default feature, and with it a handler given
no `with_metrics` records through `OtelMetrics` instruments on the global
meter provider as it stands at each measurement, built again when the process
installs another: install `Telemetry` before or after building the handler and
nothing else is needed. The client's `rpc.client.call.duration` binds the same
way.

`with_metrics` *replaces* that default, so a handler given a counter of its
own stops exporting `rpc.server.call.duration` and the `a2a.server.*`
instruments. To count something of your own beside the export, use
`.with_additional_metrics(my_metrics)`: each callback goes to the default (or
to whatever `with_metrics` set), then to yours. The trait's callbacks are
listed in the [Metrics reference](./metrics.md#the-metrics-trait).

## Flushing and shutting down

Dropping `Telemetry` flushes and shuts every provider down, best-effort.
`shutdown()` does the same and returns what it can see failed;
`force_flush()` exports everything recorded so far without shutting down.
Either is safe on any thread, inside a Tokio runtime of either flavour or
outside one, and may block while a collector is slow: each of the three
providers is given the OpenTelemetry SDK's 5 s shutdown timeout, one after the
other ([Quick start](./quick-start.md#stopping) has the measured times).

`build`, `shutdown` and `force_flush` return `TelemetryError`:

| Variant | When |
|---|---|
| `Config { variable, value, reason }` | A variable, or a value given in code, this crate cannot act on |
| `Exporter { signal, source }` | `opentelemetry-otlp` refused to build an exporter |
| `Tls(String)` | The TLS configuration could not be built |
| `Runtime(io::Error)` | The export thread could not be started |
| `Export(Vec<String>)` | A flush or shutdown failed, or exports were lost since the last report, per signal |

## What it handles for you

Things it gets right that hand-wiring tends not to, each covered by a test (in
`crates/a2a-protocol-server/tests/telemetry_export/` or the unit tests beside
the code), except the one that `deny.toml` enforces:

* **It works from anywhere**: a plain `fn main`, a `current_thread` runtime,
  a test. Export runs on a private runtime of its own, so building the gRPC
  exporter does not need yours, and shutting down from inside a
  `current_thread` runtime neither deadlocks nor loses the final flush.
  (The obvious hand-wiring — `opentelemetry-otlp`'s default blocking HTTP
  client under a simple span processor, inside Tokio — panics with "Cannot
  drop a runtime in a context where blocking is not allowed" and exports
  nothing; that was observed, not predicted.)
* **OTLP/HTTP without reqwest.** Export goes over this crate's own hyper and
  rustls (ring) stack; `deny.toml` keeps reqwest out of the graph.
* **TLS to a gRPC collector verifies against real roots.** For an `https://`
  endpoint `opentelemetry-otlp` 0.33.1 builds an empty `ClientTlsConfig`, which
  in tonic 0.14 enables no roots; `Telemetry` passes the Mozilla roots and
  any extra CA explicitly.
* **The exporter's own stack is never exported.** Spans and events from
  `opentelemetry`, `opentelemetry_sdk`, `opentelemetry_otlp`, `opentelemetry_http`,
  `hyper`, `hyper_util`, `h2`, `tonic`, `tower`, `rustls` and this crate's own
  `otel::telemetry` module are filtered out of the layer, so an export cannot
  produce telemetry that needs exporting. They still reach your other layers,
  so a `fmt` layer still shows an export error.
* **A lost export is reported, on either transport.** `opentelemetry-otlp`
  retries a failed export (four attempts by default, with backoff), and when
  it gives up the SDK's batch processor logs the failure and drops the batch;
  its `shutdown` returns `Ok` regardless (`opentelemetry_sdk` 0.33.1).
  `Telemetry` counts each export its exporter gave up on, and `shutdown` and
  `force_flush` return `TelemetryError::Export` naming the signal and the
  count. An export refused once and delivered on retry is not counted.
* **Log records are `INFO` and above** by default (`with_log_level` changes
  it); spans are not filtered by level here. See [Logs](./logs.md).

## JSON bodies

JSON bodies (`http/json`) are the `otlp-json` feature, off by default even in
`a2a-protocol-sdk`: `opentelemetry-otlp`'s `http-json` feature also makes JSON
the protocol of every exporter in the process that is built without one, and
Cargo's feature unification would impose that on an application's own
exporters.

## Older API: `init_otlp_pipeline`

`init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` predate
`Telemetry` and stay for existing callers. They export **metrics only**, over
**gRPC only** (`OTEL_EXPORTER_OTLP_PROTOCOL` has no effect on them), must be
called from inside a Tokio runtime — outside one they panic with `there is no
reactor running` — and their `service_name` argument, an `Option<&str>`, names the service
only when `OTEL_SERVICE_NAME` (or `service.name` in
`OTEL_RESOURCE_ATTRIBUTES`) does not, as the environment specification
prescribes (before 0.15 it overwrote them; see the
[upgrade guide](../../reference/upgrading/0.14-to-0.15/telemetry.md#init_otlp_pipeline-and-init_otlp_pipeline_with_endpoint-take-optionstr)).
Calling one twice replaces the first provider and silently orphans it. New
code should use `Telemetry`.
