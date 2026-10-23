<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Prometheus and Grafana for a2a-rust agents

Prometheus configuration, recording and alerting rules, an OpenTelemetry
Collector configuration and Grafana dashboards for the metrics a2a-rust
emits (the catalogue is in `book/src/deployment/observability.md`). Every
metric and label name below was read from a live Prometheus fed by an agent
built from this repository, not derived from the code.

| Path | What it is |
|---|---|
| `prometheus/prometheus.yml` | Prometheus with its OTLP receiver: agents push metrics straight to it |
| `prometheus/prometheus-collector.yml` | Prometheus scraping an OpenTelemetry Collector instead (`honor_labels: true`, see the comment) |
| `otel-collector/config.yaml` | The Collector: OTLP in on 4317 (gRPC) and 4318 (HTTP), a Prometheus exporter on 8889 |
| `prometheus/rules/a2a-recording.yml` | Recording rules: call rate, error ratio and latency quantiles per binding and method, unanswered requests, push-delivery outcomes, client-side calls |
| `prometheus/rules/a2a-alerts.yml` | Eight alerts: error ratio, persistence errors, unanswered requests, event queues growing, push deliveries truncated or failing, non-streaming p99, an agent gone silent |
| `prometheus/tests/a2a-alerts.test.yml` | `promtool` unit tests: each alert fires on the input it is for and not on a control |
| `grafana/dashboards/a2a-server.json`, `a2a-client.json` | Two dashboards, folder "A2A", data source uid `a2a-prometheus` |
| `grafana/provisioning/` | Data source and dashboard provisioning; set `PROMETHEUS_URL` and `A2A_DASHBOARDS_DIR` |
| `scripts/drive.sh`, `scripts/webhook_sink.py` | Traffic for a live check: every binding, errors, streams, push deliveries to a webhook that answers and one that does not |
| `scripts/check_dashboards.py` | The live check: rules healthy and recording rules populated; every dashboard query returns data through Grafana's query API (or Prometheus's) |

## Pointing an agent at it

Every binary in this repository, and any agent that builds
`server::otel::Telemetry`, reads the standard `OTEL_*` variables.

Straight to Prometheus (`prometheus --web.enable-otlp-receiver`):

```sh
OTEL_SERVICE_NAME=my-agent \
OTEL_RESOURCE_ATTRIBUTES=service.instance.id=$(hostname) \
OTEL_EXPORTER_OTLP_ENDPOINT=http://prometheus:9090/api/v1/otlp \
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \
OTEL_TRACES_EXPORTER=none OTEL_LOGS_EXPORTER=none \
my-agent
```

Through the Collector (all three signals):

```sh
OTEL_SERVICE_NAME=my-agent \
OTEL_RESOURCE_ATTRIBUTES=service.instance.id=$(hostname) \
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
my-agent
```

Set `service.instance.id` per replica. `service.name` becomes the `job`
label and `service.instance.id` the `instance` label; the SDK does not set
an instance id itself, so two replicas without one write the same series.

## How this was tested

Measured 2026-10-23 on a 4-vCPU Linux container, with Prometheus 3.15.1,
Grafana OSS 13.2.4 and otelcol-contrib 0.163.2. The Prometheus and Grafana
tarballs' SHA-256 matched their release checksums; the Collector's release
checksum file could not be fetched from the session, so that binary was
used unverified, as a local test tool only. The agents were this
repository's `tck/sut` (`a2a-tck-sut`) and the `a2a` CLI at commit
`7a73bdc`, driven by `scripts/drive.sh`.

- `promtool check config` on both Prometheus files and `promtool check
  rules`: SUCCESS (13 recording rules, 8 alerts).
- `promtool test rules prometheus/tests/a2a-alerts.test.yml`: SUCCESS. Each
  alert's expression was then broken in turn (a threshold, the outcome
  label, a range), and the tests failed every time (8 of 8 caught); a
  ninth case proves the unanswered-requests rule subtracts errors.
- Direct OTLP: `check_dashboards.py --grafana http://127.0.0.1:3000
  --prometheus http://127.0.0.1:9090 grafana/dashboards --expect-empty
  "Persistence errors" "HTTP connections"`: ok. All 21 rules healthy, every
  recording rule with data, both dashboards provisioned in folder "A2A",
  and all 19 panel queries returning data through Grafana's `/api/ds/query`
  except the two named panels.
- Through the Collector: the same check against the second Prometheus
  (`--prometheus` only): ok, with the same two panels empty. Without
  `honor_labels` the same series read `job="otel-collector"` with the agent
  in `exported_job` (checked on a third Prometheus).
- Alerts fired live on the generated traffic: `A2AServerErrorRatioHigh`
  (11% of calls were a deliberate task-not-found), and `A2AAgentSilent`
  for CLI runs made before they were given an instance id.

The two panels left empty, and why: no store failed, so there were no
persistence errors to count; and `tck/sut` serves HTTP with its own loop,
while the connection-pool gauges are reported only by the SDK's `serve`,
`serve_with_addr` and `Server::serve_with_shutdown` (the book's catalogue
says so), so that panel needs an agent served that way.

The live check found two defects before anything was committed, both fixed
here: the unanswered-requests rule first subtracted only responses, which
counts every error as unanswered (an error is not counted as a response);
and the Collector comment named port 4317 for an exporter that defaults to
HTTP/protobuf on 4318.
