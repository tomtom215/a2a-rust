<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Troubleshooting

The page for when something is wrong now. Symptoms first, because that is what
you have at 3am; the explanation is underneath each one.

If you are reading this before anything is wrong, read
[Observability](./observability.md) instead and turn the signals on — most of
the entries below are much shorter when `a2a.server.*` metrics exist.

## Start here: almost nothing is on by default

**`a2a-protocol-server` declares no default features.** That is deliberate — a
protocol library should not drag in a TLS stack, a database driver and an
OpenTelemetry exporter to serve one agent — but it means several "it does
nothing" symptoms are one line in `Cargo.toml`.

Which crate you depend on changes the answer, and this is worth checking before
anything else:

| Crate | Default features |
|---|---|
| `a2a-protocol-sdk` | `tls-rustls` |
| `a2a-protocol-client` | `tls-rustls` |
| `a2a-protocol-server` | **none** |

So a reader who depends on the umbrella `a2a-protocol-sdk` already has TLS, and
one who depends on `a2a-protocol-server` directly does not. Everything else is
off in both cases:

| You want | Feature |
|---|---|
| Structured logs | `tracing` |
| Metrics / traces over OTLP | `otel` |
| SQLite / PostgreSQL task stores | `sqlite`, `postgres` |
| WebSocket, gRPC, Axum | `websocket`, `grpc`, `axum` |
| JWT bearer auth | `auth-jwt` |
| Card signing types | `signing` |

```toml
a2a-protocol-server = { version = "0.10", features = ["tracing", "otel", "sqlite"] }
```

## The agent logs nothing

**Two independent switches, and both must be on.**

1. The `tracing` feature must be enabled, or every logging call in the crate
   compiles to nothing.
2. Your binary must install a subscriber. The library emits events; it does not
   decide where they go.

```bash
# With a subscriber installed, this is the usual first move:
RUST_LOG=a2a_protocol_server=debug,a2a_protocol_client=debug cargo run
```

If logs appear for your own code but not the SDK's, it is switch 1. If nothing
appears at all, it is switch 2.

## No metrics anywhere, and no error either

The `Metrics` trait has a **default no-op implementation for every method**. An
agent with no metrics provider installed records nothing and reports no problem
— which is correct behaviour, and indistinguishable from a broken exporter.

See [Observability](./observability.md) for the full catalogue and for how to
tell "not installed" from "installed and exporting nowhere".

## Push notifications never arrive

Read the outcome label before doing anything else:

```text
a2a.server.push_deliveries{outcome="delivered"}   the webhook accepted it
a2a.server.push_deliveries{outcome="failed"}      reached, and refused it — or the sender errored
a2a.server.push_deliveries{outcome="timeout"}     the webhook did not answer in time
a2a.server.push_deliveries{outcome="skipped"}     nothing was attempted
```

`skipped` is the one that surprises people, because it is **not a network
result — it is a configuration result.** The sender reports how long its own
retry schedule needs, and `push_delivery_timeout` was shorter than that. The
delivery was cut short with attempts still left on the schedule. Nothing was
wrong with the webhook, and nothing will be, however many times you restart it:
either raise `push_delivery_timeout` or shorten the sender's schedule.

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
| Subscriber sees a gap, no error | The queue buffer (`DEFAULT_QUEUE_CAPACITY`, 256) overflowed for a slow consumer | Raise the capacity, or consume faster |

The last one is worth stating plainly: a consumer that is too slow loses
*events*, not the task. The task's stored state remains correct, and refetching
it gives the truth. Streams are a live view, not the record.

## Requests are refused with `Overloaded`

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

## The agent card fails signature verification

`signing` on `a2a-protocol-server` is a **forwarding feature**: it makes the
signing types available and nothing more. **The server never signs the card it
serves.** Sign the card yourself with `sign_agent_card` and hand the signed card
to `RequestHandlerBuilder`; `examples/incident-response` does exactly this.

A card served unsigned verifies as unsigned, which is not a failure the server
will report — it has done what it was asked.

## SQLite: `database is locked`

The SDK's pools set `journal_mode=WAL`, `busy_timeout=5000`,
`synchronous=NORMAL` and `foreign_keys=ON` on every connection, so contention
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
