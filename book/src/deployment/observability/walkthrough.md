<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Walkthrough: the local stack

From nothing to a dashboard with data, a trace in Tempo and the log lines
written inside it, on one machine. It uses the Compose file in
`deploy/observability/`, this repository's TCK agent (`tck/sut`, which serves
JSON-RPC, HTTP+JSON, gRPC and WebSocket from one process with push
notifications enabled), the `a2a` CLI, and the traffic script the dashboards
were checked with. You need Docker with Compose and a checkout of this
repository.

## 1. Start the stack

```bash
docker compose -f deploy/observability/docker-compose.yml up -d
```

This starts six services on the network `a2a-observability_default`, after a
one-shot `volume-init` that gives the Collector's and Tempo's volumes to the
user they run as. Every port is published on `127.0.0.1` only, and every
image is pinned by digest as well as tag:

| Service | Image | Published port | Role |
|---|---|---|---|
| `otel-collector` | `otel/opentelemetry-collector-contrib:0.163.2` | 4317 (gRPC), 4318 (HTTP) | Receives OTLP; metrics to Prometheus, traces to Tempo, logs to Loki; 1 GiB memory limit; queues on disk; its own metrics on 8888 |
| `prometheus` | `prom/prometheus:v3.15.1` | 9090 | OTLP and remote-write receivers on, the rules loaded, 7 days' or 5 GB retention, delta conversion, start-time zeros and exemplars |
| `alertmanager` | `prom/alertmanager:v0.34.2` | 9093 | Receives the alerts; notifies nobody until you add a receiver ([Alerts](./alerts.md#where-alerts-go)) |
| `tempo` | `grafana/tempo:2.9.0` | 3200 | Traces, kept 24 hours; span metrics and the service graph to Prometheus |
| `loki` | `grafana/loki:3.7.9` | 3100 | Logs, kept 7 days |
| `grafana` | `grafana/grafana:13.2.4` | 3001 (host), 3000 (container) | Dashboards in folder "A2A"; Tempo and Loki under Explore; alerts under Alerting |

Grafana has no login: the anonymous user is an admin, so keep it off any
network you do not control (see [On a shared host](#on-a-shared-host)).
Everything is kept in local volumes, Grafana's own state included; the
provisioned dashboards cannot be saved over from the UI ("Save as" makes a
copy).

## 2. Run an agent pointed at it

Build the agent and the CLI, then start the agent with the standard
variables. `SUT_HOST` is its HTTP address (gRPC listens one port below), and
`SUT_WS_HOST` turns on its WebSocket listener:

```bash
cargo build -p a2a-tck-sut -p a2a-cli

OTEL_SERVICE_NAME=tck-sut \
OTEL_RESOURCE_ATTRIBUTES=service.instance.id=local-1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
SUT_HOST=127.0.0.1:9999 SUT_WS_HOST=127.0.0.1:9997 \
target/debug/a2a-tck-sut
```

Its first line on standard error is the startup line: for each of traces,
metrics and logs, `otlp http/protobuf` and the endpoint, `http://localhost:4318`
with `/v1/traces`, `/v1/metrics` or `/v1/logs` appended. If it says `nothing
is exported`, the endpoint variable did not reach the process.
Any agent built on this SDK that builds `Telemetry` the
same way works here: an agent in a container on the stack's network uses
`http://otel-collector:4318` instead.

## 3. Drive traffic

`scripts/drive.sh` runs the `a2a` CLI against every binding in a loop — sends,
streams, a `GetTask` for a task that does not exist (a deliberate failure),
and `ListTasks` — and, with `WEBHOOK_OK` and `WEBHOOK_BAD` set, JSON-RPC
messages carrying an inline push config to a receiver that answers and to one
that does not, so push deliveries are both delivered and failed.
`scripts/webhook_sink.py` is the receiver that answers:

```bash
python3 deploy/observability/scripts/webhook_sink.py 8099 &

WEBHOOK_OK=http://127.0.0.1:8099/hook WEBHOOK_BAD=http://127.0.0.1:9/hook \
deploy/observability/scripts/drive.sh target/debug/a2a \
  http://127.0.0.1:9999 127.0.0.1:9998 ws://127.0.0.1:9997 900
```

The arguments are the CLI, the HTTP URL (JSON-RPC and HTTP+JSON), the gRPC
address, the WebSocket URL and how many seconds to run. Fifteen minutes fills
the dashboards' five-minute rates comfortably. The TCK agent allows private
webhook URLs, which the push deliveries to `127.0.0.1` need.

The CLI builds `Telemetry` too, so exporting `OTEL_EXPORTER_OTLP_ENDPOINT` in
the shell that runs `drive.sh` feeds the **A2A client** dashboard from its
calls. Each CLI run is a short-lived process that exports once, at exit, and
each run without a `service.instance.id` is a new instance, a series of its
own. The compose Prometheus writes a zero at each series' start time, so these
runs do show on the rate panels; on a Prometheus without that flag they show
only on *Calls in the range by processes that started in it* ([A2A
client](./dashboards.md#a2a-client)).

## 4. Read a dashboard

Open `http://localhost:3001`, then **Dashboards → A2A → A2A server**. Within a
few minutes the *Calls* row shows calls per second on all four bindings, and
*Caller errors* a steady rate on every binding: the deliberate task-not-found,
`-32001` over JSON-RPC, `404` over HTTP+JSON, `NOT_FOUND` over gRPC. It is the
caller's mistake, answered correctly, so *Failed calls* stays empty: until
0.16.0 the JSON-RPC half of it counted as a failure there, and paged
([what counts as a failure](./alerts.md#what-counts-as-a-failure)).
*Server span duration* has points with diamonds on them: exemplars, each one
span; click one to open its trace. *Push deliveries, by
outcome* shows `delivered` and failed outcomes side by side. *Persistence
errors* stays empty, because no store fails, and *HTTP connections* stays empty
for this agent, which serves HTTP with its own loop rather than the SDK's
`serve`. [Dashboards, alerts and recording rules](./dashboards.md) describes
every panel.

To see the recording rules behind the panels, open Prometheus on
`http://localhost:9090` and query `a2a:rpc_server_calls:rate5m`; **Alerts**
lists the twenty alerts, and Alertmanager on `http://localhost:9093` (or
Grafana's **Alerting**) shows any that fire. Before 0.16.0
`A2AServerErrorRatioHigh` fired on this traffic: 11% of calls were the
deliberate task-not-found. It no longer does.

## 5. Find a trace

In Grafana, **Explore → Tempo**, search by service name `tck-sut`. Each call is
a trace whose root is the `SERVER` span `lf.a2a.v1.A2AService/{Method}`, with
`invoke_agent` and the work the call spawned beneath it; a call made by the
exported CLI has the CLI's `CLIENT` span above it, in the same trace.
[Spans reference](./spans.md) lists what each span carries.

## 6. Follow a trace to its logs

On a span in Tempo, **Links → Logs for this trace** opens Loki filtered by the
span's trace id, five minutes either side: the log lines the SDK wrote inside
that call (checked on 2026-11-04 with Grafana 13.2.4: the query filters on
Loki's structured metadata, where the trace id is). The same menu has the
span's rate, error rate and p95 from Tempo's span metrics, and the **Service
Graph** tab draws the calls between agents. From
the other side, **Explore → Loki**, select the agent's lines by its
`service_name` label; a line with a `trace_id` links back to its trace in
Tempo. [Logs](./logs.md#logs-in-loki-linked-to-traces) explains the
provisioning that makes both links.

## 7. Stop

Stop the agent with Ctrl-C (`shutdown_signal()` flushes before it exits), then:

```bash
docker compose -f deploy/observability/docker-compose.yml down
```

Add `-v` to delete the volumes and the data in them.

## On a shared host

```bash
GRAFANA_ADMIN_PASSWORD=... docker compose \
  -f deploy/observability/docker-compose.yml \
  -f deploy/observability/docker-compose.prod.yml up -d
```

adds a Grafana login (no anonymous user, no sign-up; Compose refuses to start
without the password), publishes only Grafana and the Collector's OTLP ports,
on every interface, so Prometheus, Alertmanager, Tempo and Loki are reachable
on the Compose network only. The Collector's 1 GiB memory limit, from which
its `memory_limiter` takes its limits, is in the base file. TLS on the OTLP receivers and on Grafana, authentication on
the receivers for agents outside your network, and backups are still yours to
add. Both Collector configurations answer health checks on 13133 (`GET /` is
200 once the pipelines run), for a liveness or readiness probe.

That override was run on 2026-10-28: Prometheus, Tempo and Loki refused
connections from the host, Grafana answered 401 without a login and with a
wrong password and served the dashboards to `admin`, and an agent's metrics
were read back through Grafana's data source proxy. Not run: the production
override behind TLS, and either stack in Kubernetes ([The stack on
Kubernetes](./kubernetes.md) has the generated manifests).
