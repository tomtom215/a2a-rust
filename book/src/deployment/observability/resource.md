<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Resource attributes

Every export carries a *resource*: the attributes that say which process sent
it. This page lists what `Telemetry` puts there, how to set the two that
matter most — the service's name and the instance's identity — and what they
become in Prometheus.

## What the resource says

| Attribute | Where it comes from |
|---|---|
| `service.name` | `OTEL_SERVICE_NAME`, else `service.name` in `OTEL_RESOURCE_ATTRIBUTES`, else `with_default_service_name`, else the OpenTelemetry SDK's `unknown_service` |
| `service.instance.id` | `OTEL_RESOURCE_ATTRIBUTES` when it names one, else a random UUID per process |
| `telemetry.sdk.*` | The OpenTelemetry SDK |
| `host.name` | `HOSTNAME` or `COMPUTERNAME`, else the kernel's hostname file on Linux |
| `host.arch`, `os.type` | The build target |
| `process.pid`, `process.executable.name` | The running process |
| anything else | `OTEL_RESOURCE_ATTRIBUTES` |

Each detected attribute (`host.*`, `os.type`, `process.*`) is set only if
`OTEL_RESOURCE_ATTRIBUTES` does not name it (on `hello-agent`,
`host.name=from-env` won). This crate forbids `unsafe`, which
`gethostname(2)` needs, so a macOS process that does not export `HOSTNAME`
carries no `host.name` rather than a wrong one. In a container these are the
container's hostname and the container's process ids, and no `k8s.*`
attribute is detected: see the Kubernetes recipe in the
[quick start](./quick-start.md#kubernetes).

These identify the machine and the process to whoever receives the export;
`TelemetryBuilder::without_resource_detection()` leaves them out.
`service.name`, `service.instance.id` and `telemetry.sdk.*` stay.

In `OTEL_RESOURCE_ATTRIBUTES`, `,` and `=` inside a value are written
percent-encoded (`deployment.environment.name=prod%2Cwest` is `prod,west`) and
decoded; an entry that is not valid percent-encoding is kept as written and
reported at startup.

## One identity per replica

`service.instance.id` is what tells replicas of one service apart. Without
one set, each process invents a random UUID, so two replicas never write the
same series — but every restart is a new series too, and an alert keyed on the
instance (`A2AAgentSilent`) fires for each one a rollout retires. Set it to
something stable per replica:

```bash
OTEL_SERVICE_NAME=my-agent \
OTEL_RESOURCE_ATTRIBUTES=service.instance.id=$(hostname) \
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4318 \
my-agent
```

In Kubernetes `$(hostname)` is not a shell: a manifest's `$(NAME)` names
another variable in the same list, so take the pod's name from the downward
API ([the manifest block](./quick-start.md#kubernetes)).

## What they become in Prometheus

Prometheus, whether it receives OTLP itself or scrapes a Collector with
`honor_labels: true`, turns `service.name` into the `job` label and
`service.instance.id` into `instance`. Every series also carries
`otel_scope_name`: `a2a.server` for the server's instruments (the default
meter name, changeable with `OtelMetricsBuilder::meter_name`) and `a2a.client`
for the client's. Alongside them the exporter emits `target_info`, a gauge
whose labels are the rest of the resource; its `service_name` label is
`OTEL_SERVICE_NAME` (or `service.name` in `OTEL_RESOURCE_ATTRIBUTES`) when
set, otherwise the name given to `Telemetry`'s `with_default_service_name` (or
to `init_otlp_pipeline`). The shipped `A2AAgentSilent` alert is built on
`target_info`, because an agent that pushes has no `up` series.

Other resource attributes are not labels on every series by default.
`deploy/observability/prometheus/prometheus.yml` promotes two
(`promote_resource_attributes: [service.version, deployment.environment.name]`)
so dashboards can group by them; [Prometheus](./prometheus.md#the-prometheus-configuration)
explains that file.
