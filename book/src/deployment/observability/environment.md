<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Environment variables

`Telemetry` reads the environment as the OpenTelemetry specification defines
it, and most variables have a builder method that sets the same thing in code.
This page is the one table of both: what each variable does, its per-signal
form, the method that matches it, which of the two wins, and the default.

A value for an exporter, a protocol, a compression, a timeout, a certificate or
a header that it cannot act on is an error naming the variable, and `build`
fails. A few are reported and skipped instead (an `OTEL_PROPAGATORS` name it
does not have, a proxy kind it does not speak), and the OpenTelemetry crates
fall back on their own for others (an unrecognised `OTEL_TRACES_SAMPLER`
becomes `parentbased_always_on`, with a warning printed before any subscriber
exists, so it is usually not seen). An empty value is read as unset, as the
specification asks.

## The matrix

"Per signal" means the variable also exists as
`OTEL_EXPORTER_OTLP_TRACES_…`, `…_METRICS_…` and `…_LOGS_…`, and the
signal's own form wins over the general one.

| Variable | Per signal | Builder method | Which wins | Default | Effect |
|---|---|---|---|---|---|
| `OTEL_SDK_DISABLED` | — | — | — | unset | `true` (in any case) builds no providers and no exporters, the layer records nothing and `install_subscriber` installs nothing; the W3C propagators are still installed. `1` does not disable |
| `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER` | each is one signal's | `with_traces`, `with_metrics`, `with_logs` | **The variable** | `otlp` (metrics and logs off with `with_langfuse`) | A comma list: `otlp`, `console` (print to standard output) and, for metrics with the `otel-prometheus` feature, `prometheus` (serve `/metrics`); `none` anywhere turns the signal off. Any other value (`zipkin`, `jaeger`, …) is an error naming it |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | yes | `with_otlp_endpoint` | **The code** | `http://localhost:4318` (HTTP), `http://localhost:4317` (gRPC) | The collector. OTLP/HTTP appends `/v1/traces`, `/v1/metrics` or `/v1/logs` to the general variable and to the code's URL; a per-signal variable is used as given. Over OTLP/HTTP it needs its scheme |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | yes | `with_otlp_protocol` | **The code** | `http/protobuf` | `grpc`, `http/protobuf` or, with the `otlp-json` feature, `http/json`; without it `http/json` is an error naming the feature |
| `OTEL_EXPORTER_OTLP_INSECURE` | yes | — | — | `false` | Only for an OTLP/gRPC endpoint written without a scheme: `true` reads it as `http://`, otherwise `https://` |
| `OTEL_EXPORTER_OTLP_HEADERS` | yes | `with_otlp_header` | **The variable**, for a header of the same name; headers with different names are all sent | none | Sent with every export |
| `OTEL_EXPORTER_OTLP_TIMEOUT` | yes | — | — | 10 s | Milliseconds, per export |
| `OTEL_EXPORTER_OTLP_COMPRESSION` | yes | `with_compression` | **The code**, but code cannot turn off what the variable turned on | none | `gzip` or `none`; `zstd` (it needs a C library) and anything else is an error naming the variable |
| `OTEL_EXPORTER_OTLP_CERTIFICATE` | yes | `with_ca_certificate_pem` | **Both**: the code's CAs are added to the file's | Mozilla roots | A PEM file of CA certificates trusted alongside the bundled Mozilla roots |
| `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE`, `OTEL_EXPORTER_OTLP_CLIENT_KEY` | yes | `with_client_certificate_pem` | **The variables** | none | PEM files of the client certificate chain and its key, for a collector that requires mutual TLS; one without the other is an error |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` (either case) | — | `with_proxy`, `without_proxy` | **The code** | direct | Export goes through an `http://` proxy, as curl reads them; see [TLS, proxies and compression](./tls-proxies.md#proxies) |
| `OTEL_EXPORTER_PROMETHEUS_HOST`, `OTEL_EXPORTER_PROMETHEUS_PORT` | — | — | — | `localhost`, 9464 | Where `prometheus` serves `/metrics`; port `0` lets the operating system choose (`Telemetry::prometheus_addr` returns it) |
| `OTEL_PROPAGATORS` | — | — | — | `tracecontext,baggage` | `tracecontext`, `baggage`, `none`, in a list; another name (`b3`, `jaeger`, …) is reported and skipped (`Telemetry::notices`), not an error |
| `OTEL_SERVICE_NAME` | — | `with_default_service_name` | **The variable** | `unknown_service` | `service.name`; see [Resource attributes](./resource.md) |
| `OTEL_RESOURCE_ATTRIBUTES` | — | — | — | — | Resource attributes, `key=value,…`. `service.name` here also wins over `with_default_service_name`. `,` and `=` inside a value are written percent-encoded (`prod%2Cwest` is `prod,west`) and decoded; an entry that is not valid percent-encoding is kept as written and reported at startup |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | — | — | — | `parentbased_always_on` | Read by the OpenTelemetry SDK itself |
| `OTEL_BSP_*`, `OTEL_BLRP_*` | — | — | — | the SDK's | The span and log batch processors' queue size, batch size and delay, read by the OpenTelemetry SDK |
| `OTEL_METRIC_EXPORT_INTERVAL` | — | — | — | 60 s | Milliseconds between metric exports, read by the OpenTelemetry SDK |
| `OTEL_SPAN_*_LIMIT` | — | — | — | the SDK's | Span attribute, event and link limits, read by the OpenTelemetry SDK |
| `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE` | metrics only | — | — | cumulative | `delta` for a backend that wants it, read by `opentelemetry-otlp` |
| `RUST_LOG` | — | — | — | `warn` | What `install_subscriber` prints to standard error, in `target=level` form, not what is exported; see [Logs](./logs.md#what-is-exported-and-what-is-printed) |
| `NO_COLOR` | — | — | — | unset | Any non-empty value turns off colour in what `install_subscriber` prints; colour is used only on a terminal anyway |
| `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY`, `LANGFUSE_BASE_URL` | — | `Langfuse::new` | — | base URL `http://localhost:3000` | Read by `Langfuse::from_env()`; see [Langfuse](../langfuse.md) |

The variables the OpenTelemetry crates read themselves (the sampler, the
batch processors, the export interval, the limits, the temporality) were not
re-implemented here; the sampler, the delta temporality and the headers were
each run against a collector on 2026-10-26.

## Which setting wins

As in the OpenTelemetry SDK, a value set in code wins over the variable: that
is `with_otlp_endpoint`, `with_otlp_protocol`, `with_compression` and the
proxy methods. Four settings differ:

* `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER` and `OTEL_LOGS_EXPORTER` win
  over `with_traces`, `with_metrics` and `with_logs`, which in turn win over
  `with_export_where_configured`.
* A header variable that names the same header as `with_otlp_header` wins over
  it (headers with different names are all sent).
* `with_ca_certificate_pem` adds to the CA file `OTEL_EXPORTER_OTLP_CERTIFICATE`
  names rather than replacing it, and both are trusted.
* The client certificate files the environment names win over
  `with_client_certificate_pem`.

The certificate and key are parsed when the exporters are built, so a file
that is not PEM, or (on OTLP/HTTP) a key that is not the certificate's, fails
`build` rather than every export. An unreadable file stops the process at
startup, naming the variable.

## Choosing which signals are built

Three things decide whether a signal gets an exporter, in this order:

1. `OTEL_SDK_DISABLED=true` turns all three off.
2. The signal's `OTEL_*_EXPORTER` variable, when set, decides (`none` is off).
3. Otherwise `with_traces` / `with_metrics` / `with_logs`, when called, decide;
   otherwise, with `with_export_where_configured()`, the signal is built only if
   an endpoint is configured for it — `OTEL_EXPORTER_OTLP_ENDPOINT`, its own
   `OTEL_EXPORTER_OTLP_<SIGNAL>_ENDPOINT`, `with_otlp_endpoint`, or (traces)
   `with_langfuse`; without it, every signal is built. Under `with_langfuse`,
   metrics and logs default to off either way.

`reference/configuration.md` lists these variables beside the rest of the
SDK's configuration ([Configuration Reference](../../reference/configuration.md)).
