<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# The stack on Kubernetes

The compose stack (`deploy/observability/docker-compose.yml`) is for a laptop
or a test box. On Kubernetes most clusters already run Prometheus through the
Prometheus Operator and Grafana with its dashboard sidecar (the
kube-prometheus-stack Helm chart has both), and an OpenTelemetry Collector
from its own chart or operator. So this repository does not ship a chart of
its own; it ships the a2a-rust parts in the form those tools load:

| File | What it is |
|---|---|
| `deploy/kubernetes/observability/prometheusrule.yaml` | A `PrometheusRule` (`monitoring.coreos.com/v1`) with every recording rule and alert of `deploy/observability/prometheus/rules/` |
| `deploy/kubernetes/observability/grafana-dashboards.yaml` | One ConfigMap per dashboard, labelled `grafana_dashboard: "1"` and annotated `grafana_folder: A2A` |
| `deploy/kubernetes/observability/kustomization.yaml` | Both, for `kubectl apply -k` |

The first two are generated from the compose stack's files by
`deploy/observability/scripts/check_stack.py --write-kubernetes`, and CI fails
when they differ from their sources, so they cannot drift. Edit the rules or
dashboards under `deploy/observability/`, then regenerate.

```bash
kubectl apply -k deploy/kubernetes/observability -n monitoring
```

Not run here: these manifests were generated and checked against their
sources, not applied to a cluster (Kubernetes does not run on the machine
they were built on).

## What your cluster needs to match

* **Rule selection.** The Prometheus Operator loads a `PrometheusRule` only
  if its labels match the Prometheus object's `ruleSelector`.
  kube-prometheus-stack selects `release: <release name>` by default: add
  that label (a kustomize `labels` entry, or `kubectl label`), or set
  `prometheus.prometheusSpec.ruleSelectorNilUsesHelmValues: false`.
* **Dashboards.** The Grafana sidecar loads ConfigMaps labelled
  `grafana_dashboard` (kube-prometheus-stack's default label) from the
  namespaces it watches; the folder comes from the `grafana_folder`
  annotation when `sidecar.dashboards.folderAnnotation: grafana_folder` is
  set. The dashboards query the data source uid `a2a-prometheus`: provision
  your Prometheus data source with that uid (kube-prometheus-stack:
  `grafana.sidecar.datasources.uid`, or an extra data source).
* **The names.** The rules read the names Prometheus's OTLP receiver gives
  (`translation_strategy: UnderscoreEscapingWithSuffixes`), with `job` and
  `instance` from `service.name` and `service.instance.id`. On the Operator:
  `enableOTLPReceiver: true` and `otlp.translationStrategy` on the Prometheus
  object, or a Collector whose Prometheus exporter uses the same strategy
  ([Prometheus](./prometheus.md#what-the-names-become)).
* **Feature flags.** `enableFeatures: [otlp-deltatocumulative,
  created-timestamp-zero-ingestion, exemplar-storage]` gives what the compose
  stack has: agents exporting delta temporality still show, a process that
  exported once has a rate, and exemplars are kept
  ([Prometheus](./prometheus.md#the-prometheus-feature-flags)).
* **The stack alerts.** `OTelCollector*` read the Collector's own telemetry
  under `job="otelcol"`. Scrape the Collector's `:8888` (a `PodMonitor` or
  `ServiceMonitor` whose job label is `otelcol`, or change the selector), and
  set `service.telemetry.metrics` as in
  `deploy/observability/otel-collector/config-compose.yaml`.
* **Alertmanager.** The Operator's Alertmanager routes by its own
  configuration; the inhibit rules in
  `deploy/observability/alertmanager/alertmanager.yml` are worth copying.

## The agents

Each pod sets `OTEL_EXPORTER_OTLP_ENDPOINT` to the Collector's service and a
stable `service.instance.id` from the downward API (`$(hostname)` is not a
shell in a manifest); the [quick start](./quick-start.md) has the block, and
`examples/deploy-agent/deployment.yaml` carries it commented out.
