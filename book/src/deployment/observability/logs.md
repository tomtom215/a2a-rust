<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Logs

The SDK logs through the `tracing` crate: warnings when something degrades
(a store retrying, a push delivery failing, a stream lagging), errors when
work is lost, and debug detail of each call. This page covers how those
events become exported log records, how that differs from what the process
prints, how to filter each, and how the shipped stack links a log line to its
trace and back.

## Where the events go

With the `tracing` feature (on by default) the SDK emits events and spans and
leaves where they go to your subscriber. Without `tracing`
(`default-features = false`) the logging calls compile to nothing. A process
can send them two places, independently:

| Destination | Set up by | Filtered by | Default |
|---|---|---|---|
| Exported as OpenTelemetry log records | `Telemetry::layer()` (and so `install_subscriber`) | `TelemetryBuilder::with_log_level` | `INFO` and above |
| Printed on standard error | `install_subscriber`'s printing layer | `RUST_LOG`, in `target=level` form | `WARN` and above, plus the startup line |
| Printed by a subscriber of your own | You (`tracing_subscriber::fmt`, …) | Whatever you install, usually `EnvFilter` | Yours |

## What is exported

`telemetry.layer()` turns each `tracing` event at or above the log level into
an OpenTelemetry log record — the event's message as the body, its level as
the severity, its fields as attributes — and exports it with the logs signal
(`OTEL_LOGS_EXPORTER`, `otlp` by default; [Environment
variables](./environment.md)). An event emitted inside a recorded span carries
that span's trace and span ids, so a backend can show the log lines of one
span: on the compose stack, Loki received `trace_id` and `span_id` on every
line written inside a call (2026-10-28). Events from the exporter's own stack
(`opentelemetry*`, `hyper`, `h2`, `tonic`, `tower`, `rustls` and the SDK's
`otel::telemetry` module) are never exported, so an export failure cannot log
a record whose export fails.

`TelemetryBuilder::with_log_level(level)` sets the least severe event
exported (default `INFO`; `tracing::Level::DEBUG` exports debug events too).
It filters log records only: spans are not filtered by level here. A
subscriber-wide filter, such as an `EnvFilter` added to the registry as a
layer of its own, applies to both — see [below](#rust_log-and-your-own-subscriber).

Task and context identifiers are on the executor's span (`a2a.task.id`,
`a2a.context.id`), and an event inherits its span's context, so one request
can be followed through this process by its trace. Events you emit from inside
an executor inherit it too.

## What is exported and what is printed

`install_subscriber` installs the export layer and a printing layer on
standard error. The printing layer shows warnings and errors from everywhere,
plus the startup line (`info` for the SDK's own telemetry module). `RUST_LOG`
changes what it prints, in the `target=level` form (not the full `EnvFilter`
syntax), and a value that is not understood is said, not ignored:

```bash
RUST_LOG=my_agent=debug,warn my-agent
```

That changes nothing about what is exported: with `install_subscriber`, every
span is exported and events at the log level and above become log records,
whatever `RUST_LOG` says. With the console log exporter on
(`OTEL_LOGS_EXPORTER=console`), events are not printed a second time on
standard error. Colour is used only when standard error is a terminal, and
never when `NO_COLOR` is set to a non-empty value.

## `RUST_LOG` and your own subscriber

`RUST_LOG` takes effect in your own subscriber only through an `EnvFilter` you
install (`tracing_subscriber::EnvFilter`, with `tracing-subscriber`'s
`env-filter` feature). The usual first move when debugging the SDK itself:

```bash
RUST_LOG=a2a_protocol_server=debug,a2a_protocol_client=debug cargo run
```

Where you put the filter decides what it filters. Added to the registry as a
layer of its own, it filters every layer, the export included — and
`EnvFilter::from_default_env()` with `RUST_LOG` unset enables only `ERROR`,
which would leave no `INFO` span to export. Attached to the printing layer
alone, it filters only what is printed:

```rust,no_run
use a2a_protocol_sdk::server::otel::Telemetry;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent")
    .with_export_where_configured()
    .build()?;
// RUST_LOG for the console only; `info` when it is unset.
let printed = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
tracing_subscriber::registry()
    .with(telemetry.layer())
    .with(tracing_subscriber::fmt::layer().with_filter(printed))
    .init();
# Ok(())
# }
```

A process with a subscriber of its own does not get `install_subscriber`'s
startup line and warnings about lost exports unless it prints them:
`telemetry.describe()` returns the line and `telemetry.notices()` the
settings it went on without ([Debugging telemetry](./debugging.md)).

## Logs without OTLP

To ship logs through a log collector that reads standard output rather than
OTLP, keep the export off (`OTEL_LOGS_EXPORTER=none`) and print JSON lines
from your own subscriber: `tracing_subscriber::fmt::layer().json()`, with
`tracing-subscriber`'s `json` feature, beside `telemetry.layer()` as above.
Each line then carries the event's fields and its span's fields, including
`a2a.task.id` from the executor's span; it does not carry the OpenTelemetry
trace id, so linking a line to its trace needs the OTLP path below.

## Logs in Loki, linked to traces

The compose stack (`deploy/observability/docker-compose.yml`) sends logs over
OTLP to the Collector, whose `otlphttp/loki` exporter forwards them to Loki's
OTLP endpoint (`http://loki:3100/otlp`). Loki is configured with
`allow_structured_metadata: true`, which OTLP ingestion needs.
`grafana/provisioning/traces-and-logs.yml` provisions the Tempo and Loki data
sources and links them both ways:

* **From a span to its logs.** Tempo's `tracesToLogsV2` points at Loki with
  `filterByTraceID: true`, searching five minutes either side of the span, so
  a span in Tempo opens the log lines written inside its trace.
* **From a log line to its trace.** Loki's derived field `trace_id` points at
  Tempo, so a log line that carries a trace id opens its trace.

[Walkthrough: the local stack](./walkthrough.md#6-follow-a-trace-to-its-logs)
does this end to end.

## The SDK's own log lines worth knowing

* **Statement-level SQL.** sqlx reports each statement as a `tracing` event
  at `DEBUG`, or at `WARN` when it takes a second or more (target
  `sqlx::query`, with the SQL as `db.statement` — placeholders, not the bound
  values). Under `Telemetry` the slow ones are log records at the default
  level, every one with `with_log_level(Level::DEBUG)`.
* **Store retries.** "task store unavailable; writing the event again" is
  logged at `WARN` at most once every ten seconds, the rest at `DEBUG`; the
  `a2a.server.store_retries` counter, not the log, is the measure.
* **A failed executor** is logged at `ERROR` with its task and cause.
