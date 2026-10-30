<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Metrics reference

Every instrument the SDK records — 27 in all — with its type, unit,
attributes, the values each attribute takes, its histogram buckets and its
Prometheus name; then the `Metrics` trait the server's instruments are fed
through, and what is not measured. The [catalogue on the landing
page](../observability.md#the-catalogue) is the short list of the server's
own instruments.

All of them are recorded through the OpenTelemetry API (the `otel` feature)
and exported by `Telemetry` ([Configuring `Telemetry`](./telemetry.md)). The
server's are under the instrumentation scope `a2a.server` (Prometheus label
`otel_scope_name="a2a.server"`), the client's under `a2a.client`. Metric names
are not listed as stable in `STABILITY.md`, so a rename is announced in the
CHANGELOG.

| Group | Instruments | Recorded by |
|---|---|---|
| [Server calls and handler](#server-calls-and-handler) | 14 | `OtelMetrics` (the handler's default) |
| [Client calls](#client-calls) | 1 | Every client call |
| [Agent card and webhooks](#agent-card-and-webhooks) | 2 | The card route; the push sender |
| [Task store](#task-store) | 1 | A store that names its database |
| [The tool loop](#the-tool-loop) | 9 | `ToolLoop` (feature `tool-loop`) |

## Server calls and handler

`OtelMetrics` records these, from the callbacks of the [`Metrics`
trait](#the-metrics-trait); with no `with_metrics` on the handler builder it is
what a handler records to.

| Instrument | Type | Unit | Attributes |
|---|---|---|---|
| `rpc.server.call.duration` | histogram | `s` | `rpc.system.name`, `rpc.method`, `rpc.status_code`, `error.type` |
| `a2a.server.requests` | counter | `{request}` | `method` |
| `a2a.server.responses` | counter | `{response}` | `method` |
| `a2a.server.errors` | counter | `{error}` | `method`, `error` |
| `a2a.server.request.duration` | histogram | `s` | `method` |
| `a2a.server.executor.duration` | histogram | `s` | `outcome` |
| `a2a.server.queue_depth` | gauge | `{queue}` | none |
| `a2a.server.persistence_errors` | counter | `{error}` | `operation`, `error` |
| `a2a.server.store_retries` | counter | `{retry}` | `operation`, `cause` |
| `a2a.server.push_deliveries` | counter | `{delivery}` | `outcome` |
| `a2a.server.pool.active` | gauge | `{connection}` | none |
| `a2a.server.pool.idle` | gauge | `{connection}` | none |
| `a2a.server.pool.created` | counter | `{connection}` | none |
| `a2a.server.pool.closed` | counter | `{connection}` | none |

The catalogue's fifteenth row, `db.client.operation.duration`, is recorded by
the task store, not `OtelMetrics` ([below](#task-store)). The three duration
histograms here use
the RPC conventions' advisory buckets, 0.1 ms to 100 s
(`RPC_DURATION_BUCKETS`):

```text
0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10, 50, 100
```

A quantile cannot read above the top bucket, so a call that runs past 100 s
reads as 100 s.

### Attribute values

| Attribute | Values |
|---|---|
| `method` | The A2A method: `SendMessage`, `SendStreamingMessage`, `GetTask`, `ListTasks`, `CancelTask`, `SubscribeToTask`, `CreateTaskPushNotificationConfig`, `GetTaskPushNotificationConfig`, `ListTaskPushNotificationConfigs`, `DeleteTaskPushNotificationConfig`, `GetExtendedAgentCard` |
| `error` on `a2a.server.errors` | `ServerError::metric_label`: `task_not_found`, `task_not_cancelable`, `invalid_params`, `serialization`, `http`, `http_client`, `transport`, `push_not_supported`, `internal`, `method_not_found`, `payload_too_large`, `unsupported_operation`, `invalid_state_transition`, `overloaded`, `halted`; for an A2A error the binding answers as it is, `unauthenticated` or `permission_denied` for a refused credential, else its code's label (below); and `executor_drain_timeout` on `SendMessage` when `HandlerLimits::executor_drain_timeout` elapses |
| A2A code labels | `parse_error`, `invalid_request`, `method_not_found`, `invalid_params`, `internal_error`, `task_not_found`, `task_not_cancelable`, `push_notification_not_supported`, `unsupported_operation`, `content_type_not_supported`, `invalid_agent_response`, `extended_agent_card_not_configured`, `extension_support_required`, `version_not_supported` |
| `outcome` on `a2a.server.executor.duration` | `completed`, `error`, `budget_exhausted`, `canceled`, `aborted` (`executor_outcome`) |
| `outcome` on `a2a.server.push_deliveries` | `delivered`, `failed`, `timeout`, `timeout_truncated`, `dropped` (`push_outcome`) |
| `operation` on `persistence_errors` and `store_retries` | `status_update`, `artifact_append`, `artifact_push`, `event_append`, `task_snapshot`, `failed_state`, `history_append`, `event_log_catchup`, `queue_handoff`, `statement`, `audit_append` (`persistence_operation`) |
| `error` on `persistence_errors` | The A2A code label of the store's error, or one of the sub-kinds below |
| `cause` on `store_retries` | `unavailable` (the store said not now; a turn's write, retried for up to `HandlerLimits::store_write_retry`), `connection_lost` (with `operation` `statement`: a PostgreSQL statement ran once more on a new connection) |

Every one of these is a bounded set: none carries a task id, a message or a
string the peer chose, so a client cannot inflate cardinality through them.

The persistence `error` sub-kinds, by operation:

| `operation` | `error` | Meaning |
|---|---|---|
| `event_append` | `position_conflict` | The position was already held by a different event, or the store could not establish it was the same one. Look for a second replica serving the same task id |
| `event_append` | `task_absent` | The in-memory store held no such task (it was evicted) |
| `event_append` | `hold_ceiling` | The in-memory log, at its held-events ceiling, dropped an event an attached stream still owed; that stream ends with `streamLagged` |
| `event_log_catchup` | `timed_out` | A resubscribe's wait for the log to catch up expired; the replay was served short |
| `queue_handoff` | `channel_closed` | The persistence processor was gone and the event was dropped |
| `audit_append` | `audit` | An audit record could not be written |

### `rpc.server.call.duration`

The OpenTelemetry semantic conventions' instrument, recorded once per inbound
call on every binding, with their attributes:

* `rpc.system.name`: `jsonrpc` (over HTTP or WebSocket), `a2a_http_json`,
  `grpc`, or `slimrpc` for `a2a-protocol-slimrpc`.
* `rpc.method`: the fully qualified method (`lf.a2a.v1.A2AService/SendMessage`),
  `_OTHER` for one the server does not serve, or absent when the request was
  refused before it named one.
* `rpc.status_code`: the status the binding answered with — the JSON-RPC error
  code (`-32001`), the HTTP status (`404`), or the gRPC status name
  (`NOT_FOUND`; gRPC and SLIMRPC, which uses gRPC's status codes, also report
  `OK`).
* `error.type`, only when the call failed: that status, or `cancelled` for a
  call whose peer went away first.

**What counts as failed** is each binding's semantic conventions' rule, so
`error.type` marks a server failure rather than every answer that was not a
success: every JSON-RPC error code ("All JSON RPC error codes SHOULD be
considered errors"); on HTTP+JSON a 5xx but not a 4xx (the HTTP conventions
leave a server span's status unset for a 4xx); on gRPC only `UNKNOWN`,
`DEADLINE_EXCEEDED`, `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE` and
`DATA_LOSS`. A `GetTask` for an unknown task is `rpc.status_code` `NOT_FOUND`
(or `404`) with no `error.type`; a dashboard that wants the 4xx class reads
`rpc.status_code`. A binding outside this crate opts into gRPC's rule with
`CustomBinding::with_grpc_server_errors`.

It records calls the handler never sees, with the status they were answered:
on JSON-RPC, a body that is not a request (no `rpc.method`), an unknown
method, a batch over the limit; on HTTP+JSON and the axum router, a request
refused before routing — an unsupported or missing `A2A-Version`, a content
type, a path no route serves (no `rpc.method`) — and one refused on its body
or parameters, under the route's method; on gRPC, a call refused for its
metadata or a request that does not convert, under its method. A streaming
call — `SendStreamingMessage` or `SubscribeToTask`, over SSE, gRPC or
WebSocket — lasts until its stream ends, as the conventions ask ("the span
SHOULD cover the full lifetime of the request and/or response streams"): it
is recorded then, with the error the stream's last frame carried, or
`cancelled` if the client went away first. That is why the shipped latency
rules take quantiles over non-streaming calls only. The same values reach a
custom `Metrics` as `on_rpc_call`.

### The `a2a.server.*` counters

`a2a.server.request.duration` times the same calls by A2A method, with the
same buckets. OpenTelemetry names a duration histogram `.duration`, so a
dashboard built for the conventions finds it (it was `a2a.server.latency`
until 0.15; see the [upgrade guide](../../reference/upgrading/0.14-to-0.15/telemetry.md#a2aserverlatency-is-gone-units-are-ucum)).

`requests`, `responses` and `errors` are separate on purpose. A request ends in
exactly one of the other two — a call answered with an error counts in
`errors`, not in `responses` — so `requests − responses − errors` is the
requests that produced no answer at all: a panicked executor, a dropped
connection, a process that went away mid-request. A single counter would hide
exactly the failure you want to see. `requests − responses` alone is not that
gap: it counts every error as unanswered, so the recording rule in
`deploy/observability/` subtracts both (measured against a live Prometheus).

`a2a.server.executor.duration` records every executor run, by how it ended —
the run, which usually outlives the call that started it (a streaming send, a
task that waits for input). A run whose future was dropped (a panic, or an
abort) is reported as `aborted` from the guard that watched it, with the time
it had taken, so it is in the histogram rather than missing from it.

`a2a.server.persistence_errors` above zero means a store write failed: the
streaming client already received the event, so the stored task and what
clients saw have diverged, and a later `GetTask` returns the wrong answer.
`a2a.server.store_retries` counts writes that failed once and then succeeded,
so nothing was lost; a failover shows as a burst, one per statement in flight,
and a rate that keeps rising is the store degrading before
`persistence_errors` starts counting what it lost. The log line beside each
retry is throttled to one every ten seconds, so the counter, not the log, is
the measure.

`a2a.server.push_deliveries` counts each delivery's final outcome:

```text
delivered          the webhook accepted it
failed             reached, and refused it — or the sender itself errored
timeout            the webhook did not answer inside the time it was given
timeout_truncated  a configuration result: push_delivery_timeout cut the schedule short
dropped            never started: the task's push lane was full behind a slow webhook
```

`timeout_truncated` is not a network failure. It means `push_delivery_timeout`
was shorter than the sender's own retry schedule, so the delivery was
abandoned with attempts remaining; it will not resolve on its own and it will
not appear as a webhook error. Every registered config is attempted for every
event (spec §4.3.3); `push_delivery_budget` only sets how many deliveries run
at once, so there is no `skipped` outcome (the `push_outcome::SKIPPED` constant
remains for code that matches on it, and is never reported).

The `pool` instruments are reported by the servers this crate runs — `serve`,
`serve_with_addr` and `Server::serve_with_shutdown`. A connection is *active*
while a request on it is in flight, including a response still streaming,
and *idle* while it is open with none; `closed` counts connections that ended
in an error or timeout. The gRPC and WebSocket dispatchers' own listeners,
and a router you serve yourself, do not report them.

## Client calls

`rpc.client.call.duration` is the conventions' client histogram (unit `s`, the
same buckets as the server's), with `rpc.system.name`, `rpc.method`,
`rpc.status_code` as the server records it (the JSON-RPC code, the HTTP
status, or the gRPC status name, `OK` on a gRPC success), `error.type` for a
failed call (that status, or the failure's class — `timeout`, `transport` —
when the agent never answered), and `server.address` and `server.port`. A
streaming call is measured until the caller drops its stream, as its span is.

A call is recorded as it ended for the caller, not as the agent answered: an
answer the client then refuses (an interceptor's `after` hook errors, the
result does not decode, the response is malformed, a content signature does
not verify) is a failed call, with `error.type` `serialization`,
`invalid_response`, `content_not_verified` or the hook's own error. A call the
caller gives up on — its future dropped by a `tokio::time::timeout` or a
`select!` — and a stream dropped before its final event or the end of its body
are recorded as `error.type` `cancelled`, with the time they ran, as the server
records a peer that goes away; the slow calls a caller abandons are the ones a
latency panel most needs. A stream read to its end is not. The interceptors'
error hook does not run for a dropped call: a destructor cannot await it. A
call refused before anything is sent — an undeclared capability, a message
that is not well formed, a `before` hook that fails — is not an RPC and is not
recorded.

The metric records through the global meter provider — the one `Telemetry`
installs — as it stands at each call, so a client built before `Telemetry`
records once it is installed, under the meter `a2a.client`. A call on a custom
transport that names no `rpc.system.name` (`Transport::rpc_system`) is not
recorded, since that attribute is required.

## Agent card and webhooks

**`http.server.request.duration`** (histogram, `s`) times each request for the
card at `/.well-known/agent-card.json`, on any listener that serves it
(JSON-RPC, HTTP+JSON, the axum router, WebSocket, gRPC), under the
`a2a.server` scope. It carries only `http.request.method`, `url.scheme`,
`http.route`, `http.response.status_code`, `network.protocol.version` and, for
a 5xx, `error.type` (the status) — none of which a caller can choose freely.
The request's span carries more ([Spans reference](./spans.md#agent-card-requests)).

**`http.client.request.duration`** (histogram, `s`) times each
push-notification POST, every retry included, under the `a2a.server` scope.
Its attributes are those of the attempt's `POST` span: `http.request.method`
(`POST`), `server.address`, `server.port`, `http.response.status_code` when a
response came back, and `error.type` when the attempt failed: the status for a
4xx or 5xx, `timeout`, or `transport` for no response. The URL is not an
attribute, because it would carry the receiver's secret and unbounded values.
`a2a.server.push_deliveries` counts each delivery's final outcome; this
metric times each attempt.

Both use the HTTP conventions' advisory buckets, 5 ms to 10 s:

```text
0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1, 2.5, 5, 7.5, 10
```

## Task store

**`db.client.operation.duration`** (histogram, `s`) times each operation of a
task store that names its database through `TaskStore::database`, under the
`a2a.server` scope, with the database conventions' advisory buckets, 1 ms to
10 s:

```text
0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10
```

Its attributes are the operation's span's ([Spans
reference](./spans.md#task-store-operations)): `db.system.name` (`postgresql`,
`sqlite`, or a custom store's), `db.operation.name` (the trait method: `get`,
`save_status_delta`, `append_event`, …), `db.namespace` and `server.address` /
`server.port` for PostgreSQL, and on failure `error.type`: `unavailable` for a
temporary failure (a pool with no free connection, a locked SQLite file, a
PostgreSQL connection error), otherwise the A2A error code the store answered
(`-32603` internal). The SQLite and PostgreSQL stores, their tenant-aware
forms, and the encrypted and audited wrappers around them name their
database; the in-memory stores run against no database and record nothing. An
operation whose future is dropped before it finishes records no duration.

In Prometheus, fed over OTLP, it is `db_client_operation_duration_seconds`,
with labels `db_system_name`, `db_operation_name` and, on failures,
`error_type` (measured 2026-10-24 against Prometheus 3.15.1 with a SQLite
store).

## The tool loop

A `ToolLoop` (`server::tool_loop`, feature `tool-loop`, with `otel`) records
the OpenTelemetry `GenAI` conventions' metrics
(`open-telemetry/semantic-conventions-genai` at `9ffc7e0`, all
`development`), with their advisory buckets:

| Instrument | Type | Unit | Recorded | Buckets |
|---|---|---|---|---|
| `gen_ai.client.inference.duration` | histogram | `s` | Each model turn | 0.01 to 81.92 s, doubling |
| `gen_ai.client.inference.usage.input_tokens` | counter | `{token}` | Each turn whose provider reported a non-zero count | — |
| `gen_ai.client.inference.usage.output_tokens` | counter | `{token}` | Likewise | — |
| `gen_ai.client.inference.operation.input_tokens` | histogram | `{token}` | Each turn whose provider reported a count | 1 to 67,108,864, ×4 |
| `gen_ai.client.inference.operation.output_tokens` | histogram | `{token}` | Likewise | 1 to 67,108,864, ×4 |
| `gen_ai.execute_tool.duration` | histogram | `s` | Each tool call that ran | 0.01 to 81.92 s, doubling |
| `gen_ai.invoke_agent.duration` | histogram | `s` | Each run of the loop | 0.1 to 409.6 s, doubling |
| `gen_ai.invoke_agent.inference_calls` | histogram | `{inference_call}` | Each run: how many turns it asked for | 1 to 128, doubling |
| `gen_ai.invoke_agent.tool_calls` | histogram | `{tool_call}` | Each run: how many calls it ran | 1 to 128, doubling |

Their attributes:

| Instruments | Attributes |
|---|---|
| A turn's duration and token histograms | `gen_ai.operation.name` (`chat`), `gen_ai.provider.name`, `gen_ai.request.model`; when known `gen_ai.response.model`, `server.address`, `server.port`; `error.type` on failure |
| The usage counters | The same without `error.type`, plus `gen_ai.token.modality` (`text`) |
| `gen_ai.execute_tool.duration` | `gen_ai.tool.name`, `gen_ai.tool.type` (`function`); `error.type` on failure |
| `gen_ai.invoke_agent.duration` | `gen_ai.request.model`, `gen_ai.agent.name` (from `ToolLoop::with_agent_name`, when set); `error.type` on failure |
| The two per-run counts | `gen_ai.agent.name` only |

`error.type` is, for a turn, the provider's failure (`ModelError::error_type`:
the HTTP status, `timeout`, `transport`, `invalid_response`,
`response_too_large`, `configuration`, `_OTHER`); for a call, `unknown_tool`,
`invalid_arguments`, `tool_error` or `tool_fatal`; for a run, the turn's type
when the model failed, `tool_fatal`, `turn_limit` or `budget_exhausted`; and
`cancelled` for any of them abandoned before it ended — its cancellation
token fired, or the future running it was dropped, as an executor timeout
does. `gen_ai.client.operation.duration` is not recorded: the conventions say
it "SHOULD NOT be reported for inference operations".

The metrics go to the global meter provider as it is when each is recorded,
so `Telemetry` installed after the loop is built still receives them;
`ToolLoop::with_meter` sends them elsewhere.

## Prometheus names

Prometheus, fed over OTLP with `UnderscoreEscapingWithSuffixes` (the shipped
configuration), scraping a Collector's Prometheus exporter, or scraping the
agent's own `/metrics`, gives the same names: dots become underscores,
counters gain `_total`, an `s` unit gains `_seconds`, and a brace-annotated
unit adds nothing. Labels follow the same rule (`rpc.system.name` →
`rpc_system_name`). The **Source** column says whether the name was read from
a live Prometheus or follows from that rule.

| Instrument | Prometheus name | Source |
|---|---|---|
| `rpc.server.call.duration` | `rpc_server_call_duration_seconds` | measured |
| `a2a.server.requests` | `a2a_server_requests_total` | measured |
| `a2a.server.responses` | `a2a_server_responses_total` | measured |
| `a2a.server.errors` | `a2a_server_errors_total` | measured |
| `a2a.server.request.duration` | `a2a_server_request_duration_seconds` | measured |
| `a2a.server.executor.duration` | `a2a_server_executor_duration_seconds` | derived |
| `a2a.server.queue_depth` | `a2a_server_queue_depth` | measured |
| `a2a.server.persistence_errors` | `a2a_server_persistence_errors_total` | measured |
| `a2a.server.store_retries` | `a2a_server_store_retries_total` | derived |
| `a2a.server.push_deliveries` | `a2a_server_push_deliveries_total` | measured |
| `a2a.server.pool.active` | `a2a_server_pool_active` | measured |
| `a2a.server.pool.idle` | `a2a_server_pool_idle` | measured |
| `a2a.server.pool.created` | `a2a_server_pool_created_total` | measured |
| `a2a.server.pool.closed` | `a2a_server_pool_closed_total` | measured |
| `db.client.operation.duration` | `db_client_operation_duration_seconds` | measured |
| `rpc.client.call.duration` | `rpc_client_call_duration_seconds` | measured |
| `http.server.request.duration` | `http_server_request_duration_seconds` | derived |
| `http.client.request.duration` | `http_client_request_duration_seconds` | derived |
| `gen_ai.client.inference.duration` | `gen_ai_client_inference_duration_seconds` | measured |
| `gen_ai.client.inference.usage.input_tokens` | `gen_ai_client_inference_usage_input_tokens_total` | measured |
| `gen_ai.client.inference.usage.output_tokens` | `gen_ai_client_inference_usage_output_tokens_total` | measured |
| `gen_ai.client.inference.operation.input_tokens` | `gen_ai_client_inference_operation_input_tokens` | measured |
| `gen_ai.client.inference.operation.output_tokens` | `gen_ai_client_inference_operation_output_tokens` | measured |
| `gen_ai.execute_tool.duration` | `gen_ai_execute_tool_duration_seconds` | measured |
| `gen_ai.invoke_agent.duration` | `gen_ai_invoke_agent_duration_seconds` | measured |
| `gen_ai.invoke_agent.inference_calls` | `gen_ai_invoke_agent_inference_calls` | measured |
| `gen_ai.invoke_agent.tool_calls` | `gen_ai_invoke_agent_tool_calls` | measured |

The server rows marked measured were rendered through `opentelemetry-prometheus`
0.33.1, the version matching this crate's `opentelemetry_sdk` (re-measured
2026-10-18 after the 0.15 rename and the move to UCUM units). The client,
store and GenAI rows are the names the shipped recording rules and dashboards
query, every one of which returned data from a live Prometheus fed by these
agents (2026-10-23 and 2026-10-24; see `deploy/observability/README.md`). The
`{token}`, `{inference_call}` and `{tool_call}` histograms have no unit suffix,
because the exporter suffixes only units it can translate. The derived rows
were added after those measurements.

Alongside them the exporter emits `target_info`, a gauge carrying the
resource ([Resource attributes](./resource.md#what-they-become-in-prometheus)).

## The `Metrics` trait

The server's instruments are fed through `server::metrics::Metrics`, a trait
with a default no-op implementation for every method. `OtelMetrics`
implements it; so can you, to count something of your own
(`RequestHandlerBuilder::with_additional_metrics`) or to replace the export
(`with_metrics`).

| Callback | Called | Feeds |
|---|---|---|
| `on_request(method)` | A request is received, before processing | `a2a.server.requests` |
| `on_response(method)` | A response is sent successfully | `a2a.server.responses` |
| `on_error(method, error_kind)` | A request ends in an error | `a2a.server.errors` |
| `on_latency(method, duration)` | A request completes, either way | `a2a.server.request.duration` |
| `on_rpc_call(&RpcCall)` | Once per inbound call on every binding, including calls the handler never reached | `rpc.server.call.duration` |
| `on_executor_run(outcome, duration)` | An executor's run ends | `a2a.server.executor.duration` |
| `on_queue_depth_change(active_queues)` | The number of active event queues changes | `a2a.server.queue_depth` |
| `on_connection_pool_stats(&ConnectionPoolStats)` | With the HTTP connection counts, from the SDK's own servers | `a2a.server.pool.*` |
| `on_persistence_error(operation, error_kind)` | A task write could not be persisted | `a2a.server.persistence_errors` |
| `on_store_retry(operation, cause)` | A store operation is run again after a failure that passed | `a2a.server.store_retries` |
| `on_push_delivery(outcome)` | A push delivery ends | `a2a.server.push_deliveries` |

`RpcCall` carries `system`, `method` (`Option`), `duration`, `status_code`
(`Option`) and `error_type` (`Option`), the values above. The label constants
are public modules: `executor_outcome`, `persistence_operation`,
`push_outcome`, `event_append_error`, `event_log_catchup_error` and
`queue_handoff_error`. `ConnectionPoolStats` carries
`active_connections`, `idle_connections`, `total_connections_created` and
`connections_closed`, the last two cumulative since start (`OtelMetrics` adds
only the increase to its counters). `NoopMetrics` discards everything, and
`Arc<T>` implements `Metrics` for any `T` that does.

**`MetricsHandle`** is the shareable `Metrics` a task store holds: every store
in this crate has `with_metrics(MetricsHandle)`, built with
`MetricsHandle::new(metrics)` or `MetricsHandle::from_arc(arc)`, and defaults
to `NoopMetrics`. The store reports what only a store can see — an event that
never reached the log, a held event dropped at the ceiling. The handler
builder gives the default store it creates the handler's own metrics; a store
you supply keeps the metrics you gave it, so pass the same metrics to both if
you want those counts.

## What is not measured

Stated so a green dashboard is not mistaken for a complete one:

* **A store's individual SQL statements have no span.** Each task-store
  operation is one span and one `db.client.operation.duration` point; the
  statements inside it are not. sqlx 0.9 reports each as a `tracing` *event*,
  not a span (target `sqlx::query`, with the SQL as `db.statement` —
  placeholders, not the bound values — `rows_affected` and `elapsed_secs`), at
  `DEBUG`, or at `WARN` when it takes a second or more. Under `Telemetry` those
  events are log records in the trace of the operation that ran them; the slow
  ones at the default `INFO` log level, every one with
  `TelemetryBuilder::with_log_level(Level::DEBUG)`. A custom store that does
  not override `TaskStore::database` has neither span nor duration.
* **There is no per-tenant metric dimension.** A tenant hitting
  `max_concurrent_tasks` shows as `Overloaded` errors in the aggregate; see
  [Multi-Tenancy](../multi-tenancy.md) for what the limits actually are.
  (The call's span does carry `a2a.tenant`.)
* **Model calls are seen only through the tool loop.** An executor built on
  `server::tool_loop` (feature `tool-loop`) records a `chat` span per model
  call and an `execute_tool` span per tool call, under `invoke_agent`, and
  the `gen_ai.*` metrics above. Any other model client is invisible to the
  SDK: instrument it, or use a framework that reports `chat` spans.
* **The connection pool of the gRPC and WebSocket listeners** and of a router
  you serve yourself is not reported.
