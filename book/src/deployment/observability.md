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
a2a-protocol-server = { version = "0.11", features = ["tracing", "otel"] }
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

Task and context identifiers are on the spans, so a single incident can be
followed across the delegation chain when agents call agents. If you emit your
own events from inside an executor, they inherit that context.

See also [Troubleshooting](./troubleshooting.md) for the symptom-first version
of this page, and [Production Hardening](./production.md) for health checks.
