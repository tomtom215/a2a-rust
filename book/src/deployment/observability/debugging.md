<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Debugging telemetry

When nothing reaches your backend, there are three questions, in order: is the
SDK recording at all, where does the process think it is sending, and what
happened to the export. This page answers each with what the process itself
prints. [Troubleshooting](../troubleshooting.md#telemetry-is-configured-and-nothing-reaches-the-collector)
has the same ground symptom-first, with more causes per symptom.

## Seeing it with no collector

`OTEL_TRACES_EXPORTER=console`, and `OTEL_METRICS_EXPORTER=console` and
`OTEL_LOGS_EXPORTER=console`, print a signal to standard output: one line per
span, per metric data point and per log record. It needs no collector and no
network, and a process that prints and sends nothing else starts no export
thread. The values are a comma list, so `otlp,console` sends and prints at
once. These are `hello-agent`'s own lines from one run (one `SendMessage`, then
SIGTERM, which printed the metrics a last time), shortened at the right:

```text
2026-10-26T11:49:38.791Z LOG INFO dispatching JSON-RPC request method="\"SendMessage\""
2026-10-26T11:49:38.791Z LOG INFO handling send message trace=041b7a3f712ac2bfa0b7597940c4458e span=5aaeaa8e92dbcf3f method=SendMessage streaming=false
2026-10-26T11:49:38.792Z SPAN "invoke_agent hello-agent" internal 1.1ms trace=041b7a3f712ac2bfa0b7597940c4458e span=aab988af2f4d0357 parent=5aaeaa8e92dbcf3f events=4 target=a2a_protocol_server::rpc a2a.task.id=…
2026-10-26T11:49:38.791Z SPAN lf.a2a.v1.A2AService/SendMessage server 2.7ms trace=041b7a3f712ac2bfa0b7597940c4458e span=5aaeaa8e92dbcf3f events=7 target=a2a_protocol_server::rpc rpc.system.name=jsonrpc …
2026-10-26T11:49:39.300Z METRIC a2a.server.requests{method=SendMessage} sum 1 {request} cumulative
2026-10-26T11:49:39.300Z METRIC rpc.server.call.duration{rpc.system.name=jsonrpc,rpc.method=lf.a2a.v1.A2AService/SendMessage} histogram count=1 sum=0.002532315 s cumulative min=0.002532315 max=0.002532315
```

Metrics are printed once per export interval, 60 s unless
`OTEL_METRIC_EXPORT_INTERVAL` (milliseconds) says otherwise, and once more when
`Telemetry` is shut down. A span line names its parent, the span's events are
counted rather than listed (the `tracing` events inside a span are `LOG` lines
of their own when logs are printed too), and the `code.*`, `thread.*`,
`busy_ns` and `idle_ns` attributes `tracing-opentelemetry` adds to every span
are left off the line — they are still exported. The format is for reading,
not for parsing, and may change; send OTLP to anything that parses.

Spans appear when they end, so a request still running has none yet.

## Is the SDK recording at all?

`Metrics` has a default no-op implementation for every method, and a handler
records through OpenTelemetry's global meter provider, so with none installed
it records nothing and reports no problem. On a dashboard, "not installed"
and "installed and exporting nowhere" look the same. The smallest provider
of your own proves the calls are arriving:

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

Hand it to the builder with `.with_additional_metrics(CountingMetrics)` (not
`.with_metrics`, which replaces the OpenTelemetry default and so stops the
export you are trying to debug). If those lines appear and your dashboard is
still empty, the problem is the exporter configuration, not the SDK — which is
the distinction this two-minute step buys you. The console exporter above
answers the same question for spans and logs.

## Where does the process think it is sending?

`install_subscriber` prints what a person needs when export does not work, on
standard error. First, once at startup, where each signal goes, or, when all
three are off, that nothing is exported and how to change that (one of these
two lines, not both):

```text
INFO a2a_protocol_server::otel::telemetry: telemetry: traces → otlp http/protobuf http://127.0.0.1:4318/v1/traces, gzip; metrics → off; logs → off
INFO a2a_protocol_server::otel::telemetry: telemetry: nothing is exported; set OTEL_EXPORTER_OTLP_ENDPOINT to send it to a collector, or OTEL_TRACES_EXPORTER=console to print spans
```

The destination has any `user:password@` removed from its URL, and names gzip
and the proxy it goes through when there is one. A signal that says `off` is
off because its `OTEL_*_EXPORTER` is `none`, or because
`with_export_where_configured()` found no endpoint for it
([Environment variables](./environment.md#choosing-which-signals-are-built)).
Under `OTEL_SDK_DISABLED=true` the line is `telemetry: disabled by
OTEL_SDK_DISABLED`, and `install_subscriber` installs nothing.

A program that wants no startup line (a command-line tool, say) builds with
`TelemetryBuilder::without_startup_summary()`: warnings and errors still print.
The `a2a` CLI does.

A process with a subscriber of its own gets the same information from
`Telemetry::describe()`, which returns the startup line, and
`Telemetry::notices()`, what the configuration asked for that this SDK does
not do and went on without (an `OTEL_PROPAGATORS` name it does not have), to
print its own way.

## When something is wrong

When an export is lost after the exporter's retries, a warning names the
signal and where it was going, at most once every 30 s with a count of the
ones held back:

```text
WARN a2a_protocol_server::otel::telemetry::counting: OTLP export of traces to https://localhost:4318/v1/traces failed after the exporter's retries and the batch was dropped: Operation failed: HTTP export failed: network error (cause: client error (Connect): invalid peer certificate: UnknownIssuer). Hint: the collector's certificate was not issued by a CA this exporter trusts (it trusts Mozilla's roots, not the operating system's): for a private CA, set OTEL_EXPORTER_OTLP_CERTIFICATE=/path/to/ca.pem
```

`opentelemetry-otlp` 0.33.1 reports every OTLP/HTTP transport failure as the
fixed text `HTTP export failed: network error`, so this crate keeps the
transport's own error and appends it as the `cause`. The `Hint` appears only
where a fact in the failure names the fix:

| The failure | The hint points at |
|---|---|
| A certificate from a CA the exporter does not trust | `OTEL_EXPORTER_OTLP_CERTIFICATE` |
| A certificate for another host name, or one that has expired | The certificate |
| A `401` or `403` | The headers (`OTEL_EXPORTER_OTLP_HEADERS`) |
| A `404` over OTLP/HTTP | The paths the exporter posts to |
| No HTTP status, to port 4317 over HTTP or port 4318 over gRPC | The other transport's usual port |

A bare refused connection gets no hint. What else is printed is warnings and
errors from everywhere, plus the startup line; `RUST_LOG=my_agent=debug,warn`
(the `target=level` form) changes it ([Logs](./logs.md#what-is-exported-and-what-is-printed)).

`shutdown` and `force_flush` return `TelemetryError::Export` naming each signal
that lost exports and how many, so a process that exits with an error from
`shutdown` lost telemetry even if no warning was seen.

## A value the process refuses

The process refuses to start, naming a variable, when it is a value the
exporter cannot act on: `zstd` compression, an OTLP/HTTP endpoint with no
scheme, `prometheus` where the `otel-prometheus` feature is not built, an
exporter name it does not know, an unreadable certificate file. A value it
cannot act on is `TelemetryError::Config`, with the variable, its value and
the reason
([Configuring `Telemetry`](./telemetry.md#flushing-and-shutting-down)).
