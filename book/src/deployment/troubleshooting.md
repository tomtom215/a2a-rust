<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Troubleshooting

The page for when something is wrong now. Symptoms first, because that is what
you have at 3am; the explanation is underneath each one.

If you are reading this before anything is wrong, read
[Observability](./observability.md) instead and turn the signals on — most of
the entries below are much shorter when `a2a.server.*` metrics exist.

## Start here: most features are off by default

**`a2a-protocol-server`'s default features are `tracing` and `otel`, and
nothing else.** That is deliberate — a protocol library should not drag in a TLS
stack, a database driver and an OpenTelemetry exporter to serve one agent — but
it means several "it does nothing" symptoms are one line in `Cargo.toml`.

Which crate you depend on changes the answer, and this is worth checking before
anything else:

| Crate | Default features |
|---|---|
| `a2a-protocol-sdk` | `tls-rustls`, `tracing`, `otel`, `otlp`, `webhook` |
| `a2a-protocol-client` | `tls-rustls`, `tracing`, `otel`, `webhook` |
| `a2a-protocol-server` | `tracing`, `otel` |

So a reader who depends on the umbrella `a2a-protocol-sdk` already has TLS, and
one who depends on `a2a-protocol-server` directly does not. Any feature that
the table above does not list for your crate is off until you ask for it:

| You want | Feature |
|---|---|
| Structured logs | `tracing` (default; re-add it if you set `default-features = false`) |
| Traces, metrics and logs over OTLP (`Telemetry`) | `otlp` (default in the SDK; `otel`, its instrumentation, is default everywhere) |
| SQLite / PostgreSQL task stores | `sqlite`, `postgres` |
| WebSocket, gRPC, Axum | `websocket`, `grpc`, `axum` |
| HTTPS on the server's own listener | `tls-rustls` |
| gRPC over TLS | `grpc-tls` |
| JWT bearer auth | `auth-jwt` |
| Card signing types | `signing` |
| Signing the content the server sends | `signing` |

```toml
a2a-protocol-server = { version = "0.15", features = ["otlp", "sqlite"] }
```

## The agent logs nothing

**Two independent switches, and both must be on.**

1. The `tracing` feature must be enabled, or every logging call in the crate
   compiles to nothing. It is on by default in the client, the server and
   the SDK; a build with `default-features = false` has to turn it back on.
2. Your binary must install a subscriber. The library emits events; it does not
   decide where they go.

```bash
# With a subscriber installed, this is the usual first move:
RUST_LOG=a2a_protocol_server=debug,a2a_protocol_client=debug cargo run
```

If logs appear for your own code but not the SDK's, it is switch 1. If nothing
appears at all, it is switch 2.

## No metrics anywhere, and no error either

A handler records through OpenTelemetry's global meter provider, so with no
provider installed (no `Telemetry`, none of your own), or with the `otel`
feature off, it records nothing and reports no problem. A `.with_metrics(..)`
of your own replaces that default; `.with_additional_metrics(..)` counts
beside it. On a dashboard each of these looks the same as a broken exporter.
[Debugging telemetry](./observability/debugging.md#is-the-sdk-recording-at-all)
says how to tell "not installed" from "installed and exporting nowhere".

## Telemetry is configured and nothing reaches the collector

Start with the line the process prints on standard error at startup when it
calls `Telemetry::install_subscriber`: it names where each signal goes
(`traces → otlp http/protobuf http://127.0.0.1:4318/v1/traces; metrics →
off; logs → off`), or says `nothing is exported`. A signal that says `off`
found no endpoint or was set to `none`. A signal that is on but reaches
nothing logs a `WARN … OTLP export … failed`, with the transport's own error
and, where the failure names the fix, a `Hint:` (an untrusted CA, a `401`, a
wrong port or path). [Debugging telemetry](./observability/debugging.md)
walks through each of these warnings and the startup refusals. Three things
it does not cover:

* **A `404` over OTLP/HTTP with a base endpoint that already ends in
  `/v1/traces`**: the exporter appends the signal's path to a base endpoint,
  so it posts to `/v1/traces/v1/traces`. Startup warns about it. Give the
  base (`http://collector:4318`) or use the per-signal variable, which is
  used as given.
* **`BatchSpanProcessor.SpansDropped … max_queue_size=2048`**: the process
  made spans faster than the exporter sent them, and the queue between the
  two was full, so spans were lost (metrics and logs are not affected). Raise
  the queue with `OTEL_BSP_MAX_QUEUE_SIZE` (memory, one span each), or sample
  with `OTEL_TRACES_SAMPLER=parentbased_traceidratio` and
  `OTEL_TRACES_SAMPLER_ARG`. Measured on `examples/swarm`, 4,000 jobs in
  about two seconds: 5,205 spans dropped at the default 2,048, none with
  `OTEL_BSP_MAX_QUEUE_SIZE=65536` (2026-10-28).
* **Warnings from the SDK you did not ask for** are shown too; they were
  silent before. `RUST_LOG=error` quiets them.

## Push notifications never arrive

Read the outcome label before doing anything else:

```text
a2a.server.push_deliveries{outcome="delivered"}   the webhook accepted it
a2a.server.push_deliveries{outcome="failed"}      reached, and refused it — or the sender errored
a2a.server.push_deliveries{outcome="timeout"}     the webhook did not answer in time, and the schedule fit inside push_delivery_timeout
a2a.server.push_deliveries{outcome="timeout_truncated"}  push_delivery_timeout cut the retry schedule short
```

`timeout_truncated` is the one that surprises people, because it is **not
purely a network result — it is a configuration result.** The sender reports
how long its own retry schedule needs, and `push_delivery_timeout` was shorter
than that, so the delivery was cut short with attempts still left on the
schedule. At the shipped defaults that is also what a webhook that never
answers produces: `HttpPushSender::new()` needs 38 seconds at worst (a
5-second DNS bound, three 10-second attempts, 1 s and 2 s of backoff) against
a 30-second bound, so the first two attempts run to their timeouts and only the
third is shortened. Look at the webhook first. To make `timeout` mean that the
webhook was slow, raise `push_delivery_timeout` to at least the sender's
schedule (38 seconds for the default sender), or shorten the schedule to fit
inside the bound.

Every registered config is attempted for every event (spec §4.3.3). The
per-event budget sets how many deliveries run at once, not which are made, so
there is no "skipped" outcome any more; servers older than this release could
report one when a slow webhook estate exhausted the budget.

If the counter shows no datapoints at all under any label, the delivery path
was never reached — check that a push config was actually registered for the
task, and see the next entry.

## An `https://` webhook fails immediately

Without the `tls-rustls` feature the bundled `HttpPushSender` speaks plaintext
HTTP only, and **fails fast** on an `https://` URL rather than silently
downgrading. `a2a-protocol-sdk` enables it by default; a direct dependency on
`a2a-protocol-server` does not. Enable the feature, terminate TLS in front of
the webhook, or supply your own `PushSender` — the trait is public and the
SSRF-validation helpers are reusable.

Failing fast here is the intended behaviour: the alternative — quietly sending
task state over plaintext to a URL whose author asked for TLS — is worse than
an error.

## Streaming stops partway through

Three different limits produce this, and they say so:

| Message | Cause | Fix |
|---|---|---|
| `event size N bytes exceeds maximum M bytes` | One event exceeded `DEFAULT_MAX_EVENT_SIZE` (16 MiB) | Send the payload as an artifact reference, not inline |
| `event queue: the persistence channel was still full after …; the background processor is not draining events` | Events are produced faster than they are persisted | Slow producer, or a stuck store — check `a2a.server.persistence_errors` |
| `event stream lagged: N events were dropped …` (`streamLagged` in the error's `data`), then the stream ends | The reader fell behind by more than the task's event log still holds, or the store keeps no log. A reader that only falls behind the queue (`DEFAULT_QUEUE_CAPACITY`, 256) is caught up from the log and sees nothing | Resubscribe with `Last-Event-ID`; for the in-memory store raise `TaskStoreConfig::max_events_per_task` (512), or use a SQL store, which keeps the whole log; or consume faster |

The last one is worth stating plainly: a consumer too slow to be caught up from
the log loses the *rest of the stream*, not the task, and is told so. The
task's stored state remains correct, and refetching it gives the truth.
Streams are a live view, not the record.

## Requests are refused with `Overloaded`

`ServerError::Overloaded` is a refusal, not a queue: the server turns the
request away at once. The message says which limit was hit; the usual ones
are:

| Message | Cause | Fix |
|---|---|---|
| `tenant '<id>' already has N task(s) in flight` | the tenant is at `max_concurrent_tasks` | raise it, or see why executors do not finish |
| `server at maximum concurrent stream capacity (N); retry later` | `with_max_concurrent_streams` (default 1024) | raise it, or find streams that never close |
| `server is at the maximum of N push notification configs; …` | `HandlerLimits::max_total_push_configs` (default 100,000) | delete unused configs, or raise the limit |

On the wire it is HTTP `503` on the HTTP+JSON binding, `RESOURCE_EXHAUSTED` on
gRPC and `-32603` on JSON-RPC; displayed as an error value it reads `server
overloaded: …`. A deployment with no tenant limits can still see the last two.
A WebSocket connection past its concurrent-request cap is answered `server
busy: too many concurrent requests`, and a send whose idempotency key the task
store cannot claim for the moment is refused the same way, with the store's
own message.

A tenant at its `max_concurrent_tasks` limit is **refused, not queued.** That is
a deliberate choice: queueing converts a declared bound into unbounded latency
and unbounded memory, so the limit would stop meaning anything under exactly the
load it exists for.

The permit is taken before any side effect and held for the life of the spawned
executor, so a leaked permit means an executor that never finished. See
[Multi-Tenancy](./multi-tenancy.md).

## Cancelling a task appears to do nothing

Cancellation in A2A is **cooperative**. The handler triggers the task's
cancellation token; a running `execute` has to observe it. An executor that
never checks the token runs to completion, and the task is recorded `Canceled`
anyway — so the state looks right while the work carried on.

The SDK's default `cancel` already cancels — it emits the terminal `Canceled`
status. Overriding `AgentExecutor::cancel` is not how you opt in; it is how you
**release what the task holds** (a reserved slot, a parked message, an open
handle). If your executor holds nothing, the default is correct.

Check your `execute` loop observes `ctx.cancellation_token`.

With several replicas, a cancel taken by one replica for a task another runs
reaches that executor's token at the owner's next claim renewal, within about
a second, when the store provides turn claims (the PostgreSQL and SQLite
stores do; [Running More Than One Replica](./horizontal-scaling/turn-claims.md#cancelling-a-task-that-runs-elsewhere)).
With a store that does not, or `without_turn_claims()`, it arrives only at the
executor's next write, unless a halt store pushes it.

## The agent card fails signature verification

`signing` on `a2a-protocol-server` makes the signing types available and
enables `RequestHandlerBuilder::with_content_signer`, which signs the messages
and artifacts the server sends. **The server never signs the card it
serves.** Sign the card yourself with `sign_card_es256` and hand the signed card
to `RequestHandlerBuilder`; `examples/incident-response` does exactly this.

A card served unsigned verifies as unsigned, which is not a failure the server
will report — it has done what it was asked.

## SQLite: `database is locked`

The SDK's pools set `journal_mode=WAL`, `busy_timeout=5000`,
`synchronous=NORMAL`, `foreign_keys=ON` and `secure_delete=ON` on every
connection, so contention
inside one process waits rather than failing. Seeing `SQLITE_BUSY` anyway
usually means a second writer the SDK did not open — another process, or a pool
you built yourself without those pragmas.

SQLite is a single-writer database. If you need concurrent writers across
processes, that is the point at which to move to PostgreSQL; see
[Running More Than One Replica](./horizontal-scaling.md).

## Building with `grpc` fails looking for `protoc`

It should not — `protoc-bin-vendored` is a dependency of the `grpc` feature, so
no system `protoc` is required. If a build still asks for one, something in your
environment is pointing `PROTOC` at a path that does not exist; unset it.

## What to attach to a bug report

```text
1. a2a-protocol-* versions, and the exact feature list from Cargo.toml
2. rustc --version
3. Which binding (JSON-RPC / HTTP+JSON / gRPC / WebSocket)
4. The task's terminal state, and the error verbatim
5. Whether tracing/otel were enabled — most reports say "no logs" and mean this
```

The feature list matters more than it looks: most "the SDK does not do X"
reports are X compiled out.
