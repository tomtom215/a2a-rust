<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Observability

Two independent things, often confused: **logs** say what happened in one
request, **metrics** say what is happening across all of them. This SDK ships
both. Logging (`tracing`) is on by default and still needs a subscriber;
metrics export (`otel`) is **off by default**.

That default is deliberate. A protocol library that pulled in an OpenTelemetry
exporter to serve one agent would be the wrong trade for most users. The cost
is that "I see no metrics" is the single most common report against this crate,
and the answer is almost always this page's first section.

## Turning them on

```toml
a2a-protocol-server = { version = "0.14", features = ["tracing", "otel"] }
```

* **`tracing`** — on by default in all three crates; with
  `default-features = false` the logging calls compile to nothing. With it,
  your binary still has to install a subscriber; the library emits events
  and does not decide where they go.
* **`otel`** — makes `OtelMetrics` available, which exports over OTLP.

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

## Exporting over OTLP — the sequence, and where it bites

Installing `OtelMetrics` is half of it. Nothing leaves the process until an
export pipeline exists, and the order matters more than it looks.

```rust
use a2a_protocol_server::otel::{OtelMetricsBuilder, init_otlp_pipeline};
use a2a_protocol_server::builder::RequestHandlerBuilder;

/// The whole sequence. Called from inside the runtime — see below.
async fn install(executor: impl a2a_protocol_server::executor::AgentExecutor)
    -> Result<(), Box<dyn std::error::Error>>
{
    // 1. The pipeline first. It installs a process-global meter provider,
    //    which is what `OtelMetrics` records through.
    let provider = init_otlp_pipeline("my-agent")?;

    // 2. Then the metrics provider, on the handler.
    let handler = RequestHandlerBuilder::new(executor)
        .with_metrics(OtelMetricsBuilder::new().build())
        .build()?;
    let _ = handler;

    // 3. On shutdown, flush. Treat an error as "metrics may have been lost",
    //    not as a failure to terminate: the final flush fails whenever the
    //    collector is unreachable.
    let _ = provider.shutdown();
    Ok(())
}
# fn main() {}
```

Four things that are easy to get wrong, in the order people hit them.

**It must be called from inside a Tokio runtime.** `init_otlp_pipeline`
builds the tonic channel, and tonic spawns onto the ambient runtime while
doing so. Called from a plain `fn main` before the runtime starts, it panics
with `there is no reactor running` — and because release builds set
`panic = "abort"`, that is a **process abort, not an error you can handle**.
Call it inside `#[tokio::main]`, or within a `Runtime::enter` guard.

**The transport is gRPC and cannot be changed.** OTLP/gRPC on port 4317. The
HTTP/protobuf exporter is not compiled in, so **`OTEL_EXPORTER_OTLP_PROTOCOL`
has no effect**. Setting it to `http/protobuf` and pointing
`OTEL_EXPORTER_OTLP_ENDPOINT` at a collector's `:4318` gives you gRPC spoken
at an HTTP port, and silence. Of the standard variables, only these reach the
exporter:

| Variable | Effect |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | ✅ collector address; defaults to `http://localhost:4317` |
| `OTEL_EXPORTER_OTLP_HEADERS` | ✅ |
| `OTEL_EXPORTER_OTLP_TIMEOUT` | ✅ |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | ❌ ignored — gRPC always |
| `OTEL_SERVICE_NAME` | ❌ overridden by the `service_name` argument |
| `OTEL_RESOURCE_ATTRIBUTES` | ◑ read, except `service.name`, which the argument overwrites |

**`OTEL_SERVICE_NAME` loses to the argument.** The SDK's resource builder does
read it, and then the `service_name` you pass overwrites what it found. That
is the opposite of what the OpenTelemetry environment-variable specification
prescribes, and it is a current limitation rather than a decision: pass the
value your environment would have supplied, or read the variable yourself and
hand it in.

**It is last-write-wins, process-wide.** Calling it twice replaces the first
provider and silently orphans it.

If `CountingMetrics` above prints and your collector is still empty, the
problem is in this section, not in the SDK.

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

* **Spans are recorded, not exported for you.** Every call runs in a
  `SERVER` span (see [Spans](#spans) below), but the `otel` feature's
  `init_otlp_pipeline` installs a meter provider only: to export the spans,
  install a `TracerProvider` and the `tracing-opentelemetry` layer yourself.
* **The executor has a span, not a metric.** Its time is the `a2a.execute`
  span's duration; no histogram records it.
* **Task-store operations have no latency instrument.** `persistence_errors`
  counts failures, not slowness — a store degrading toward a timeout shows up
  in request latency first, without saying it was the store.
* **There is no per-tenant metric dimension.** A tenant hitting
  `max_concurrent_tasks` shows as `Overloaded` errors in the aggregate; see
  [Multi-Tenancy](./multi-tenancy.md) for what the limits actually are.
* **The client is not instrumented** the way the server is.

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

Work spawned for the call runs in `INTERNAL` children of that span:
`a2a.execute` (the executor, with the task and context ids),
`a2a.process_events`, `a2a.deliver_push` and `a2a.sse`. A span is exported
when it closes, and `tracing` holds a span open until its children close, so
a call's span reaches your backend once the work it spawned has finished —
with its own end time, the call's, not theirs.

Recording is not free. On a loopback round trip of about 175 µs, a
`tracing-opentelemetry` layer over a batch processor added about 100 µs a
call, almost all of it the bridge building the call's two spans; with no
layer installed the spans cost too little to separate from noise on
JSON-RPC. The measurement and its conditions are in ADR 0013.

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

**Calling.** Add `TracePropagationInterceptor` to the client and it writes
the ambient `CurrentTrace` onto every request over JSON-RPC, REST or gRPC. An
agent that is the first hop starts one with `CurrentTrace::start_root()`.

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
`a2a.execute` span; nothing links one hop's task id to the next hop's.

See also [Troubleshooting](./troubleshooting.md) for the symptom-first version
of this page, and [Production Hardening](./production.md) for health checks.
