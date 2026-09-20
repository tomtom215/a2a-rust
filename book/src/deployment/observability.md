<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Observability

Two independent things, often confused: **logs** say what happened in one
request, **metrics** say what is happening across all of them. This SDK ships
both, and **neither is on by default**.

That default is deliberate. A protocol library that pulled in an OpenTelemetry
exporter to serve one agent would be the wrong trade for most users. The cost
is that "I see no metrics" is the single most common report against this crate,
and the answer is almost always this page's first section.

## Turning them on

```toml
a2a-protocol-server = { version = "0.13", features = ["tracing", "otel"] }
```

* **`tracing`** — the crate's logging calls compile to nothing without it. With
  it, your binary still has to install a subscriber; the library emits events
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

Counters gain `_total`, the histogram gains `_seconds` from its `s` unit,
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

The deviation that *is* visible: OpenTelemetry names duration histograms
`.duration`, so the conventional name would be
`a2a.server.request.duration` → `a2a_server_request_duration_seconds`,
which is what a dashboard template built for OTel will look for.
`a2a_server_latency_seconds` will not match it.

Neither is corrected in place, because the catalogue is published as a
contract — see the line under *The catalogue* above — and changing a
published contract is a breaking change that belongs in its own release
with its own upgrade note. Both are recorded in `docs/handoff.md`.

## The four signals worth alerting on

**`errors` / `requests`.** The obvious one. Split by `method`: a rising error
rate confined to `message/stream` is a different incident from one across
everything.

**`persistence_errors` above zero, at all.** A store write that fails means the
task's recorded state and its actual state have diverged. Streams recover from
loss by refetching; this is the case where refetching returns the wrong answer.
Alert on any non-zero rate rather than on a threshold.

**`queue_depth` that does not come down.** Each active stream holds a queue.
A depth that grows monotonically is subscribers that never closed — usually a
client that stopped reading without disconnecting.

**`push_deliveries{outcome="skipped"}`.** Not a network failure. It means
`push_delivery_timeout` was shorter than the sender's own retry schedule, so
the delivery was abandoned with attempts remaining. It will not resolve on its
own and it will not appear as a webhook error, because the webhook was never
the problem. The four labels:

```text
delivered   the webhook accepted it
failed      reached, and refused it — or the sender itself errored
timeout     the webhook did not answer inside the time it was given
skipped     a configuration result: push_delivery_timeout cut the schedule short
```

## What is not measured

Stated so a green dashboard is not mistaken for a complete one:

* **No spans are exported.** The `otel` feature exports metrics and nothing
  else: no `TracerProvider`, no span export, no durations recorded. Metrics
  can say *this* server was slow; they cannot say a 40-second task was 38
  seconds waiting two hops away.

  Trace **context** is a separate thing and it is carried, as of 0.13 — see
  [Trace context](#trace-context) below. That gives every hop in a
  delegation chain the same trace id, which is what a collector needs to
  join them. Recording the spans themselves is still yours to install.
* **Nothing here measures the executor.** Latency is request latency; time spent
  inside your `AgentExecutor` is yours to instrument.
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

Task and context identifiers are on the spans, so one request can be followed
through **this** process. If you emit your own events from inside an executor,
they inherit that context.

**Logs alone still do not join a delegation chain.** An early revision of
this page said they did, which was wrong: two processes produce two span
trees, and correlating by task id does not rescue it because each agent mints
its own. What joins them is the trace id below.

## Trace context

Since 0.13 the SDK carries [W3C Trace
Context](https://www.w3.org/TR/trace-context/) across an A2A hop. It is
propagation, not tracing: nothing here records a span or exports one. What it
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
(W3C §3.3.1.5). There is no sampler. The `a2a.task.id` of each hop is still
not attached to anything, because there is no span to attach it to.

See also [Troubleshooting](./troubleshooting.md) for the symptom-first version
of this page, and [Production Hardening](./production.md) for health checks.
