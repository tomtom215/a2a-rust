<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0039: What the shipped alerts count as failure, how the stack watches itself, and its Kubernetes form

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0 (the maintainer may veto in review). Built and
run: the rules under `promtool test rules`, the compose stack live on
2026-11-04. Kubernetes manifests generated and checked against their sources,
not applied to a cluster (none runs on the machine this was built on).
**Author:** Tom F.

---

## Context

The observability stack in `deploy/observability/` was read by audit C
(2026-10-29) and triaged on 2026-11-01. Its open rows each need a choice
that the code cannot make for itself:

* **NEW-c-1.** The instrumentation follows the OpenTelemetry RPC
  conventions, which make every JSON-RPC error code an error and only a
  gRPC/HTTP server-side status one. The error-ratio alert used that as the
  definition of the server failing, so a client polling for tasks the
  retention reaper removed paged the on-call over JSON-RPC and not over
  HTTP+JSON or gRPC.
* **C-14.** The p99 alert's 2.5 s threshold sat inside the (1 s, 5 s] bucket,
  so the quantile it compared was a straight-line guess (98 calls under 1 s
  and 2 at 1.1 s read as 3.0 s). Alerts had no receiver. Store retries,
  executor failures, model-call failures and client failures had no alert.
  One failed task write paged `critical` for five minutes and resolved.
* **C-15.** Nothing watched the Collector, and in the base compose file
  `memory_limiter` had no container limit to take its share of.
* **C-19, C-21, C-05, L11.** Retention, persistence, delta temporality,
  exemplars and one-shot processes, each with a documentation-only and a
  configuration answer.
* **C-22.** Kubernetes users had a Deployment with a commented OTEL block and
  nothing else.

The maintainer's instruction for 0.16.0 is the best engineering option for
each, not a deferral.

## Decisions

### 1. A failure is the server's failure (NEW-c-1)

The rules, not the instrumentation, change: `error.type` stays as the
conventions define it (a dashboard that wants every JSON-RPC error still has
it). The recording rules leave out, besides `cancelled`, every status that
A2A §5.4 maps to a 4xx: the JSON-RPC codes other than -32603 and -32006, any
HTTP 4xx, and every gRPC code outside the six server-side failures. One
pattern, used in three rules (server errors, a new
`a2a:rpc_server_caller_errors:rate5m`, client errors);
`deploy/observability/scripts/check_stack.py` derives the set from
`ErrorCode::http_status`, `grpc_server_error` and the gRPC code list in the
source and fails when the rules differ, so a code added to A2A must be
placed on one side. A code not in the pattern counts as failed.

The client error ratio applies the same rule, plus every failure with no
status (timeout, transport, a response that does not decode). Its dashboard
panel "Failures by kind" still shows every kind.

Options weighed: (a) change the SDK to set `error.type` only for
server-class JSON-RPC codes: deviates from the conventions on the wire of
telemetry other tools read. (b) Rules only, as chosen. (c) Alert on
`a2a_server_errors_total` by kind: a second definition of failure beside
the RPC metric.

Known gap: on JSON-RPC an overload and a rate-limit refusal are both -32603
(A2A has no throttling code), so both count as the server failing; on gRPC
they are `RESOURCE_EXHAUSTED`, which the gRPC rule does not count.

### 2. Latency is alerted on a bucket bound, not a quantile (C-14 a)

`A2AUnaryLatencyHigh` now fires when more than 1% of non-streaming calls of a
method took longer than 5 s (`a2a:rpc_server_unary_over_5s:ratio_rate5m`,
read from the `le="5"` bucket, exact), with a 0.1 calls/s floor. The p50,
p95 and p99 rules stay for the dashboard, whose description says they are
interpolated. Not chosen: adding a 2.5 s boundary to the SDK's buckets (it
departs from the conventions' advisory buckets the code cites, and adds a
series per histogram); keeping p99 with a threshold on a bound (still an
interpolation).

### 3. One failed write is a warning that stays; sustained failures page (C-14 d)

`A2APersistenceErrors` (warning) fires for an hour after any failed task
write; `A2APersistenceFailing` (critical) when writes fail in every
five-minute window for 10 minutes. The warning's expression has a second arm
for a counter whose first sample is already 1: an OpenTelemetry counter with
a given attribute set is exported only once it counts something, so
`increase()` cannot see its first failure, and the old rule never fired on a
single failure at all (shown by `promtool` against the old rules). Prometheus's
`created-timestamp-zero-ingestion` (enabled in compose) writes a zero at the
point's start time and closes this for short-lived processes; for a
long-running process the start time is the process's, which is older than
Prometheus accepts for a new series (conjectured, not measured), so the rule
does not rely on it.

### 4. Alerts reach Alertmanager, which notifies nobody until told (C-14 b)

The compose stack runs Alertmanager with one receiver that has no
integration, inhibit rules (sustained persistence failures over the warning,
a silent job over lost instances, the Collector down over every silent job),
and a Grafana data source for it. A shipped receiver that pages someone
cannot be right for every deployment; the files and the book say plainly
that until a receiver is added, alerts are visible and notify nobody.

### 5. The stack watches itself (C-15)

The Collector serves its own metrics on 8888, Prometheus scrapes it and the
other components, and `rules/stack-alerts.yml` adds `OTelCollectorDown`,
`OTelCollectorExportFailing`, `OTelCollectorRefusing`,
`OTelCollectorQueueFilling`, `PrometheusRuleEvaluationFailing` and
`PrometheusNotificationsFailing`. The base compose file gives the Collector a
1 GiB limit (measured: `memory_limiter` configured itself at 819 MiB) and
keeps exporter queues on disk (`file_storage`).

### 6. Delta temporality is converted, not refused (C-21)

Measured on Prometheus 3.15.1: a delta sum without the feature is refused
with HTTP 500 ("invalid temporality and type combination") and never
appears. The compose Prometheus runs with `otlp-deltatocumulative`, which
converts it; the standalone Collector configuration has a
`deltatocumulative` processor for the scrape path. Both keep per-series
state, so one converter must see all of a series' points; the docs say so.

### 7. Exemplars from traces (C-05, the stack's half)

Tempo's metrics generator writes span metrics and the service graph, with
exemplars, to Prometheus (`--web.enable-remote-write-receiver`,
`exemplar-storage`); the Prometheus data source links exemplars (`traceID`
from Tempo, `trace_id` from OTLP) to Tempo, the Tempo data source queries
span metrics from a span, and the server dashboard has a span-duration panel
with exemplars. The SDK's own metrics carry none (OpenTelemetry Rust 0.33);
that half is upstream and another work package's.

### 8. One-shot processes (L11)

Option 1 (docs) and option 2 (a panel) of the triage, plus the Prometheus
`created-timestamp-zero-ingestion` feature, which was not among the options
and does more: measured live, 69 one-shot CLI runs each had two samples
(0 at start, 1 at export) and the rate panels showed 0.3 calls/s. The new
client panel ("Calls in the range by processes that started in it") counts
them on a Prometheus without the feature. Not built: option 3 (a Collector
pipeline folding runs into one series per job), which adds Collector state
for a case the feature already covers. The per-run series churn (a new
`instance` per run) remains; the book says to set a stable
`service.instance.id` for batch jobs only with the feature on.

### 9. Retention and persistence (C-19)

Prometheus retention moves from the deprecated flag to the configuration
file with a size bound (7 d or 5 GB); Loki's compactor deletes after 7 d;
Grafana has a volume and its provisioned dashboards cannot be saved over
from the UI; every image is pinned by digest (checked by `check_stack.py`);
Tempo and the Collector run as their image's user with volumes chowned by a
one-shot service; base ports are bound to 127.0.0.1, the production overlay
publishes Grafana and OTLP only.

### 10. Kubernetes: generated objects, not a chart (C-22)

`deploy/kubernetes/observability/` holds a `PrometheusRule` with every rule
group and one Grafana dashboard ConfigMap per dashboard (sidecar label
`grafana_dashboard: "1"`, folder annotation `A2A`), generated by
`check_stack.py --write-kubernetes`, with a kustomization; CI fails when they
differ from the sources. The book page says how to load them into
kube-prometheus-stack and what Prometheus flags and OTLP settings the rules
assume. Not built: a Helm chart for the Collector and the stack (a second
deployment surface to maintain; the upstream charts exist) and the
deploy-agent example's OTEL block (another package's file).

## Consequences

* Breaking for anyone who built on the old rule semantics:
  `a2a:rpc_server_errors:rate5m`, `a2a:rpc_server_error_ratio:rate5m` and
  `a2a:rpc_client_error_ratio:rate5m` no longer count caller mistakes;
  `A2AUnaryLatencyHigh` has a new expression; `A2APersistenceErrors` is a
  warning and its page is `A2APersistenceFailing`. Every alert has a
  `runbook_url` annotation.
* `promtool check rules` and `promtool test rules` run in CI
  (`.github/workflows/observability.yml`), with `check_stack.py`.
* The compose stack now needs three Prometheus feature flags; a user's own
  Prometheus without them loses the delta conversion, the zero samples and
  the exemplars, and the book lists the flags.
