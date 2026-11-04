<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Alerts and runbooks

`deploy/observability/prometheus/rules/` ships 20 alerts: 14 on the agents
(`a2a-alerts.yml`) and 6 on the observability stack itself
(`stack-alerts.yml`). Each one's `runbook_url` annotation links to its
section below: what it means, and what to check first. Thresholds are
starting points; tune `for:` and the ratios to your traffic. Every alert has
`promtool` unit tests (`prometheus/tests/`), which CI runs
(`.github/workflows/observability.yml`). [ADR
0039](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0039-observability-stack-alerts-and-deployment.md)
records why the rules are written as they are.

## Where alerts go

Prometheus sends firing alerts to Alertmanager (the compose stack runs one on
`127.0.0.1:9093`, and Grafana shows them under **Alerting**, data source
"Alertmanager"). **Alertmanager's only receiver, `default`, has no
integration: it notifies nobody** until you give it one. To be emailed, paged
or posted to a chat, add an integration to that receiver in
`deploy/observability/alertmanager/alertmanager.yml`, for example a webhook:

```yaml
receivers:
  - name: default
    webhook_configs:
      - url: https://alerts.example.com/hook
```

and check it with `amtool check-config alertmanager.yml`. The file's inhibit
rules hold back alerts that are a consequence of one already firing:
`A2APersistenceErrors` while `A2APersistenceFailing` fires for the same job
and operation, `A2AAgentInstancesLost` while the job is `A2AAgentSilent`,
and every agent's silence while `OTelCollectorDown` fires (this stack has one
Collector; with several, or agents that push past it, narrow that rule).

## What counts as a failure

The ratio alerts count the server's failures, not every status the
OpenTelemetry conventions mark as an error. A call the client abandoned
(`error_type="cancelled"`) and a call refused as the caller's mistake are
left out: every status A2A §5.4 maps to a 4xx, so on JSON-RPC every code but
`-32603` (internal error) and `-32006` (invalid agent response), any HTTP
4xx, and every gRPC code outside `UNKNOWN`, `DEADLINE_EXCEEDED`,
`UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE` and `DATA_LOSS`. Until 0.16.0 the
JSON-RPC codes counted: a client polling for tasks the retention reaper had
removed paged the on-call over JSON-RPC, and not over HTTP+JSON or gRPC
(audit NEW-c-1). Caller mistakes are `a2a:rpc_server_caller_errors:rate5m`,
the server dashboard's *Caller errors* panel. `scripts/check_stack.py`
derives the list from the crates' error codes and fails CI when a code is
added and the rules do not place it.

One case stays mixed: A2A has no throttling code, so on JSON-RPC an
overloaded server and a rate-limited caller both answer `-32603` and count
as failures, while over gRPC they are `RESOURCE_EXHAUSTED`, which does not.

Each ratio alert also needs traffic (a floor in calls a second): one failure
in two calls an hour is 50% and nobody's problem. A job under the floor that
fails every call does not page on the ratio.

## `A2AServerErrorRatioHigh`

**warning.** More than 5% of a job's calls failed (as above) for 10 minutes,
while it took more than 0.1 calls a second. Split by method and binding with
`a2a:rpc_server_errors:rate5m` (the server dashboard's *Failed calls per
second*). A rise confined to `SendStreamingMessage` is a different incident
from one across every method. Then the job's traces in Tempo, filtered by
status `error`.

## `A2APersistenceErrors`

**warning.** A task write could not be persisted in the last hour, for this
job and operation. A streaming client already received those events, so the
stored task and what clients saw have diverged; a later `GetTask` returns the
wrong answer. It stays firing for an hour after the failure so it is seen,
rather than paging for five minutes and resolving as it did before 0.16.0
(audit C-14). Look at the `operation` and `error` labels on
`a2a_server_persistence_errors_total` (the [metrics
reference](./metrics.md#attribute-values) lists them) and the store's own
logs at that time. The rule also catches a failure on a counter whose first
sample is already 1 (an OpenTelemetry counter is exported only once it has
counted something), which `increase()` alone cannot see.

## `A2APersistenceFailing`

**critical.** Task writes failed in every five-minute window for 10 minutes:
the store is refusing writes. Every task updated meanwhile may read back
wrong. Check the store's health (connections, disk, a failover in progress,
`A2AStoreRetriesHigh`) before the agent.

## `A2ARequestsUnanswered`

**warning.** Requests that produced neither a response nor an error, per
instance, for 15 minutes: a panicking executor, dropped connections, or a
process exiting mid-request. The threshold (0.01 a second) sits above
floating-point noise, since the rule is a difference of rates. Check the
instance's logs for panics and its restarts.

## `A2AQueueDepthGrowing`

**warning.** Active event queues above 100 and rising for 30 minutes. Each
open stream holds one; a depth that never comes down is subscribers that
stopped reading without disconnecting.

## `A2APushDeliveryTruncated`

**warning.** Push deliveries were given up with retries left:
`push_delivery_timeout` is shorter than the sender's retry schedule. A
configuration problem; it will not resolve on its own and no webhook reports
it.

## `A2APushDeliveryFailing`

**warning.** Over 20% of push deliveries failed, timed out or were truncated
for 15 minutes. Split by outcome with `a2a:push_deliveries:rate5m`; `failed`
and `timeout` point at the receiver, `suspended` and `gone` at the policy that
stopped delivering to it.

## `A2AUnaryLatencyHigh`

**warning.** More than 1% of a method's non-streaming calls took longer than
5 s for 15 minutes, while it took more than 0.1 calls a second. It reads the
5 s bucket bound itself (`a2a:rpc_server_unary_over_5s:ratio_rate5m`), so the
share is exact. Until 0.16.0 it compared an interpolated p99 with 2.5 s,
which sits inside the (1 s, 5 s] bucket of the default boundaries: 98 calls
under 1 s and 2 at 1.1 s read as a p99 of 3.0 s and paged (audit C-14). The
dashboard's quantiles are still interpolated. A blocking `SendMessage`
includes the executor's work; set the threshold per method if your agents
run long turns, or alert on another bound (`le` 1, 10, 50, 100).

## `A2AAgentSilent`

**critical.** A job seen in the last day of which no instance has sent
telemetry for 10 minutes: the agent or its exporters stopped. Over OTLP there
is no scrape and so no `up` series; `target_info` comes with every export.
Keyed on the job: a rollout or restart, which replaces instances, does not
fire it, and it stays firing for a day. If every job went silent together,
look at the Collector first (`OTelCollectorDown`).

## `A2AAgentInstancesLost`

**warning.** Fewer instances of a job sent telemetry in the last 10 minutes
than in the same 10 minutes an hour before: an instance stopped and was not
replaced, or the job was scaled in. It resolves within the hour, once the
smaller count is what the hour before saw too.

## `A2AStoreRetriesHigh`

**warning.** Store operations ran again after a failure more than 0.1 times a
second for 15 minutes, by `cause` (`unavailable`, `connection_lost`,
`read_only`). Each retry passed, so nothing was lost yet; a rate that stays up
is the store degrading before `A2APersistenceErrors` counts what it lost. A
failover shows as a burst shorter than this.

## `A2AExecutorFailureRatioHigh`

**warning.** More than 10% of the agent's executor runs returned an error or
never returned (`aborted`: a panic or a dropped run) for 15 minutes, while it
ran more than 0.05 a second. `budget_exhausted` and `canceled` are not counted:
the first is often the caller's deadline, the second the caller's choice.
Split by outcome with `a2a:executor_runs:rate5m`, then the failing runs'
traces.

## `A2AModelCallsFailing`

**warning.** More than 10% of the tool loop's model calls failed for 15
minutes, while it made more than 0.05 a second: the provider is down,
rate-limiting (`error_type` 429) or timing out. The GenAI dashboard splits
them by model and `error_type`.

## `A2AClientErrorRatioHigh`

**warning.** More than 5% of the calls a client job made to one agent
(`server_address`) failed for 10 minutes, while it made more than 0.1 calls a
second to it. Failed means the agent's failure by the rule above, or no
answer at all (`timeout`, `transport`, a response that did not decode); a 404
or `TaskNotFound` is the caller's own mistake and is not counted. The client
dashboard's *Failures by kind* shows every kind.

## `OTelCollectorDown`

**critical.** Prometheus has not reached the Collector's own telemetry
endpoint (`:8888`, job `otelcol`) for 2 minutes. Every agent exporting
through it is losing traces, metrics and logs: each SDK retries, then drops.
Check the container (`docker compose ps`, its logs, an out-of-memory kill).

## `OTelCollectorExportFailing`

**warning.** One of the Collector's exporters gave up on items (its retries
ran out) in every five-minute window for 10 minutes: that backend (Prometheus,
Tempo or Loki) is down, refusing, or unreachable. What it gave up on is lost.
On 2026-11-04, with Loki stopped, `otelcol_exporter_send_failed_log_records`
appeared about four and a half minutes later, once the retries had run out;
the queue (`OTelCollectorQueueFilling`) grows first.

## `OTelCollectorRefusing`

**warning.** The Collector refused or failed incoming items for 10 minutes.
Refused is usually `memory_limiter` pushing back because the container is near
its memory limit (1 GiB in the compose stack; the limiter refuses above 819
MiB); the agents retry, then drop. Raise the limit or add a Collector.

## `OTelCollectorQueueFilling`

**warning.** An exporter's sending queue stayed over 80% full for 5 minutes:
its backend is slower than the traffic or not answering. A full queue
refuses new items. The compose stack keeps the queues on disk
(`file_storage`), so a Collector restart does not lose them.

## `PrometheusRuleEvaluationFailing`

**warning.** Rules in a group failed to evaluate for 10 minutes. An alert
whose rule cannot evaluate never fires, so the alerts in that group are blind
until this is fixed. The rule's `lastError` is on Prometheus's *Rules* page.
This one fired live on 2026-11-04 on a first draft of `OTelCollectorRefusing`
whose single selector over several metric names gave duplicate series.

## `PrometheusNotificationsFailing`

**warning.** Prometheus could not send alerts to Alertmanager for 10 minutes,
so firing alerts notify nobody. It shows on Prometheus's *Alerts* page, which
is all there is while Alertmanager is unreachable.
