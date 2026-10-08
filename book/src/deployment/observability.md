<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Observability

Three signals, often confused: **logs** say what happened in one request,
**traces** say how one request travelled — through this agent and every agent
it called — and **metrics** say what is happening across all of them. This SDK
records all three. Logging and spans (`tracing`) are on by default and still
need a subscriber; exporting them over OTLP (`otel`) is **off by default**.

That default is deliberate. A protocol library that pulled in an OpenTelemetry
exporter to serve one agent would be the wrong trade for most users. The cost
is that "I see no metrics" is the single most common report against this crate,
and the answer is almost always this page's first section.

## Turning them on

```toml
a2a-protocol-sdk = { version = "0.14", features = ["otel"] }
```

* **`tracing`** — on by default in all three crates; with
  `default-features = false` the logging calls and spans compile to nothing.
  With it, your binary still has to install a subscriber; the library emits
  events and spans and does not decide where they go.
* **`otel`** — makes [`Telemetry`](#exporting-with-telemetry) available: OTLP
  export of traces, metrics and logs, configured by the standard `OTEL_*`
  environment. On the SDK it also turns on the client's half: each call's
  `traceparent` names the call's own span (see [Spans](#spans)).

With the `otel` feature, the whole setup is this, and the rest of the page
explains it:

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

// Hand the handler builder `.with_metrics(telemetry.otel_metrics())`, serve,
// and on the way out:
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

With the `otel` feature, `OtelMetrics` is the real provider and exports the
catalogue below over OTLP.

## Exporting with `Telemetry`

`Telemetry` (ADR 0013, option 5) builds one OTLP exporter per signal, on the
transport the environment names, installs the tracer and meter providers and
the W3C `tracecontext` and `baggage` propagators as the process-global ones,
and returns a guard whose drop — or explicit `shutdown` — flushes all three.
`telemetry.layer()` is the `tracing` layer that turns spans into OpenTelemetry
spans and events into OpenTelemetry log records; add it next to whatever other
layers you run.

It reads the environment as the OpenTelemetry specification defines it, and
refuses a value it cannot act on rather than ignoring it:

| Variable | Effect |
|---|---|
| `OTEL_SDK_DISABLED=true` | Builds nothing; the layer records nothing |
| `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER` | `otlp` or `none` per signal; any other value (`console`, `prometheus`, …) is an error naming it |
| `OTEL_EXPORTER_OTLP_PROTOCOL` (and `_TRACES_` / `_METRICS_` / `_LOGS_`) | `grpc` or `http/protobuf`, the default; `http/json` is an error (not compiled in) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` (and per signal) | The collector; OTLP/HTTP appends `/v1/traces`, `/v1/metrics` or `/v1/logs` to the general one |
| `OTEL_EXPORTER_OTLP_HEADERS` (and per signal) | Sent with every export |
| `OTEL_EXPORTER_OTLP_TIMEOUT` (and per signal) | Milliseconds; 10 s when unset |
| `OTEL_EXPORTER_OTLP_CERTIFICATE` (and per signal) | A PEM file of CA certificates trusted alongside the bundled Mozilla roots |
| `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES` | The resource; either naming the service wins over `with_default_service_name` |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG`, `OTEL_BSP_*` | Read by the OpenTelemetry SDK itself |

The builder's `with_otlp_endpoint`, `with_otlp_protocol`, `with_otlp_header`
and `with_ca_certificate_pem` set the same things in code, and — as in the
OpenTelemetry SDK — a value set in code wins over the variable. Client
certificates (mutual TLS to the collector) are not supported yet.

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
  endpoint `opentelemetry-otlp` 0.32 builds an empty `ClientTlsConfig`, which
  in tonic 0.14 enables no roots; `Telemetry` passes the Mozilla roots and
  any extra CA explicitly.
* **The exporter's own stack is never exported.** Spans and events from
  `opentelemetry*`, `hyper`, `h2`, `tonic`, `tower` and `rustls` are filtered
  out of the layer, so an export cannot produce telemetry that needs
  exporting. They still reach your other layers, so a `fmt` layer still shows
  an export error.
* **A lost export is reported.** The OpenTelemetry SDK's batch processors
  log a failed export and drop the batch, and their `shutdown` returns `Ok`
  regardless (`opentelemetry_sdk` 0.32.1). Over OTLP/HTTP, `Telemetry` counts
  exports that did not reach the collector or that it refused, and
  `shutdown` and `force_flush` return `TelemetryError::Export` naming the
  signal and the count. Over gRPC the export goes through tonic's client, and
  a failure is only logged.
* **Log records are `INFO` and above** by default (`with_log_level` changes
  it); spans are not filtered by level here.

### The older `init_otlp_pipeline`

`init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` predate
`Telemetry` and stay for existing callers. They export **metrics only**, over
**gRPC only** (`OTEL_EXPORTER_OTLP_PROTOCOL` has no effect on them), must be
called from inside a Tokio runtime — outside one they panic with `there is no
reactor running`, which release builds' `panic = "abort"` turns into a process
abort — and their `service_name` argument overwrites `OTEL_SERVICE_NAME`,
the opposite of what the environment specification prescribes. Calling one
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
| `a2a.server.requests` | counter | request | Inbound A2A requests |
| `a2a.server.responses` | counter | response | Outbound A2A responses |
| `a2a.server.errors` | counter | error | Request errors |
| `a2a.server.latency` | histogram | s | Request latency |
| `a2a.server.queue_depth` | gauge | queue | Active event queues |
| `a2a.server.persistence_errors` | counter | error | Store writes that failed |
| `a2a.server.push_deliveries` | counter | delivery | Push attempts, by `outcome` |
| `a2a.server.pool.active` | gauge | connection | In-use HTTP connections |
| `a2a.server.pool.idle` | gauge | connection | Idle HTTP connections |
| `a2a.server.pool.created` | counter | connection | Connections created since start |
| `a2a.server.pool.closed` | counter | connection | Connections closed on error or timeout |

`rpc.server.call.duration` is the OpenTelemetry semantic conventions'
instrument, with their advisory buckets (5 ms to 10 s) and attributes:
`rpc.system.name` (`jsonrpc` — over HTTP or WebSocket — `a2a_http_json`, or
`grpc`); `rpc.method`, the fully qualified method
(`lf.a2a.v1.A2AService/SendMessage`) or `_OTHER` for one the server does not
serve; `rpc.status_code`; and, only when the call failed, `error.type`. Both
are the status the binding answered with — the JSON-RPC error code (`-32001`),
the HTTP status (`404`), or the gRPC status name (`NOT_FOUND`; gRPC also
reports `OK`) — or `cancelled` for a call whose peer went away first. It
records calls the handler never sees: on JSON-RPC, a body that is not a
request (no `rpc.method`), an unknown method, a batch over the limit; on
HTTP+JSON, a request refused on its body or parameters, under the route's
method. For a streaming method it times the call up to the stream being
established, not the stream.
The same values reach a custom `Metrics` as `on_rpc_call`.

`a2a.server.latency` is deprecated in its favour and stays until at least the
next minor release, per `STABILITY.md` §3. It now uses the same buckets; it
used the SDK's defaults, sized for milliseconds, which put every call under
5 s in one bucket.

The `pool` instruments are reported by the servers this crate runs — `serve`,
`serve_with_addr` and `Server::serve_with_shutdown`. A connection is *active*
while a request on it is in flight, including a response still streaming,
and *idle* while it is open with none; `closed` counts connections that ended
in an error or timeout. The gRPC and WebSocket dispatchers' own listeners,
and a router you serve yourself, do not report them.

`requests` and `responses` are separate on purpose. They are not redundant: the
gap between them is requests that produced no response — a panicked executor, a
dropped connection, a process that went away mid-request. A single counter
would hide exactly the failure you want to see.

### What these become in Prometheus

The dots are correct: OpenTelemetry names metrics this way
(`http.server.request.duration`), and the exporter translates `.` to `_`.
But you should not have to guess the translated name, so here it is,
measured rather than derived — rendered through `opentelemetry-prometheus`
0.32.0, the version matching this crate's `opentelemetry_sdk`:

| Instrument | Prometheus name |
|---|---|
| `rpc.server.call.duration` | `rpc_server_call_duration_seconds` |
| `a2a.server.requests` | `a2a_server_requests_total` |
| `a2a.server.responses` | `a2a_server_responses_total` |
| `a2a.server.errors` | `a2a_server_errors_total` |
| `a2a.server.latency` | `a2a_server_latency_seconds` |
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
`target_info`, a gauge whose `service_name` label is the `service_name` you
passed to `init_otlp_pipeline`; that is where it lands, and the section
above is why the environment cannot set it.

**One known deviation, and a measurement that bounds it.** The Unit column
above is not UCUM: the specification brace-annotates dimensionless counts
(`{request}`, not `request`). Re-rendering the whole catalogue with UCUM
units produces a **byte-identical** exposition — same names, same metadata,
same SHA-256 — because this exporter only suffixes units it can translate,
and `{request}` and `request` both translate to nothing. So the deviation is
a metadata-correctness issue, not a cause of wrong metric names here; other
exporters may treat it differently.

The deviation that was visible — a duration histogram not named for the
conventions, so a dashboard template built for OpenTelemetry RPC metrics
found nothing — is answered by adding `rpc.server.call.duration`, the name
such a template looks for, rather than renaming `a2a.server.latency`: the
catalogue is published as a contract (see the line under *The catalogue*
above), so the old name is deprecated first and removed in a later release
with its own upgrade note. The UCUM units are left as they are, for the same
reason and because the measurement above shows they change nothing here.

## The four signals worth alerting on

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
* **Task-store operations have no latency instrument.** `persistence_errors`
  counts failures, not slowness — a store degrading toward a timeout shows up
  in request latency first, without saying it was the store.
* **There is no per-tenant metric dimension.** A tenant hitting
  `max_concurrent_tasks` shows as `Overloaded` errors in the aggregate; see
  [Multi-Tenancy](./multi-tenancy.md) for what the limits actually are.
  (The call's span does carry `a2a.tenant`.)
* **The client records spans, not metrics.** There is no
  `rpc.client.call.duration`; the client span's duration is the measure.
* **Model calls are not seen.** The SDK does not know which model your
  executor calls. Instrument the model client itself — its spans nest under
  `invoke_agent` — or use a framework that reports `chat` spans.

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
semantic conventions' `rpc.system.name`, `rpc.method` and, when the call
fails, `rpc.status_code` and `error.type` with the span's status set to
error — the same values as `rpc.server.call.duration` above. A method the
server does not serve is named for its binding (`jsonrpc`), with
`rpc.method` `_OTHER` and the name the peer sent, cut to 128 bytes, as
`rpc.method_original`.

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
`a2a.execute`, as in 0.14.

**Message content is opt-in.** With
`RequestHandlerBuilder::with_span_content_capture(true)`, the executor's span
records the request's message as `gen_ai.input.messages` and the agent's
replies and artifacts as `gen_ai.output.messages`, in the conventions' JSON
message format. That copies what users and agents say into your traces and
wherever they are exported, which is why it is off. Each attribute holds at
most 64 KiB of text and data, cut with a marker beyond that; raw file bytes
are never recorded, only their media type and length.

**Each call through the client is a `CLIENT` span** with the same name as the
server span it causes, the same A2A attributes as far as the request and
response show them, the agent's name, description and version when the
client was built from its card, and `server.address` and `server.port`. A
streaming call's span covers consuming the stream, and is exported when the
stream is dropped. As the draft conventions require of A2A instrumentation,
the client reports no `invoke_agent` span of its own.

A span is exported when it closes, and `tracing` holds a span open until its
children close, so a call's span reaches your backend once the work it
spawned has finished — with its own end time, the call's, not theirs. Every
attribute is recorded once: `tracing-opentelemetry` 0.33.0 exports a field
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
supplied through `with_custom_transport`.

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
traced calls over one of the other three.

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
