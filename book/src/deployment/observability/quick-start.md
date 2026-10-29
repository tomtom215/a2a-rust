<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Quick start by environment

This page gets an agent's telemetry to where you can read it, for each place
an agent runs. The code is the same everywhere — the `hello-agent` block in
the [chapter's quick start](../observability.md#quick-start-pick-your-deployment):
`Telemetry::builder().with_export_where_configured().build()`, then
`install_subscriber()`, then serve with `shutdown_signal()`. Only the
environment changes.

## Your machine, no collector

Set nothing: nothing is exported, and the line on standard error says so. To
see the telemetry anyway:

```bash
OTEL_TRACES_EXPORTER=console OTEL_METRICS_EXPORTER=console OTEL_LOGS_EXPORTER=console cargo run -p hello-agent
```

prints it on standard output. [Debugging telemetry](./debugging.md#seeing-it-with-no-collector)
shows what those lines look like.

## The whole stack on your machine

```bash
docker compose -f deploy/observability/docker-compose.yml up -d
```

starts a Collector, Prometheus, Tempo, Loki and Grafana with this repository's
dashboards; then set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318` and
open Grafana on `http://localhost:3000`
([`deploy/observability`](https://github.com/tomtom215/a2a-rust/tree/main/deploy/observability)).
[Walkthrough: the local stack](./walkthrough.md) takes it step by step, to a
dashboard, a trace and its logs.

## A collector on your machine

`OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318`. 4318 is OTLP/HTTP, the
default protocol. The Collector's gRPC receiver is 4317 and also needs
`OTEL_EXPORTER_OTLP_PROTOCOL=grpc`; the wrong pairing is reported with a hint.

## Docker

`localhost` inside a container is the container. Name the collector by its
service name on a shared network (`http://otel-collector:4318` on the compose
stack's network, `a2a-observability_default`) or by an address the container
can reach. A collector on the Docker host is `http://host.docker.internal:4318`
on Docker Desktop; on Linux, run the container with
`--add-host=host.docker.internal:host-gateway` for the same name.

## Kubernetes

Name the collector's Service, and give each pod its own `service.instance.id`,
which is what tells replicas apart (Prometheus's `instance` label). Without it
each process invents a random one, and every restart is a new series.

```yaml
env:
  - name: POD_NAME
    valueFrom: {fieldRef: {fieldPath: metadata.name}}
  - name: POD_NAMESPACE
    valueFrom: {fieldRef: {fieldPath: metadata.namespace}}
  - name: OTEL_EXPORTER_OTLP_ENDPOINT
    value: "http://otel-collector.observability.svc.cluster.local:4318"
  - name: OTEL_RESOURCE_ATTRIBUTES
    value: "service.instance.id=$(POD_NAME),k8s.pod.name=$(POD_NAME),k8s.namespace.name=$(POD_NAMESPACE)"
```

Kubernetes replaces `$(POD_NAME)`, and only with a variable defined earlier in
the list. The SDK detects no `k8s.*` attribute itself: `host.name` is the
container's hostname (a pod's name, unless the manifest sets another) and
`process.pid` is 1 when the binary is the entry point (see
[Resource attributes](./resource.md)). Stop the pod with `shutdown_signal()`,
as [below](#stopping). `examples/deploy-agent/deployment.yaml` carries this
block, commented out.

## HTTPS

`OTEL_EXPORTER_OTLP_ENDPOINT=https://collector.example:4318`. A certificate
from a public CA needs nothing more: the exporter trusts Mozilla's roots and
never reads the operating system's store, so a CA added to `/etc/ssl/certs`
changes nothing. For a private CA, mount its PEM file and name it:
`OTEL_EXPORTER_OTLP_CERTIFICATE=/etc/otel/ca.pem` (trusted in addition to the
Mozilla roots). A collector that requires a client certificate takes
`OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` and `OTEL_EXPORTER_OTLP_CLIENT_KEY`,
also PEM files. [TLS, proxies and compression](./tls-proxies.md#tls) has the
rest: the errors each mistake produces, self-signed certificates, and an
endpoint by IP address.

## Behind a proxy

`HTTPS_PROXY`, `HTTP_PROXY` or `ALL_PROXY` (an `http://` proxy, in upper or
lower case) send the export through it, and the startup line says `via proxy`.
Exempt a collector inside the cluster with `NO_PROXY`, because a proxy cannot
reach it — `.cluster.local` or `.svc.cluster.local`, not `.svc`.
[TLS, proxies and compression](./tls-proxies.md#proxies) has the matching
rules, which entries exempt an in-cluster collector, and proxy logins.

## Prometheus scraping the pod, no collector

Build with the `otel-prometheus` feature (off by default, and `deploy-agent` is
built without it), set `OTEL_METRICS_EXPORTER=prometheus`, and set
`OTEL_EXPORTER_PROMETHEUS_HOST=0.0.0.0` so a scraper in another pod can
connect: the default is `localhost`, port 9464. It is plain HTTP with no
authentication. Without the feature the value is an error naming it, and the
process does not start. [Prometheus](./prometheus.md) compares this with the
other ways in.

## Stopping

`shutdown_signal()` resolves on SIGTERM, which is what Kubernetes and Docker
send, and on Ctrl-C. A server that waits on `tokio::signal::ctrl_c()` alone is
ended by SIGTERM without a flush: measured on `hello-agent`, the collector then
received no spans and no metrics. The flush that follows costs what the
collector costs: on `hello-agent` the process exited 0.1 s after SIGTERM to a
collector that answers, 1.8 s to a port that refuses, and 15.0 s to one that
accepts the connection and never answers (each of the three providers waits up
to 5 s); `OTEL_EXPORTER_OTLP_TIMEOUT=2000` made the last 4.1 s (one run each,
debug build). Count that in a pod's `terminationGracePeriodSeconds`.

## It does not work

Read the line printed at startup, then the warning that follows a lost export;
each says what it knows. See [Debugging telemetry](./debugging.md) and
[Troubleshooting](../troubleshooting.md#telemetry-is-configured-and-nothing-reaches-the-collector).

## What these were run against

On 2026-10-28, against real backends: the compose stack above (OpenTelemetry
Collector 0.163.2, Prometheus 3.15.1, Tempo 2.9.0, Loki 3.7.9, Grafana 13.2.4),
fed by `deploy-agent`, `tck/sut` and `live-model` over OTLP/HTTP and OTLP/gRPC,
by `deploy-agent` straight to Prometheus's OTLP receiver, and to the console
exporter; then by the `deploy-agent` image in Docker, on the stack's network.
Every dashboard panel returned data (`check_dashboards.py`), a trace opened in
Tempo and a log line linked to it, and logs carried `trace_id` and `span_id`.
Before that, `hello-agent` and the `a2a` CLI, on one Linux machine on
2026-10-26, against stand-in receivers on loopback (a small Python OTLP/HTTP
receiver with TLS and client-certificate options, and this repository's own
`hyper` and `tonic` test collectors and forward proxy).

Not run: Kubernetes (the manifest was parsed as YAML and its `$(NAME)`
references checked, nothing more; a cluster could not be started where this
was tested), IPv6 on the wire (the host had no IPv6 in its kernel; the
endpoint and `NO_PROXY` handling of IPv6 addresses is unit-tested), and an
endpoint with a public CA. A gRPC-only receiver was imitated by a socket that
closes the connection, and a real one may answer differently, though the hint
depends only on the port and on the failure carrying no HTTP status.

The TLS and proxy cases were run against a real collector and a real proxy;
[TLS, proxies and compression](./tls-proxies.md#how-this-was-tested) lists them.
