<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Prometheus and Grafana for a2a-rust agents

Prometheus configuration, recording and alerting rules, an OpenTelemetry
Collector configuration and Grafana dashboards for the metrics a2a-rust
emits (the catalogue is in `book/src/deployment/observability.md`). Every
metric and label name below was read from a live Prometheus fed by an agent
built from this repository, not derived from the code, except the executor's
(`a2a_server_executor_duration_seconds` and its labels), which was added later
and follows the book's translation rule.

## The whole stack in one command

```sh
docker compose -f deploy/observability/docker-compose.yml up -d
```

starts a Collector, Prometheus, Alertmanager, Tempo (traces), Loki (logs) and
Grafana, with the dashboards, rules and data sources provisioned. Then run any
agent with

```sh
OTEL_SERVICE_NAME=my-agent OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 my-agent
```

and open Grafana on <http://localhost:3001> (no login: the anonymous user is
an admin, so keep it off any network you do not control). The dashboards are
in the folder "A2A"; traces and logs are under Explore, and a span opens the
log lines written inside it, a log line its trace. An agent in a container on
the stack's network (`a2a-observability_default`) uses
`http://otel-collector:4318`. Published ports, on 127.0.0.1 only: 3001
Grafana, 9090 Prometheus, 9093 Alertmanager, 4317/4318 the Collector, 3200
Tempo, 3100 Loki. Alerts reach Alertmanager, whose receiver **notifies nobody**
until you give it an integration (`alertmanager/alertmanager.yml`; the book's
*Alerts and runbooks* page). Every image is pinned by digest.

Metrics travel Collector → Prometheus's OTLP receiver, so `service.name` and
`service.instance.id` become `job` and `instance` exactly as when an agent
pushes to Prometheus itself, and nothing below changes. Run it for a laptop or
a test box; it keeps everything in local volumes and has no authentication.

### On a shared host

```sh
GRAFANA_ADMIN_PASSWORD=... docker compose \
  -f deploy/observability/docker-compose.yml \
  -f deploy/observability/docker-compose.prod.yml up -d
```

adds a Grafana login (no anonymous user; Compose refuses to start without
the password), publishes only Grafana and the Collector's OTLP ports, so
Prometheus, Tempo and Loki are reachable on the Compose network only, and
publishes those on every interface. TLS on the OTLP receivers and on
Grafana, authentication on the receivers for agents outside your network, and
backups are still yours to add; the file says how far it goes.

Both Collector configurations put `memory_limiter` first in every pipeline
(a burst is refused and the agent retries, rather than the Collector being
killed for memory; its limits are 80% and 20% of the container's memory
limit, 1 GiB in the compose file) and answer health checks on 13133 (`GET /`
is 200 once the pipelines run), for a Kubernetes liveness or readiness probe.
Both serve the Collector's own metrics on 8888, which Prometheus scrapes as
job `otelcol` and `rules/stack-alerts.yml` watches.

| Path | What it is |
|---|---|
| `prometheus/prometheus.yml` | Prometheus with its OTLP receiver: agents push metrics straight to it |
| `prometheus/prometheus-collector.yml` | Prometheus scraping an OpenTelemetry Collector instead (`honor_labels: true`, see the comment) |
| `prometheus/prometheus-compose.yml` | The compose stack's Prometheus: `prometheus.yml` plus Alertmanager and scrape jobs for the stack's components |
| `docker-compose.yml` | The stack above: Collector, Prometheus, Alertmanager, Tempo, Loki, Grafana |
| `alertmanager/alertmanager.yml` | Alertmanager: grouping, inhibit rules, one receiver with no integration |
| `otel-collector/config-compose.yaml` | The Collector of the compose stack: metrics to Prometheus over OTLP, traces to Tempo, logs to Loki |
| `tempo/tempo.yaml`, `loki/loki.yaml` | Tempo and Loki, single binary, local storage, OTLP in |
| `grafana/provisioning/traces-and-logs.yml` | Tempo and Loki data sources, linked both ways; set `TEMPO_URL` and `LOKI_URL` (the compose stack mounts it) |
| `docker-compose.prod.yml` | The stack closed down for a shared host: Grafana login, only Grafana and OTLP published, a memory limit on the Collector |
| `otel-collector/config.yaml` | The Collector on its own: OTLP in on 4317 (gRPC) and 4318 (HTTP), a Prometheus exporter on 8889; traces and logs go to its `debug` exporter, so add exporters for your trace and log backends |
| `prometheus/rules/a2a-recording.yml` | Recording rules: call rate, error ratio and latency quantiles per binding and method, unanswered requests, push-delivery outcomes, client-side calls |
| `prometheus/rules/a2a-alerts.yml` | Fourteen alerts on the agents; the book's *Alerts and runbooks* page has one section for each |
| `prometheus/rules/stack-alerts.yml` | Six alerts on the stack: the Collector down, dropping, refusing or queueing; Prometheus failing to evaluate rules or to reach Alertmanager |
| `prometheus/tests/*.test.yml` | `promtool` unit tests: each alert fires on the input it is for and not on a control (CI: `.github/workflows/observability.yml`) |
| `scripts/check_stack.py` | What promtool cannot check: the rules' caller-class pattern against the crates' error codes, image digests, the compose Prometheus file, and the generated Kubernetes manifests (`--write-kubernetes` regenerates `deploy/kubernetes/observability/`) |
| `grafana/dashboards/a2a-server.json`, `a2a-client.json`, `a2a-genai.json` | Three dashboards (server, client, and the tool loop's GenAI metrics), folder "A2A", data source uid `a2a-prometheus`; screenshots in `book/src/images/dashboards/` |
| `grafana/provisioning/` | Data source and dashboard provisioning; set `PROMETHEUS_URL` and `A2A_DASHBOARDS_DIR` (both are required: unset, Grafana substitutes an empty string and the data source or the dashboards are missing) |
| `scripts/drive.sh`, `scripts/webhook_sink.py` | Traffic for a live check: every binding, errors, streams, push deliveries to a webhook that answers and one that does not |
| `scripts/check_dashboards.py` | The live check: rules healthy and recording rules populated; every dashboard query returns data through Grafana's query API (or Prometheus's) |

## Pointing an agent at it

Every binary in this repository that serves or calls an agent, and any agent
that builds `server::otel::Telemetry`, reads the standard `OTEL_*` variables.
The exceptions (the official-TCK runner, which has no SDK dependency by
design; an MCP server over stdio; the benchmark harnesses; the code
generator) are listed with their reasons in `scripts/check_otel_wiring.py`,
which CI runs.

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

Set `service.instance.id` per replica. In Kubernetes `$(hostname)` is not a
shell: a manifest's `$(NAME)` names another variable in the same list, so take
the pod's name from the downward API and use that (the book's
[quick start](../../book/src/deployment/observability.md#quick-start-pick-your-deployment)
has the block). `service.name` becomes the `job` label and
`service.instance.id` the `instance` label. `Telemetry` sets a
random `service.instance.id` per process unless `OTEL_RESOURCE_ATTRIBUTES`
names one; set it yourself, as above, for a stable per-replica label.

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
- Direct OTLP: `check_dashboards.py --grafana http://127.0.0.1:3001
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

### The 0.16.0 stack (2026-11-04)

The compose stack as it stands, from this tree, with host ports moved (other
work on the machine needed 4317, 4318, 9090 and 3001), fed by `hello-agent`
and the `a2a` CLI built from the tree: three sends, a stream and a `GetTask`
for a task that does not exist per round, and every third round a one-shot CLI
run exporting under its own instance. Every service started; Prometheus
scraped `otelcol`, `alertmanager`, `tempo`, `loki` and itself, and had
Alertmanager as its target.

- `memory_limiter` configured itself from the 1 GiB limit: 819 MiB, spike
  204 MiB.
- The deliberate `-32001` was `a2a:rpc_server_caller_errors:rate5m` (0.48 a
  second, every `GetTask`), and the error ratio had no series: nothing failed.
- Tempo's span metrics and service graph reached Prometheus
  (`traces_service_graph_request_total{client="a2a-cli",server="hello-agent"}`),
  with exemplars labelled `traceID`; the data source links both `traceID` and
  `trace_id`.
- 69 one-shot CLI runs, 69 instances, each series `0` at its start and `1` at
  exit (`created-timestamp-zero-ingestion`); the client rate panel read 0.3
  calls a second.
- Five CLI runs exporting delta temporality appeared as five series
  (`otlp-deltatocumulative`). The control, a Prometheus 3.15.1 without the
  flag, answered a delta sum with HTTP 500 and stored nothing.
- Loki stopped for five minutes: the Loki exporter's queue grew from 105 to
  174, `otelcol_exporter_send_failed_log_records` appeared once its retries
  ran out (about four and a half minutes), and `OTelCollectorExportFailing`
  went pending. A first draft of `OTelCollectorRefusing` failed to evaluate
  (one selector over several names gave duplicate series);
  `PrometheusRuleEvaluationFailing` went pending on it, and the rule was
  rewritten. Then 42 rules, all healthy.
- Grafana 13.2.4's *Logs for this trace* (driven with Playwright) opened Loki
  with a query that filters on the `trace_id` structured metadata, and showed
  the trace's line; a bare line filter on the id finds none.
- `check_dashboards.py` on the server and client dashboards: every panel whose
  traffic was sent had data, the three added in 0.16.0 included; the empty
  ones were failures, push deliveries and the store, none of which this agent
  produced.

Not run: the production overlay (it changed only in ports), the GenAI
dashboard, and the Kubernetes manifests in a cluster.

### The GenAI dashboard and the task-store row (2026-10-24)

Measured on the same stack, with llama-server (llama.cpp `f5027fb`)
serving Qwen3 1.7B (Q4_K_M) on a 4-vCPU container. The feed was one
long-lived test process, not committed: it serves three tool-loop agents
over JSON-RPC with `serve_with_addr` and SQLite task stores (one whose
model answers, one whose model URL is unreachable, one whose model calls
time out after 300 ms), and asks each in turn as an A2A client, exporting
through `Telemetry` over OTLP. `scripts/drive.sh` ran beside it against
`tck/sut`.

- `check_dashboards.py`, same arguments: ok. 21 rules; 4 client, 21 GenAI
  and 17 server queries, every one returning data except "Persistence
  errors" (no store failed). "HTTP connections" had data this time, from
  the process served with `serve_with_addr`.
- The screenshots in the README and the book were taken at the end of a
  15-minute window of this traffic, in Grafana's light theme.

**A process that exits after one call cannot feed these panels.** Each
series a short-lived process exports has one or two samples, so `rate()`
over it tapers to nothing within the window, and a ratio over it is
undefined; the GenAI ratio panel stayed empty while the agents ran as
one-shot `examples/` binaries. Feed a check from a process that lives at
least one window, and confirm its series in Prometheus before reading the
panels (`count by (job, instance) ({__name__=~"gen_ai.*"})`).

The harness's model ran on four CPU cores shared with builds, so its calls
are slow: an agent call took 40 s to over 100 s. A quantile cannot read
above its histogram's top bucket. The server's p99 flattens at 100 s, the
top of the RPC conventions' buckets (0.1 ms to 100 s since 0.15; a first
capture, with the old 5 ms to 10 s set, pinned every quantile at 10 s), and
the GenAI model-call p99 at 81.92 s, the top of the GenAI conventions'
buckets. A deployment whose calls run past those limits sees the limit, not
the duration.

The live check found two defects before anything was committed, both fixed
here: the unanswered-requests rule first subtracted only responses, which
counts every error as unanswered (an error is not counted as a response);
and the Collector comment named port 4317 for an exporter that defaults to
HTTP/protobuf on 4318.

### TLS, proxies and the hardened stack (2026-10-28)

otelcol-contrib 0.163.2 with its OTLP receivers on TLS (a private CA; mutual
TLS with `client_ca_file`; a self-signed certificate marked CA and one marked
`CA:FALSE`; an expired certificate), and Squid 6.13 in front of it (open,
denying everything, and requiring a login), fed by `tck/sut` over OTLP/HTTP
and OTLP/gRPC. Every configuration that should work delivered traces,
metrics and logs; every one that should not ended with a warning naming the
cause and what to change. The texts behind those warnings are the inputs of the
hint tests (`crates/a2a-protocol-server/src/otel/telemetry/hints/tests.rs`): three of its four TLS hints had matched
error names rustls never prints, and never fired.

Then the compose stack with the hardened Collector configuration (the
memory limiter configured itself from the container, the health check
answered 200) and the production override: Prometheus, Tempo and Loki
refused connections from the host, Grafana answered 401 without a login and
with a wrong password and served the dashboards to `admin`, and an agent's
metrics were read back through Grafana's data source proxy. Not run: the
production override behind TLS, and either stack in Kubernetes.

