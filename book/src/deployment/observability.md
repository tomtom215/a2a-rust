<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Observability

Three signals, often confused: **logs** say what happened in one request,
**traces** say how one request travelled — through this agent and every agent
it called — and **metrics** say what is happening across all of them. This SDK
records all three, and since 0.15 all of it is on by default: spans and
metrics are recorded through the OpenTelemetry API (`otel`), and the SDK crate
ships the OTLP exporter (`otlp`). What stays the application's choice is
where they go: nothing is exported until an endpoint is configured, and spans
and logs need a `tracing` subscriber (`Telemetry::install_subscriber` installs
one). If you see no metrics, the answer is almost always this page's first
section. (Until 2026-10-23 this paragraph said exporting was off by default,
which stopped being true in 0.15.)

## Turning them on

```toml
a2a-protocol-sdk = "0.15"   # `tracing`, `otel` and `otlp` are default features
```

* **`tracing`** — on by default in all three crates; with
  `default-features = false` the logging calls and spans compile to nothing.
  With it, your binary still has to install a subscriber; the library emits
  events and spans and does not decide where they go.
* **`otel`** — on by default in the client, the server and the SDK (since
  0.15): `OtelMetrics`, and each span's OpenTelemetry bridge, so the server's
  `SERVER` span takes the caller's `traceparent` as its parent and each client
  call's `traceparent` names the call's own span (see [Spans](#spans)). The
  OpenTelemetry API only: recorded to the provider your process installs, or
  to nothing.
* **`otlp`** — makes [`Telemetry`](#exporting-with-telemetry) available: OTLP
  export of traces, metrics and logs, configured by the standard `OTEL_*`
  environment. On by default in the SDK only; on `a2a-protocol-server`
  directly, ask for it (`features = ["otlp"]`).

With `otlp`, the whole setup is this, and the rest of the page explains it:

```rust,no_run
use a2a_protocol_server::otel::Telemetry;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent") // OTEL_SERVICE_NAME wins over it
    .build()?;
tracing_subscriber::registry().with(telemetry.layer()).init();

// Build handlers after this: one given no `with_metrics` records to the meter
// provider it installed. Serve, and on the way out:
telemetry.shutdown()?; // or let it drop: both flush
# Ok(())
# }
```

To send traces to Langfuse instead of a collector, see
[Langfuse](./langfuse.md).

## Metrics are opt-in twice over

`Metrics` is a trait with a **default no-op implementation for every method**.
An agent with no provider installed records nothing and reports no problem.
That is correct — a library should not force a metrics backend — but it means
"no data" and "misconfigured exporter" look identical from the outside.

You install one provider on the builder:

```rust
# use std::time::Duration;
use a2a_protocol_server::metrics::Metrics;

/// The smallest useful provider: prove the calls are arriving before
/// debugging an exporter.
struct CountingMetrics;

impl Metrics for CountingMetrics {
    fn on_request(&self, method: &str) {
        eprintln!("request: {method}");
    }
    fn on_error(&self, method: &str, error_kind: &str) {
        eprintln!("error: {method} {error_kind}");
    }
    fn on_latency(&self, method: &str, duration: Duration) {
        eprintln!("latency: {method} {duration:?}");
    }
}
```

Hand it to the builder with `.with_metrics(CountingMetrics)`. If those lines
appear and your dashboard is still empty, the problem is the exporter, not the
SDK — which is the distinction this two-minute step buys you.

With `otel`, `OtelMetrics` records the catalogue below, and with `otlp`
`Telemetry` exports it over OTLP. `otel` is a default feature, and with it a
handler given no `with_metrics` records through `OtelMetrics` instruments on
the global meter provider as it stands at each measurement, built again when
the process installs another: install `Telemetry` before or after building the
handler and nothing else is needed. The client's `rpc.client.call.duration`
binds the same way. Before 0.15 such a handler recorded nothing until given
`with_metrics`.

`with_metrics` *replaces* that default, so a handler given a counter of its
own stops exporting `rpc.server.call.duration` and the `a2a.server.*`
instruments. To count something of your own beside the export, use
`.with_additional_metrics(my_metrics)`: each callback goes to the default (or
to whatever `with_metrics` set), then to yours.

## Exporting with `Telemetry`

`Telemetry` (ADR 0013, option 5) builds one OTLP exporter per signal, on the
transport the environment names, installs the tracer and meter providers and
the W3C `tracecontext` and `baggage` propagators as the process-global ones,
and returns a guard whose drop — or explicit `shutdown` — flushes all three.
`telemetry.layer()` is the `tracing` layer that turns spans into OpenTelemetry
spans and events into OpenTelemetry log records; add it next to whatever other
layers you run, or call `telemetry.install_subscriber()` to install it as the
process's only subscriber (it never replaces one already set).

A binary that must run as well without a collector as with one builds it with
`with_export_where_configured()`: each signal is then built only when an
endpoint names where it goes — `OTEL_EXPORTER_OTLP_ENDPOINT`, the signal's own
`OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` / `_METRICS_` / `_LOGS_ENDPOINT`, or
`with_otlp_endpoint` in code — and a signal with none has no exporter and no
export thread. Every binary in this repository does this:

```rust,no_run
use a2a_protocol_server::otel::Telemetry;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent")
    .with_export_where_configured()
    .build()?;
telemetry.install_subscriber();
# Ok(())
# }
```

Each process also reports a `service.instance.id`, a random UUID unless
`OTEL_RESOURCE_ATTRIBUTES` names one: it is what tells replicas of one
service apart (Prometheus turns it into the `instance` label).

It reads the environment as the OpenTelemetry specification defines it, and
refuses a value it cannot act on rather than ignoring it:

| Variable | Effect |
|---|---|
| `OTEL_SDK_DISABLED=true` | Builds nothing; the layer records nothing |
| `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER` | `otlp` or `none` per signal; any other value (`console`, `prometheus`, …) is an error naming it |
| `OTEL_EXPORTER_OTLP_PROTOCOL` (and `_TRACES_` / `_METRICS_` / `_LOGS_`) | `grpc`, `http/protobuf` (the default) or, with the `otlp-json` feature, `http/json`; without it `http/json` is an error naming the feature |
| `OTEL_EXPORTER_OTLP_ENDPOINT` (and per signal) | The collector; OTLP/HTTP appends `/v1/traces`, `/v1/metrics` or `/v1/logs` to the general one |
| `OTEL_EXPORTER_OTLP_HEADERS` (and per signal) | Sent with every export |
| `OTEL_EXPORTER_OTLP_TIMEOUT` (and per signal) | Milliseconds; 10 s when unset |
| `OTEL_EXPORTER_OTLP_CERTIFICATE` (and per signal) | A PEM file of CA certificates trusted alongside the bundled Mozilla roots |
| `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE`, `OTEL_EXPORTER_OTLP_CLIENT_KEY` (and per signal) | PEM files of the client certificate chain and its key, for a collector that requires mutual TLS; one without the other is an error |
| `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES` | The resource; either naming the service wins over `with_default_service_name` |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG`, `OTEL_BSP_*` | Read by the OpenTelemetry SDK itself |

The builder's `with_otlp_endpoint`, `with_otlp_protocol`, `with_otlp_header`
and `with_ca_certificate_pem` set the same things in code, and — as in the
OpenTelemetry SDK — a value set in code wins over the variable.
`with_client_certificate_pem` is the exception: client certificate files the
environment names win over it, as a header variable wins over a header set in
code. The certificate and key are parsed when the exporters are built, so a
file that is not PEM, or (on OTLP/HTTP) a key that is not the certificate's,
fails `build` rather than every export.

JSON bodies (`http/json`) are the `otlp-json` feature, off by default even in
`a2a-protocol-sdk`: `opentelemetry-otlp`'s `http-json` feature also makes JSON
the protocol of every exporter in the process that is built without one, and
Cargo's feature unification would impose that on an application's own
exporters.

Things it gets right that hand-wiring tends not to, each covered by a test in
`crates/a2a-protocol-server/tests/telemetry_export/`:

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
  `opentelemetry*`, `hyper`, `h2`, `tonic`, `tower` and `rustls` are filtered
  out of the layer, so an export cannot produce telemetry that needs
  exporting. They still reach your other layers, so a `fmt` layer still shows
  an export error.
* **A lost export is reported, on either transport.** `opentelemetry-otlp`
  retries a failed export (four attempts by default, with backoff), and when
  it gives up the SDK's batch processor logs the failure and drops the batch;
  its `shutdown` returns `Ok` regardless (`opentelemetry_sdk` 0.33.1).
  `Telemetry` counts each export its exporter gave up on, and `shutdown` and
  `force_flush` return `TelemetryError::Export` naming the signal and the
  count. An export refused once and delivered on retry is not counted.
* **Log records are `INFO` and above** by default (`with_log_level` changes
  it); spans are not filtered by level here.

### The older `init_otlp_pipeline`

`init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` predate
`Telemetry` and stay for existing callers. They export **metrics only**, over
**gRPC only** (`OTEL_EXPORTER_OTLP_PROTOCOL` has no effect on them), must be
called from inside a Tokio runtime — outside one they panic with `there is no
reactor running`, which release builds' `panic = "abort"` turns into a process
abort — and their `service_name` argument, an `Option<&str>` since 0.15, names
the service only when `OTEL_SERVICE_NAME` (or `service.name` in
`OTEL_RESOURCE_ATTRIBUTES`) does not, as the environment specification
prescribes; before 0.15 it overwrote them. Calling one
twice replaces the first provider and silently orphans it. New code should use
`Telemetry`.

If `CountingMetrics` above prints and your collector is still empty, the
problem is in the exporter configuration, not in the SDK.

## The catalogue

Every instrument the server emits, with its unit. These names are stable;
treat them as the contract.

| Metric | Type | Unit | Meaning |
|---|---|---|---|
| `rpc.server.call.duration` | histogram | s | Every inbound call, on every binding, by method and outcome |
| `a2a.server.requests` | counter | `{request}` | Inbound A2A requests |
| `a2a.server.responses` | counter | `{response}` | Outbound A2A responses |
| `a2a.server.errors` | counter | `{error}` | Request errors |
| `a2a.server.request.duration` | histogram | s | Request duration (named `a2a.server.latency` before 0.15) |
| `a2a.server.queue_depth` | gauge | `{queue}` | Active event queues |
| `a2a.server.persistence_errors` | counter | `{error}` | Store writes that failed |
| `a2a.server.push_deliveries` | counter | `{delivery}` | Push attempts, by `outcome` |
| `a2a.server.pool.active` | gauge | `{connection}` | In-use HTTP connections |
| `a2a.server.pool.idle` | gauge | `{connection}` | Idle HTTP connections |
| `a2a.server.pool.created` | counter | `{connection}` | Connections created since start |
| `a2a.server.pool.closed` | counter | `{connection}` | Connections closed on error or timeout |
| `db.client.operation.duration` | histogram | s | Each operation of a task store that names its database, by `db.operation.name` and outcome |

`rpc.server.call.duration` is the OpenTelemetry semantic conventions'
instrument, with their advisory buckets (0.1 ms to 100 s; 5 ms to 10 s
before 0.15) and attributes:
`rpc.system.name` (`jsonrpc` — over HTTP or WebSocket — `a2a_http_json`,
`grpc`, or `slimrpc` for `a2a-protocol-slimrpc`); `rpc.method`, the fully qualified method
(`lf.a2a.v1.A2AService/SendMessage`) or `_OTHER` for one the server does not
serve; `rpc.status_code`; and, only when the call failed, `error.type`. Both
are the status the binding answered with — the JSON-RPC error code (`-32001`),
the HTTP status (`404`), or the gRPC status name (`NOT_FOUND`; gRPC and
SLIMRPC, which uses gRPC's status codes, also report `OK`) — or `cancelled` for a call whose peer went away first.

**What counts as failed** is each binding's semantic conventions' rule, so
`error.type` marks a server failure rather than every answer that was not a
success: every JSON-RPC error code ("All JSON RPC error codes SHOULD be
considered errors"); on HTTP+JSON a 5xx but not a 4xx (the HTTP conventions
leave a server span's status unset for a 4xx); on gRPC only `UNKNOWN`,
`DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE` and
`DATA_LOSS`. A `GetTask` for an unknown task is `rpc.status_code` `NOT_FOUND`
(or `404`) with no `error.type`. Before 0.15 every non-success status set it;
a dashboard counting failures from `error.type` on HTTP+JSON or gRPC now sees
only server failures, and one that wants the 4xx class reads
`rpc.status_code`. A binding outside this crate opts into gRPC's rule with
`CustomBinding::with_grpc_server_errors`.

It
records calls the handler never sees, with the status they were answered:
on JSON-RPC, a body that is not a request (no `rpc.method`), an unknown
method, a batch over the limit; on HTTP+JSON and the axum router, a request
refused before routing — an unsupported or missing `A2A-Version`, a content
type, a path no route serves (no `rpc.method`) — and one refused on its body
or parameters, under the route's method; on gRPC, a call refused for its
metadata or a request that does not convert, under its method. A streaming
call — `SendStreamingMessage` or `SubscribeToTask`, over SSE, gRPC or
WebSocket — lasts until its stream ends, as the conventions ask ("the span
SHOULD cover the full lifetime of the request and/or response streams"): it
is recorded then, with the error the stream's last frame carried, or
`cancelled` if the client went away first. Before 0.15 it was timed only up
to the stream being established, and recorded as a success whatever the
stream did next. The same values reach a custom `Metrics` as `on_rpc_call`.

**The client measures its calls too:** `rpc.client.call.duration`, the
conventions' client histogram (unit `s`, the same buckets), with
`rpc.system.name`, `rpc.method`, `rpc.status_code` as the server records it
(the JSON-RPC code, the HTTP status, or the gRPC status name, `OK` on a gRPC
success), `error.type` for a failed call (that status, or the failure's
class — `timeout`, `transport` — when the agent never answered), and
`server.address` and `server.port`. A streaming call is measured until the
caller drops its stream, as its span is. It records through the global
meter provider — the one `Telemetry` installs — as it stood when the client
was built, under the meter `a2a.client`; a call on a custom transport that
names no `rpc.system.name` is not recorded, since that attribute is
required.

`a2a.server.request.duration` times the same calls by A2A method, with the
same buckets. Before 0.15 it was `a2a.server.latency`, deprecated in 0.14.0
and removed in 0.15.0 per `STABILITY.md` §3; OpenTelemetry names a duration
histogram `.duration`, so a dashboard built for the conventions finds it.

The `pool` instruments are reported by the servers this crate runs — `serve`,
`serve_with_addr` and `Server::serve_with_shutdown`. A connection is *active*
while a request on it is in flight, including a response still streaming,
and *idle* while it is open with none; `closed` counts connections that ended
in an error or timeout. The gRPC and WebSocket dispatchers' own listeners,
and a router you serve yourself, do not report them.

`requests`, `responses` and `errors` are separate on purpose. A request ends in
exactly one of the other two — a call answered with an error counts in
`errors`, not in `responses` — so `requests − responses − errors` is the
requests that produced no answer at all: a panicked executor, a dropped
connection, a process that went away mid-request. A single counter would hide
exactly the failure you want to see. (Until 2026-10-23 this paragraph called
`requests − responses` that gap, which counts every error as unanswered;
measured against a live Prometheus, the recording rule in
`deploy/observability/` subtracts both.)

### The tool loop's metrics

A `ToolLoop` (`server::tool_loop`, feature `tool-loop`, with `otel`)
records the OpenTelemetry `GenAI` conventions' metrics
(`open-telemetry/semantic-conventions-genai` at `9ffc7e0`, all
`development`), with their advisory buckets:

| Instrument | Kind | Unit | Recorded |
|---|---|---|---|
| `gen_ai.client.inference.duration` | histogram | s | Each model turn |
| `gen_ai.client.inference.usage.input_tokens` | counter | `{token}` | Each turn whose provider reported a non-zero count |
| `gen_ai.client.inference.usage.output_tokens` | counter | `{token}` | Likewise |
| `gen_ai.client.inference.operation.input_tokens` | histogram | `{token}` | Each turn whose provider reported a count |
| `gen_ai.client.inference.operation.output_tokens` | histogram | `{token}` | Likewise |
| `gen_ai.execute_tool.duration` | histogram | s | Each tool call that ran |
| `gen_ai.invoke_agent.duration` | histogram | s | Each run of the loop |
| `gen_ai.invoke_agent.inference_calls` | histogram | `{inference_call}` | Each run: how many turns it asked for |
| `gen_ai.invoke_agent.tool_calls` | histogram | `{tool_call}` | Each run: how many calls it ran |

A turn's instruments carry `gen_ai.operation.name` (`chat`),
`gen_ai.provider.name`, `gen_ai.request.model`, and when known
`gen_ai.response.model`, `server.address` and `server.port`; the usage
counters add `gen_ai.token.modality` (`text`). A call's carries
`gen_ai.tool.name` and `gen_ai.tool.type`; a run's, `gen_ai.request.model`
and the name `ToolLoop::with_agent_name` gives it. `error.type` is the
provider's failure for a turn (`ModelError::error_type`: the HTTP status,
`timeout`, …), `unknown_tool`, `invalid_arguments`, `tool_error` or
`tool_fatal` for a call, `turn_limit` or `budget_exhausted` for a run, and
`cancelled` for any of them abandoned before it ended — its cancellation
token fired, or the future running it was dropped, as an executor timeout
does. `gen_ai.client.operation.duration` is not recorded: the conventions
say it "SHOULD NOT be reported for inference operations".

The metrics go to the global meter provider as it is when each is
recorded, so `Telemetry` installed after the loop is built still receives
them; `ToolLoop::with_meter` sends them elsewhere.

### Agent-card requests

A request for the card at `/.well-known/agent-card.json`, on any listener
that serves it (JSON-RPC, HTTP+JSON, the axum router, WebSocket, gRPC), is an
HTTP `SERVER` span named `GET /.well-known/agent-card.json` (or `HTTP` and
the route for a method the conventions do not know) and a point in the HTTP
conventions' `http.server.request.duration` (histogram, `s`, their advisory
buckets), under the `a2a.server` scope. The span carries
`http.request.method`, `url.path`, `url.scheme`, `http.route`,
`http.response.status_code`, `network.protocol.version`, `network.peer.*`
when the listener knows the connection, `server.address` and `server.port`
from `Host`, and `user_agent.original`; a query is recorded only as
`url.query="REDACTED"`. `url.scheme` is `https` when this server's TLS
ended the connection. A 5xx sets `error.type` and the error status; a 4xx,
as the conventions require of a server span, does not. The span follows
`InboundTracePolicy` as a call's does. The metric carries only the method,
scheme, route, status, protocol version and `error.type`, none of which a
caller can choose freely.

### Webhook deliveries

With `otel`, each push-notification POST, every retry included, is
measured in the HTTP conventions' `http.client.request.duration`
(histogram, `s`, their advisory buckets of 5 ms to 10 s), under the
`a2a.server` scope, on the global meter provider as it is at the
measurement. Its attributes are those of the attempt's `POST` span:
`http.request.method` (`POST`), `server.address`, `server.port`,
`http.response.status_code` when a response came back, and `error.type`
when the attempt failed: the status for a 4xx or 5xx, `timeout`, or
`transport` for no response. The URL is not an attribute, because it would
carry the receiver's secret and unbounded values. `a2a.server.push_deliveries`
still counts each delivery's final outcome; this metric times each attempt.

### Task-store operations

A task store that names its database through `TaskStore::database` gets a
`CLIENT` span around each of its operations and a point in the database
conventions' `db.client.operation.duration` (histogram, `s`, their advisory
buckets of 1 ms to 10 s; `otel` feature), under the `a2a.server` scope
(`docs/db/database-spans.md` and `database-metrics.md`,
`open-telemetry/semantic-conventions@55c98cf`). The SQLite and PostgreSQL
stores, their tenant-aware forms, and the encrypted and audited wrappers
around them name theirs; the in-memory stores run against no database and
record neither. A custom store opts in by returning a `StoreDatabase`.

One span is one `TaskStore` operation as the handler called it, whatever
queries the store ran for it, as the conventions recommend for a
higher-level API:

| Attribute | Value |
|---|---|
| span name | `{db.operation.name} {db.namespace}`, or `{db.operation.name} {server.address}[:{server.port}]` without a namespace, or the operation alone |
| `db.system.name` | `postgresql`, `sqlite`, or the custom store's |
| `db.operation.name` | The trait method: `get`, `save_status_delta`, `append_event`, … |
| `db.namespace` | PostgreSQL: the database name |
| `server.address`, `server.port` | PostgreSQL: the host or socket path in the pool's options; the port only when it is not 5432 |
| `error.type` | On failure: `unavailable` for a temporary one (a pool with no free connection, a locked SQLite file, a PostgreSQL connection error), otherwise the A2A error code the store answered (`-32603` internal); the span's status is set to error |

`error.type` is not the database's SQLSTATE or result code, which the
conventions prefer: the stores turn a database error into an `A2aError`
before it reaches the trait, and whether it was temporary, and its A2A
code, are what survive. The metric carries the same attributes. In Prometheus, fed over
OTLP, it is `db_client_operation_duration_seconds`, with labels
`db_system_name`, `db_operation_name` and, on failures, `error_type`
(measured 2026-10-24 against Prometheus 3.15.1 with a SQLite store).

### What these become in Prometheus

The dots are correct: OpenTelemetry names metrics this way
(`http.server.request.duration`), and the exporter translates `.` to `_`.
But you should not have to guess the translated name, so here it is,
measured rather than derived — rendered through `opentelemetry-prometheus`
0.33.1, the version matching this crate's `opentelemetry_sdk` (re-measured
2026-10-18 after the 0.15 rename and the move to UCUM units: the duration
histogram's name follows the rename, and every other name is unchanged):

| Instrument | Prometheus name |
|---|---|
| `rpc.server.call.duration` | `rpc_server_call_duration_seconds` |
| `a2a.server.requests` | `a2a_server_requests_total` |
| `a2a.server.responses` | `a2a_server_responses_total` |
| `a2a.server.errors` | `a2a_server_errors_total` |
| `a2a.server.request.duration` | `a2a_server_request_duration_seconds` |
| `a2a.server.queue_depth` | `a2a_server_queue_depth` |
| `a2a.server.persistence_errors` | `a2a_server_persistence_errors_total` |
| `a2a.server.push_deliveries` | `a2a_server_push_deliveries_total` |
| `a2a.server.pool.active` | `a2a_server_pool_active` |
| `a2a.server.pool.idle` | `a2a_server_pool_idle` |
| `a2a.server.pool.created` | `a2a_server_pool_created_total` |
| `a2a.server.pool.closed` | `a2a_server_pool_closed_total` |

Counters gain `_total`, the histograms gain `_seconds` from their `s` unit,
and gauges gain nothing. Every series also carries
`otel_scope_name="a2a.server"` — the default meter name, changeable with
`OtelMetricsBuilder::meter_name`. Alongside them the exporter emits
`target_info`, a gauge whose `service_name` label is the service name:
`OTEL_SERVICE_NAME` when set, otherwise the name passed to
`init_otlp_pipeline`.

**Units are UCUM since 0.15.** A dimensionless count is brace-annotated
(`{request}`, `{connection}`), as the specification asks; before 0.15 they
were bare words (`request`). Rendering the catalogue both ways through this
exporter produces the same metric names, because it suffixes only units it
can translate and translates neither form; the change corrects the metadata
other exporters and backends read.

## The dashboards

[`deploy/observability/`](https://github.com/tomtom215/a2a-rust/tree/main/deploy/observability)
ships three Grafana dashboards, provisioned with Prometheus recording and
alerting rules for the metrics on this page. Its README says how to point an
agent at them. Each panel carries a description of what it shows; hover the
ⓘ beside its title. The screenshots below are the dashboards as they ship,
over 15 minutes of traffic from this repository's test harness: three
tool-loop agents against a local model (one whose model answers, one whose
model is unreachable, one whose model calls time out), and the `a2a` CLI calling the TCK's agent on every binding with
push notifications to a working webhook and an unreachable one.

**A2A server.** Calls per second and failed calls by binding and method;
p50, p95 and p99 of non-streaming calls; errors by kind; requests without a
response; active event queues; persistence errors; push deliveries by
outcome; HTTP connections; and task-store operations and their p95 duration.
Persistence errors are empty here because no store write failed.

![The A2A server dashboard](../images/dashboards/a2a-server.png)

**A2A client.** Calls per second, failed calls and failures by kind, by
agent and binding, and the p95 of non-streaming calls, from
`rpc.client.call.duration`.

![The A2A client dashboard](../images/dashboards/a2a-client.png)

**GenAI.** From the tool loop's `gen_ai.*` metrics: model calls by model
and outcome, the failed share, and the duration of successful calls; tokens
per second and per call; tool calls by tool and outcome, and their
duration; agent runs by outcome, their duration, and the model and tool
calls each run took.

![The GenAI dashboard](../images/dashboards/a2a-genai.png)

## The four signals worth alerting on

The rules in [`deploy/observability/`](https://github.com/tomtom215/a2a-rust/tree/main/deploy/observability)
alert on these, and the [dashboards](#the-dashboards) above chart them.

**`errors` / `requests`.** The obvious one. Split by `method`: a rising error
rate confined to `SendStreamingMessage` is a different incident from one across
everything.

**`persistence_errors` above zero, at all.** A store write that fails means the
task's recorded state and its actual state have diverged. Streams recover from
loss by refetching; this is the case where refetching returns the wrong answer.
Alert on any non-zero rate rather than on a threshold.

**`queue_depth` that does not come down.** Each active stream holds a queue.
A depth that grows monotonically is subscribers that never closed — usually a
client that stopped reading without disconnecting.

**`push_deliveries{outcome="timeout_truncated"}`.** Not a network failure. It
means `push_delivery_timeout` was shorter than the sender's own retry schedule,
so the delivery was abandoned with attempts remaining. It will not resolve on
its own and it will not appear as a webhook error, because the webhook was
never the problem. `outcome="skipped"` is its sibling: the 30-second per-event
budget ran out before this config was reached, so nothing was sent to it. The
five labels:

```text
delivered          the webhook accepted it
failed             reached, and refused it — or the sender itself errored
timeout            the webhook did not answer inside the time it was given
timeout_truncated  a configuration result: push_delivery_timeout cut the schedule short
skipped            the per-event budget ran out before this config was tried
```

## What is not measured

Stated so a green dashboard is not mistaken for a complete one:

* **The executor has a span, not a metric.** Its time is the
  `invoke_agent` span's duration; no histogram records it.
* **A store's individual SQL statements have no span.** Each task-store
  operation is one span and one `db.client.operation.duration` point (see
  [Task-store operations](#task-store-operations)); the statements inside
  it are not. sqlx 0.9 reports each as a `tracing` *event*, not a span
  (target `sqlx::query`, with the SQL as `db.statement` — placeholders, not
  the bound values — `rows_affected` and `elapsed_secs`), at `DEBUG`, or at
  `WARN` when it takes a second or more. Under `Telemetry` those events are
  log records in the trace of the operation that ran them; the slow ones at
  the default `INFO` log level, every one with
  `TelemetryBuilder::with_log_level(Level::DEBUG)`. A custom store that does
  not override `TaskStore::database` has neither span nor duration.
* **There is no per-tenant metric dimension.** A tenant hitting
  `max_concurrent_tasks` shows as `Overloaded` errors in the aggregate; see
  [Multi-Tenancy](./multi-tenancy.md) for what the limits actually are.
  (The call's span does carry `a2a.tenant`.)
* **Model calls are seen only through the tool loop.** An executor built on
  `server::tool_loop` (feature `tool-loop`) records a `chat` span per model
  call and an `execute_tool` span per tool call, under `invoke_agent`, and
  the `gen_ai.*` metrics above. Any other model client is invisible to the
  SDK: instrument it, or use a framework that reports `chat` spans.

## Logs

With `tracing` enabled and a subscriber installed:

```bash
RUST_LOG=a2a_protocol_server=debug,a2a_protocol_client=debug cargo run
```

Task and context identifiers are on the executor's span (`a2a.task.id`,
`a2a.context.id`), so one request can be followed through **this** process.
If you emit your own events from inside an executor, they inherit that
context.

## Spans

With the `tracing` feature — on by default — every inbound call, on every
binding, runs in one span of kind `SERVER`, named for the method as the gRPC
service spells it: `lf.a2a.v1.A2AService/SendMessage`. It carries the
semantic conventions' `rpc.system.name`, `rpc.method`, `rpc.status_code`
when the binding sent one, and, when the call failed by its binding's rule
above, `error.type` with the span's status set to error — the same values as
`rpc.server.call.duration`. A method the
server does not serve is named for its binding (`jsonrpc`), with
`rpc.method` `_OTHER` and the name the peer sent, cut to 128 bytes, as
`rpc.method_original`; the v0.3 `message/stream` is one of them. A JSON-RPC
span, over HTTP or WebSocket, also carries `jsonrpc.protocol.version`
(`2.0`), the method as sent (`SendMessage`) as `rpc.method_original`, and the
request's id as `jsonrpc.request.id` (not for a notification or a `null` id),
as `docs/rpc/json-rpc.md` asks.

Its addresses follow `docs/rpc/rpc-spans.md`. `network.peer.address` and
`network.peer.port` are the connection the call arrived on, when the
binding knows it (the address `CallContext::peer_addr` returns).
`server.address` and `server.port` are the address the client used: the
request's `Host`, with the port only when `Host` names one. They are left
unset on gRPC, whose metadata does not carry `:authority`, and for a `Host`
that is not a valid authority; the conventions say not to fill them from the
connection. None of the four is on `rpc.server.call.duration`: `Host` is the
caller's to choose, and a metric attribute it chose is unbounded.

It also carries the attributes of the **draft OpenTelemetry A2A
conventions** — `open-telemetry/semantic-conventions-genai` pull request
#195, read at `842a839`; open, and every attribute `development`, so these
names may still change:

| Attribute | Value |
|---|---|
| `a2a.method.name` | `SendMessage`, `GetTask`, … |
| `a2a.protocol.version` | `1.0` |
| `a2a.tenant` | The request's `tenant`, when set |
| `a2a.message.id` | The request's message, when it carries one |
| `a2a.message.reference_task_ids` | Its `referenceTaskIds` (string array; `otel` feature) |
| `a2a.task.id` | The task the call ran or named |
| `a2a.task.state` | `TASK_STATE_COMPLETED` …, when the response carries a task |
| `gen_ai.conversation.id` | The A2A `contextId` |
| `gen_ai.agent.name`, `.description`, `.version` | From the handler's agent card |

The draft asks HTTP server spans to be renamed `{a2a.method.name}`; this SDK
keeps one name per call on every binding (ADR 0013), as its gRPC rule does.

**A binding outside `a2a-protocol-server`** records the same span and metric
through `BindingSpan`: it names itself with a `CustomBinding` — its own
`rpc.system.name` and the function that turns a `ServerError` into the status
it sends — and runs each `RequestHandler` call inside
`BindingSpan::open(..).run(..)`. `a2a-protocol-slimrpc` does this for all
eleven methods, as `slimrpc`. Its `tests/telemetry.rs` checks the span and
the metric over a real in-process SLIM fabric.

Work spawned for the call runs in `INTERNAL` children of that span:
`a2a.process_events`, `a2a.deliver_push`, `a2a.sse`, and the executor's run.
**The executor's span follows the OpenTelemetry GenAI agent conventions**
(`docs/gen-ai/gen-ai-agent-spans.md`, also `development`): it is named
`invoke_agent {card name}` and carries `gen_ai.operation.name =
invoke_agent`, the agent's name, description and version, the context as
`gen_ai.conversation.id`, the task's final `a2a.task.state`, and — kept from
before — `a2a.task.id` and `a2a.context.id`. That is what an agent-aware
backend keys on: Langfuse shows the run as an agent and groups a context's
runs into one session. If an agent framework inside your executor reports
its own `invoke_agent` span, turn this one off with
`RequestHandlerBuilder::with_agent_span_conventions(false)`; the span is then
`a2a.execute`, as in 0.14. A run that ends `Failed` — the executor's own
status, or the one written for an executor that returned `Err` or ran past
its timeout — sets the span's status to error and its `error.type` to the
failure class (`internal`, `invalid-request`, `transient`, `policy-refusal`,
or `budget-exhausted`, which a timeout is).

**Each push-notification POST is an HTTP `CLIENT` span**, named `POST`, a
child of `a2a.deliver_push` or `a2a.process_events`, with
`http.request.method`, `server.address`, `server.port`,
`http.response.status_code`, `http.request.resend_count` on a retry, and
`error.type` — the status, `timeout` or `transport` — for a failed attempt.
Its `url.full` is the webhook's origin and `/REDACTED`, since a webhook's
path or query is often its secret. With the `otel` feature the POST carries
that span as its `traceparent` (and `tracestate`), so a receiver that
continues the trace is a child of a span your backend has; when no span is
recorded — no OpenTelemetry layer, or `InboundTracePolicy::Drop` — none is
sent.

**Message content is opt-in.** With
`RequestHandlerBuilder::with_span_content_capture(true)`, the executor's span
records the request's message as `gen_ai.input.messages` and the agent's
replies and artifacts as `gen_ai.output.messages`, in the conventions' JSON
message format. That copies what users and agents say into your traces and
wherever they are exported, which is why it is off. Each attribute holds at
most 64 KiB of text and data, cut with a marker beyond that; raw file bytes
are never recorded, only their media type and length. The finish reason is
not content: a completed turn records `gen_ai.response.finish_reasons`
`["stop"]` and a failed one `["error"]` either way, as a string array (it was
a `finish_reason` inside the output message before 0.15, which the
conventions deprecate).

**Each call through the client is a `CLIENT` span** with the same name as the
server span it causes (for a method outside the A2A service, `_OTHER`, it is
named for its `rpc.system.name` and carries `rpc.method_original`), the same A2A attributes as far as the request and
response show them, the agent's name, description and version when the
client was built from its card, and `server.address` and `server.port`. A
JSON-RPC call's span, over HTTP or WebSocket, carries what the server's does:
`jsonrpc.protocol.version` (`2.0`), the method as sent as
`rpc.method_original`, and the request's id as `jsonrpc.request.id` — after
a retry, the id of the request the response answered. A
streaming call's span covers consuming the stream, and is exported when the
stream is dropped; it carries `rpc.status_code` as the client's metric does,
and the error a stream ended on. As the draft conventions require of A2A instrumentation,
the client reports no `invoke_agent` span of its own.

A `Delegation`'s lease renewals and the cancel it sends when dropped run
on spawned tasks, in the span and ambient trace current when the delegation
started, so their calls are in the delegating task's trace.

**Fetching a card is a `GET` span**, the HTTP conventions' `CLIENT` span
for a plain HTTP request: `http.request.method`, `url.full` (scheme, host,
port and path; userinfo and the query are dropped), `server.address`,
`server.port`, `http.response.status_code` (`304` for a revalidated card),
and on failure `error.type` — the status, or `invalid_response` for a card
a2a.proto's field rules refuse. **Checking a push notification**
(`WebhookReceiver::receive`) is an `INTERNAL` span, `a2a.webhook.receive`,
under whatever span the integrator's HTTP server recorded; when none is
recorded it continues the `traceparent` the agent's push sender sent, so a
notification is found under the task that caused it. It records
`a2a.webhook.outcome` (`accepted`, `duplicate`, `rejected`), the task once
the request authenticated, and for a rejection its kind as `error.type`
(`unknown_token`, `bad_authorization`, …) — never the token, the
credentials or the body.

A span is exported when it closes, and `tracing` holds a span open until its
children close, so a call's span reaches your backend once the work it
spawned has finished — with its own end time, the call's, not theirs; a
streaming call's is its stream's. Every
attribute is recorded once: `tracing-opentelemetry` 0.34.0 exports a field
recorded twice as two attributes with one key, and the SDK's observability
gate fails on any span that carries one.

Recording is not free. On a loopback JSON-RPC round trip with a trivial
executor, a `tracing-opentelemetry` layer recording every span cost about
65 µs a call more than before these spans and attributes (323 → 388 µs, the
median of three rounds; ADR 0013 has the conditions). With no layer
installed the difference was inside the run-to-run noise.

With the `otel` feature and a `tracing-opentelemetry` layer installed, a
well-formed inbound `traceparent` is the `SERVER` span's remote parent, and
the `traceparent` the executor sends onward names that recorded span — so a
downstream agent's trace points at a parent your backend has. The inbound
trace policy decides first: `Restart` makes the span a new root, and `Drop`
records no span at all (the call's metric is still recorded). Without the
`otel` feature the spans are ordinary `tracing` spans, and the id sent
onward is minted for the hop, as before.

**Logs alone still do not join a delegation chain.** An early revision of
this page said they did, which was wrong: two processes produce two span
trees, and correlating by task id does not rescue it because each agent mints
its own. What joins them is the trace id below.

## Trace context

Since 0.13 the SDK carries [W3C Trace
Context](https://www.w3.org/TR/trace-context/) across an A2A hop. It is
propagation: it works whether or not anything records spans. What it
guarantees is that every agent in a chain sees the same trace id, so whatever
does record spans can stitch them together — including across the Python,
JavaScript, Go and Java agents in the interoperability kit, since
`traceparent` is a wire format rather than a Rust type.

**Serving.** The server parses an inbound `traceparent`, advances the span,
and hands it to the executor:

```rust
# use a2a_protocol_client::{A2aClient, ClientResult, CurrentTrace};
# use a2a_protocol_server::RequestContext;
# use a2a_protocol_types::params::MessageSendParams;
# async fn delegate(
#     ctx: &RequestContext,
#     downstream: &A2aClient,
#     params: MessageSendParams,
# ) -> ClientResult<()> {
// `ctx.trace_context()` names *this* hop's span, so sending it verbatim
// makes the callee a child of this agent.
if let Some(trace) = ctx.trace_context().cloned() {
    CurrentTrace::scope(trace, async {
        downstream.send_message(params).await
    })
    .await?;
}
# Ok(())
# }
```

`None` means the caller sent no `traceparent`, or sent one the SDK refused.
It never means the SDK invented one: a malformed header is dropped rather
than repaired, because attaching work to a guessed-at trace is a wrong
answer where a missing trace is only an absent one.

**Calling.** With the `otel` feature and a `tracing-opentelemetry` layer
recording, nothing is needed: every call's `traceparent` names the call's own
`CLIENT` span, so the agent called becomes its child — from inside an
executor, under the executor's span; from anywhere else, under whatever span
is current. `ClientBuilder::with_trace_propagation(false)` turns that off for
a client calling agents that should not learn your trace ids. It applies to
the transports the client builds for JSON-RPC, HTTP+JSON and gRPC, not to one
supplied through `with_custom_transport`. Such a transport names the
`rpc.system.name` its calls' spans carry through `Transport::rpc_system`
(`jsonrpc` for `WebSocketTransport`, `slimrpc` for `SlimRpcTransport`); the
span keeps the agent from the card either way.

Without a recording layer, add `TracePropagationInterceptor` to the client and
it writes the ambient `CurrentTrace` onto every request over JSON-RPC, REST or
gRPC. An agent that is the first hop starts one with
`CurrentTrace::start_root()`. With both, a `CurrentTrace` scope is the client
span's parent when no recorded span is current.

Not over the `websocket` transport, which has no per-request header channel —
it carries headers only on the HTTP upgrade, and a `traceparent` fixed there
would report every request on the connection as one span, which W3C §3.4
forbids. The interceptor is silently ineffective on an established WebSocket
connection; the transport warns once per connection when it drops one. Send
traced calls over one of the other three. On the server, for the same reason,
a WebSocket call's span is not a child of the upgrade request's
`traceparent`: each call is a root span of its own, with a span link to the
upgrade's trace (under `InboundTracePolicy::Continue`). Before 0.15 every call
on a connection was recorded as a child of the operation that opened it.

**Trusting the caller's trace.** The inbound `traceparent` is read while the
`CallContext` is built, which is before your interceptors run — so on a public
endpoint the peer choosing the `trace-id` and the sampling bit has not been
authenticated yet. W3C §7.2 names what that allows: forged `trace-id`
collisions that make the data unusable, and an anonymous caller deciding what
your tracing vendor bills you for. A front gate sets
`RequestHandlerBuilder::with_inbound_trace_policy(InboundTracePolicy::Restart)`
to mint its own ids (§3.4's "Restart trace"), or `Drop` to refuse to trace an
unauthenticated request at all. The default is `Continue`, which joins the
caller's trace — right inside a trusted mesh, and the reason A2A delegation
chains are readable end to end.

**What is not done.** Nothing *interprets* `tracestate` — the SDK adds no
vendor entry of its own and reads nobody else's; it only truncates whole
entries when a list exceeds the documented 4096-character or 32-member cap
(W3C §3.3.1.5). There is no sampler. Each hop's `a2a.task.id` is on its own
`invoke_agent` span; nothing links one hop's task id to the next hop's.

See also [Troubleshooting](./troubleshooting.md) for the symptom-first version
of this page, and [Production Hardening](./production.md) for health checks.
