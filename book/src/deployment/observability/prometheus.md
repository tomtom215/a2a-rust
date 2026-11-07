<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Prometheus

There are four ways to get an agent's metrics into Prometheus, and the shipped
dashboards and rules work with all four, because each ends with the same
metric and label names. This page compares them, explains the Prometheus
configuration files in `deploy/observability/prometheus/`, and gives the name
translation that connects an OpenTelemetry instrument to its series.

| Path | Signals | Use it when | Configuration |
|---|---|---|---|
| Agent → Collector → Prometheus's OTLP receiver | all three (traces to Tempo, logs to Loki) | You want traces and logs too; the compose stack does this | `docker-compose.yml`, `otel-collector/config-compose.yaml`, `prometheus/prometheus-compose.yml` |
| Agent → Prometheus's OTLP receiver | metrics | You run Prometheus and nothing else | `prometheus/prometheus.yml` |
| Agent → Collector ← Prometheus scrapes `:8889` | all three (traces and logs to the Collector's `debug` exporter until you add backends) | You already scrape a Collector | `otel-collector/config.yaml`, `prometheus/prometheus-collector.yml` |
| Prometheus scrapes the agent's `/metrics` | metrics | No collector at all, Prometheus scrapes pods | `otel-prometheus` feature, `OTEL_METRICS_EXPORTER=prometheus` |

## Push to Prometheus directly

Prometheus 3 receives OTLP itself when started with
`--web.enable-otlp-receiver`. Point the agent's metrics at it and turn the
other two signals off, since Prometheus takes metrics only:

```bash
OTEL_SERVICE_NAME=my-agent \
OTEL_RESOURCE_ATTRIBUTES=service.instance.id=$(hostname) \
OTEL_EXPORTER_OTLP_ENDPOINT=http://prometheus:9090/api/v1/otlp \
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \
OTEL_TRACES_EXPORTER=none OTEL_LOGS_EXPORTER=none \
my-agent
```

OTLP/HTTP appends `/v1/metrics` to the base endpoint, which is the path
Prometheus's receiver serves under `/api/v1/otlp`. `service.name` becomes the
`job` label and `service.instance.id` the `instance` label.

## Through the Collector into Prometheus

The compose stack's Collector (`otel-collector/config-compose.yaml`) receives
all three signals on 4317 and 4318 and forwards metrics with its
`otlphttp/prometheus` exporter to `http://prometheus:9090/api/v1/otlp`, traces
to Tempo and logs to Loki. Because metrics still arrive at Prometheus over
OTLP, `job` and `instance` come out exactly as when an agent pushes to
Prometheus itself, and nothing downstream changes. The agent sets only:

```bash
OTEL_SERVICE_NAME=my-agent \
OTEL_RESOURCE_ATTRIBUTES=service.instance.id=$(hostname) \
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
my-agent
```

Both Collector configurations put `memory_limiter` first in every pipeline (a
burst is refused and the agent retries, rather than the Collector being killed
for memory; its limits are 80% and 20% of the container's memory limit) and
answer health checks on 13133 (`GET /` is 200 once the pipelines run). The
compose file gives the Collector a 1 GiB limit, so the limiter refuses above
819 MiB (its startup log said so on 2026-11-04); until 0.16.0 only the
production overlay set one, and without it the limiter takes its share of the
host's memory (audit C-15). Its exporters keep their queues on disk
(`file_storage`), so a restart sends what was queued.

### The Collector watches itself

Both configurations serve the Collector's own metrics on 8888
(`service.telemetry.metrics`), and Prometheus scrapes them as job `otelcol`:
received, refused and failed items per receiver, sent and failed items and
the queue's size and capacity per exporter. `rules/stack-alerts.yml` alerts on
a Collector that cannot be scraped, an exporter that gives up on items, a
receiver that refuses, and a queue over 80% full ([Alerts and
runbooks](./alerts.md#otelcollectordown)). otelcol-contrib 0.163.2 names
these counters without `_total` (`otelcol_receiver_refused_spans`,
`otelcol_exporter_send_failed_log_records`); the rules accept both spellings.
The compose Prometheus also scrapes Alertmanager, Tempo and Loki.

## Scrape a Collector

`otel-collector/config.yaml` is the Collector on its own: OTLP in on 4317 and
4318, and a `prometheus` exporter serving the metrics on 8889 with
`translation_strategy: UnderscoreEscapingWithSuffixes` and
`resource_to_telemetry_conversion` off. `prometheus/prometheus-collector.yml`
scrapes it:

```yaml
scrape_configs:
  - job_name: otel-collector
    honor_labels: true
    static_configs:
      - targets: ["otel-collector:8889"]
```

`honor_labels: true` is what keeps the agents apart. The exporter labels each
series with the agent's own `job` (`service.name`) and `instance`
(`service.instance.id`); without `honor_labels` Prometheus replaces them with
the Collector's, and every agent's series reads `job="otel-collector"` with the
agent moved to `exported_job` (checked against a live Prometheus). Traces and
logs go to the Collector's `debug` exporter in this file: add exporters for
your trace and log backends.

## Scrape the agent, no collector

With the `otel-prometheus` feature, `OTEL_METRICS_EXPORTER=prometheus` serves
the metrics at `/metrics` in the Prometheus text format, for a Prometheus that
scrapes this process with no collector between them. It listens on
`OTEL_EXPORTER_PROMETHEUS_HOST` (`localhost`) and `_PORT` (9464), the
specification's defaults, and is the `otlp,prometheus` list's other half if
you want both. Port `0` lets the operating system choose;
`Telemetry::prometheus_addr()` returns the address it bound.

The names are the ones [below](#what-the-names-become), because it is the same
translation: on `hello-agent` one `SendMessage` made
`a2a_server_requests_total{method="SendMessage",otel_scope_name="a2a.server"} 1`
and `rpc_server_call_duration_seconds_count{…} 1`. The listener is bound when
`Telemetry` is built, so a port in use fails there and names the variable. It
answers `GET` and `HEAD` on `/metrics`, `405` with `Allow: GET, HEAD` to any
other method there, and `404` elsewhere.

It is plain HTTP and unauthenticated: with `OTEL_EXPORTER_PROMETHEUS_HOST=0.0.0.0`
(a scraper on another machine needs that or an address) anyone who can reach
the port can read every metric, so it belongs behind the network policy that
already protects your other metrics ports. Without the feature the value is
noted (`Telemetry::notices`, printed at `WARN`) and dropped, and a signal with
no other exporter named gets its default; until 2026-11-01 it was an error and
the process did not start. The feature is off by
default, in the SDK as well, and `deploy-agent` is built without it.

The listener serves at most 16 connections at once (a further one waits in the
listen backlog), one request per connection (`Connection: close`), and closes
any connection after 30 seconds however far it got, with 10 seconds for the
request's headers; until 2026-11-04 it took every connection and kept each
open as long as the client liked (audit C-16).

### On the application's own port

To serve `/metrics` behind the application's own authentication and
connection limits, bind nothing and serve the same text yourself: build with
`Telemetry::builder().without_prometheus_listener()` (then
`OTEL_EXPORTER_PROMETHEUS_HOST` and `_PORT` are not read), take
`telemetry.prometheus()`, and answer `GET /metrics` in your own router, behind
your own authentication, with `200`, a `Content-Type` of
`PrometheusScrape::CONTENT_TYPE` and the body `scrape.render()?`.

`Telemetry::prometheus()` is `None` unless the `prometheus` exporter is
chosen, and `prometheus_addr()` is `None` without the listener. The default
host stays `localhost`, the specification's (ADR 0038 §3).

A scraped agent has an `up` series; one that pushes has none, which is why
the shipped `A2AAgentSilent` alert watches `target_info` instead.

## The Prometheus configuration

`prometheus/prometheus.yml` is Prometheus for agents that push OTLP to it,
directly or through a Collector. `prometheus/prometheus-compose.yml`, which
the compose stack runs, is the same file plus an `alerting` block (its
Alertmanager) and scrape jobs for the stack's own components;
`scripts/check_stack.py` fails if their shared blocks differ:

```yaml
otlp:
  promote_resource_attributes:
    - service.version
    - deployment.environment.name
  translation_strategy: UnderscoreEscapingWithSuffixes

storage:
  tsdb:
    out_of_order_time_window: 30m
    retention:
      time: 7d
      size: 5GB

rule_files:
  - rules/*.yml
```

* **`promote_resource_attributes`.** `service.name` and `service.instance.id`
  become `job` and `instance` without promotion; these two more are promoted
  to labels on every series so dashboards can group by them. Every other
  resource attribute stays on `target_info` only.
* **`translation_strategy: UnderscoreEscapingWithSuffixes`.** Dots become
  underscores and the unit and type suffixes are added
  (`rpc.server.call.duration` → `rpc_server_call_duration_seconds`), the same
  names the Collector's Prometheus exporter and the agent's own `/metrics`
  produce, so the rules and dashboards work on every path.
* **`out_of_order_time_window: 30m`.** OTLP pushes arrive out of order under
  retries; this accepts samples up to 30 minutes late rather than dropping
  them.
* **`retention`.** Seven days or 5 GB of blocks, whichever comes first; the
  size bound keeps a burst of new series from filling the disk (audit C-19).
  Until 0.16.0 the compose stack set only `--storage.tsdb.retention.time=7d`,
  a flag Prometheus 3.15 marks deprecated in favour of this block.
* **`rule_files`** loads the recording and alerting rules
  ([Dashboards, alerts and recording rules](./dashboards.md)).
  `prometheus-collector.yml` loads the same rules.

## The Prometheus feature flags

The compose stack starts Prometheus with
`--enable-feature=exemplar-storage,otlp-deltatocumulative,created-timestamp-zero-ingestion`
and `--web.enable-remote-write-receiver`. Pass the same to your own
Prometheus for the same behaviour:

* **`otlp-deltatocumulative`** converts delta sums and histograms to the
  cumulative series every rule and panel reads ([below](#delta-temporality)).
* **`created-timestamp-zero-ingestion`** writes a zero at each OTLP point's
  start time. A process that exports once (a CLI run, a batch job) then has
  two samples, `0` at its start and its count at exit, so `rate()` sees it.
  Measured on 2026-11-04: 69 one-shot `a2a` CLI runs, each its own
  `instance`, showed as 0.3 calls a second on the client dashboard; without
  the flag each series is a single sample and the rate panels stay empty
  (audit L11). Each run is still a new series; see
  [Dashboards](./dashboards.md#a2a-client).
* **`exemplar-storage`** keeps exemplars: the trace id of one measurement
  behind a point. Tempo's span metrics carry them (remote-written, hence
  `--web.enable-remote-write-receiver`), and Grafana links each to its trace.
  The SDK's own metrics carry none: OpenTelemetry Rust 0.33 records no
  exemplars.

## Delta temporality

`OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=delta` makes the exporter
send each export's change instead of the running total. Prometheus stores
cumulative series only: measured on Prometheus 3.15.1, a delta sum sent to
its OTLP receiver without `otlp-deltatocumulative` is refused with HTTP 500
and the log line `invalid temporality and type combination`, so the series
never appear and every shipped panel and rule is empty (audit C-21). With the
flag three delta points of 2 were stored as 2, 4, 6, and five CLI runs with
the variable set showed as five series. For the scrape path
(`otel-collector/config.yaml`), the Collector's `deltatocumulative` processor
does the same before its Prometheus exporter. Either converter keeps state
per series, so all of a series' points must reach the same one: one
Prometheus, or one Collector, per set of agents.

## What the names become

The dots are correct: OpenTelemetry names metrics this way
(`http.server.request.duration`), and the exporter translates `.` to `_`.
Counters gain `_total`; a histogram or gauge whose unit is `s` gains
`_seconds`; a brace-annotated unit (`{token}`, `{request}`) adds nothing; and
attribute dots become underscores (`rpc.system.name` → `rpc_system_name`). A
histogram is the usual `_bucket`, `_sum` and `_count` series under that name.

| Instrument | Prometheus name |
|---|---|
| `rpc.server.call.duration` | `rpc_server_call_duration_seconds` |
| `rpc.client.call.duration` | `rpc_client_call_duration_seconds` |
| `a2a.server.requests` | `a2a_server_requests_total` |
| `a2a.server.request.duration` | `a2a_server_request_duration_seconds` |
| `a2a.server.queue_depth` | `a2a_server_queue_depth` |
| `db.client.operation.duration` | `db_client_operation_duration_seconds` |
| `gen_ai.client.inference.usage.input_tokens` | `gen_ai_client_inference_usage_input_tokens_total` |
| `gen_ai.client.inference.operation.input_tokens` | `gen_ai_client_inference_operation_input_tokens` (no unit suffix) |

The [Metrics reference](./metrics.md#prometheus-names) has the name of every
instrument, and which were read from a live Prometheus and which follow from
the rule.
