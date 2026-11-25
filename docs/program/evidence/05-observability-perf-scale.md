<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> **Frozen audit input, 2026-11-25, of the tree at `2058058d`.** Not maintained. Written by a delegated
> read-only audit for [`docs/program/README.md`](../README.md); its VERIFIED and INFERRED labels are the auditor's own.
> Re-check an anchor before acting on it. `UPSTREAM/` is a local clone of the named upstream repository.

# 05 — Observability, performance and scale audit (a2a-rust 0.16.0)

Read-only audit of `/home/user/a2a-rust` at `2058058d` (`git log -1`), 2026-11-25. Nothing was built, run or
timed. Every claim is labelled:

- **VERIFIED** — read in the file cited (code, results file or design record) in this session.
- **CONJECTURED** — a hypothesis from reading code; must be measured before acting.
- **NOT MEASURED** — the repository has no measurement; nothing here supplies one.

Paths are relative to the repository root; `srv/` abbreviates `crates/a2a-protocol-server/src/` and `cli/`
abbreviates `crates/a2a-protocol-client/src/`.

Limits of this audit: the ADRs 0013, 0051 and parts of 0032/0050/0054/0056 were read by targeted search, not
line by line; the three Grafana dashboards were read as panel titles and metric names, not per query; the
audit registers were read through `docs/audits/0.16.0-release/findings.json` (130 rows) and the two review
registers, not the per-area markdown under `docs/audits/2026-10-29/`. Upstream OpenTelemetry semantic
conventions were not fetched, so "current upstream" comparisons are NOT VERIFIED here.

---

## 1. Observability

### 1.1 Spans (all VERIFIED by reading the `info_span!` call)

| Span (`tracing` name → `otel.name`) | Kind | Attributes declared | Where |
|---|---|---|---|
| `a2a.rpc` → fully qualified method (`lf.a2a.v1.A2AService/SendMessage`) or `_OTHER` | SERVER | `rpc.system.name`, `rpc.method`, `rpc.method_original`, `rpc.status_code`, `jsonrpc.protocol.version`, `jsonrpc.request.id`, `error.type`, `http.request.method`, `http.route`, `server.address`, `server.port`, `network.peer.address`, `network.peer.port`, `a2a.method.name`, `a2a.protocol.version`, `a2a.tenant`, `a2a.message.id`, `a2a.task.id`, `a2a.task.state`, `gen_ai.conversation.id`, `gen_ai.agent.{name,description,version}` | `srv/rpc_span/mod.rs:428-457`; finish at `:391-413` |
| `a2a.task` → `a2a.execute` / `invoke_agent {card.name}` | INTERNAL | `a2a.task.id`, `a2a.context.id`, `a2a.task.state`, `a2a.lineage.root`, `a2a.lineage.depth`, `error.type`, `gen_ai.operation.name`, `gen_ai.conversation.id`, `gen_ai.agent.*`, `gen_ai.input.messages`, `gen_ai.output.messages` (content only with `with_span_content_capture`, capped at `CONTENT_LIMIT` 64 KiB, `agent.rs:68`) | `srv/rpc_span/agent.rs:165-183`; lineage at `agent/lineage.rs:16-19` |
| `a2a.task` → caller-given name (executor, event processor, push delivery, `a2a.sse`) | INTERNAL child of the call | `otel.name` only | `srv/rpc_span/spawn.rs:16-30`; SSE at `srv/streaming/sse.rs:511` |
| `gen_ai.chat` → `chat {model}` | CLIENT | `gen_ai.operation.name=chat`, `gen_ai.provider.name`, `gen_ai.request.model`, `server.address`, `server.port`, `gen_ai.response.model`, `gen_ai.response.id`, `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `error.type` (module doc also names `gen_ai.response.finish_reasons`) | `srv/rpc_span/tool.rs:100-116`, doc `:5-37` |
| `gen_ai.execute_tool` → `execute_tool {name or _OTHER}` | INTERNAL | `gen_ai.operation.name=execute_tool`, `gen_ai.tool.name`, `gen_ai.tool.call.id`, `gen_ai.tool.type=function`, `gen_ai.tool.description`, `error.type` | `srv/rpc_span/tool.rs:161-172` |
| `a2a.store` → `{operation}{target}` | CLIENT | `db.system.name`, `db.operation.name`, `db.namespace`, `server.address`, `server.port`, `error.type` (A2A code or `unavailable`, not SQLSTATE) | `srv/store/traced.rs:97-124` |
| card-serving span | SERVER | HTTP attributes | `srv/rpc_span/card.rs:184` |
| webhook POST span (push delivery) | CLIENT | HTTP attributes; writes its own `traceparent` | `srv/rpc_span/webhook.rs:249`, `:85` |
| `a2a.rpc.client` → qualified method | CLIENT | `rpc.system.name`, `rpc.method`, `rpc.method_original`, `jsonrpc.*`, `a2a.method.name`, `a2a.protocol.version`, `server.address`, `server.port`, `rpc.status_code`, `error.type`, `a2a.message.id`, `a2a.task.id`, `a2a.task.state`, `gen_ai.conversation.id`, `gen_ai.agent.*` | `cli/call_span.rs:73-98` |
| client card-fetch span | CLIENT | — | `cli/discovery/card_span.rs:30` |
| client webhook-receiver span | SERVER | — | `cli/webhook/span.rs:38` |

Convention versions the code targets (VERIFIED, pinned by upstream commit, not by release tag; no `schema_url`
string exists under `srv/otel`, `srv/rpc_span`, `srv/metrics`):

- RPC/HTTP/DB: `open-telemetry/semantic-conventions@55c98cf` (`srv/rpc_span/names.rs:43`, `address.rs:5`,
  `card.rs:11`, `webhook.rs:9` "2026-10-20", `stream.rs:6`, `srv/store/traced.rs:10`, `cli/call_metric.rs:7`).
- GenAI tool loop: `open-telemetry/semantic-conventions-genai@9ffc7e0`, "all `development`"
  (`srv/rpc_span/tool.rs:6-8`, `tool_metrics.rs:5-7`); executor span at `@4f85037` (2026-10-06)
  (`srv/rpc_span/agent.rs:10`).
- A2A attributes: draft PR #195 of `semantic-conventions-genai` at head `842a839`, read 2026-10-06, "open and
  every attribute in it is `development`" (`srv/rpc_span/a2a.rs:4-16`).

### 1.2 Metrics (VERIFIED)

Server, meter `a2a.server`, built in `srv/otel/mod.rs:149-323`, recorded at `:325-447`; control plane in
`srv/otel/control.rs:33-75`.

| Metric | Type | Unit | Labels | Cardinality bound |
|---|---|---|---|---|
| `rpc.server.call.duration` | Histogram (13 buckets, 0.1 ms–100 s, `mod.rs:460-462`) | s | `rpc.system.name`, `rpc.method`, `rpc.status_code`, `error.type`, opt. `a2a.tenant`, opt. `gen_ai.agent.name` | method ∈ 11 + `_OTHER` (`names.rs:121-143`); tenant ∈ allow-list + `_OTHER` (`control.rs:92-103`) |
| `a2a.server.requests` / `.responses` | Counter | {request}/{response} | `method` | 11 methods |
| `a2a.server.errors` | Counter | {error} | `method`, `error` | bounded by `ServerError::metric_label` |
| `a2a.server.request.duration` | Histogram | s | `method` | duplicates the RPC histogram |
| `a2a.server.executor.duration` | Histogram | s | `outcome` (5 values, `metrics/labels.rs:9-23`), opt. tenant/agent | bounded |
| `a2a.server.queue_depth` | Gauge | {queue} | none | 1 |
| `a2a.server.pool.{active,idle}` / `.pool.{created,closed}` | Gauge / Counter | {connection} | none | **HTTP listener connections**, fed only from `srv/serve/connections.rs:69`; not a database pool |
| `a2a.server.persistence_errors` | Counter | {error} | `operation`, `error` | constants in `metrics/labels.rs:31-172` |
| `a2a.server.store_retries` | Counter | {retry} | `operation`, `cause` | constants |
| `a2a.server.push_deliveries` | Counter | {delivery} | `outcome` | `metrics/push_outcome.rs` |
| `a2a.server.refusals` | Counter | {request} | `cause` ∈ `stream_cap, tenant_cap, push_config_cap, idempotency_store, event_log, halt_store` (`labels.rs:184-194`), opt. tenant | bounded |
| `a2a.server.lease.events` | Counter | {event} | `event` (`lapsed`) | bounded |
| `a2a.server.halts`, `a2a.server.halt.stopped_tasks` | Counter | {action}/{task} | `scope` ∈ all/tenant/root, `action` ∈ halt/resume | bounded |
| `a2a.server.claim.events` | Counter | {event} | `event` (`reaped`, `finished`) | bounded |
| `a2a.server.subscribe.wakes` | Counter | {wake} | `cause` (`notification`, `resync`, `timeout`) | bounded |
| `a2a.server.steering` | Counter | {message} | `outcome` (9 values, `labels.rs:228-245`) | bounded |
| `db.client.operation.duration` | Histogram | s | `db.system.name`, `db.operation.name`, `db.namespace`, `server.address`, `server.port`, `error.type` | task store only (`srv/store/traced.rs:129-160`) |
| `http.server.request.duration` (card) | Histogram | s | HTTP | `srv/rpc_span/card.rs:284` |
| `http.client.request.duration` (push webhook) | Histogram | s | HTTP | `srv/rpc_span/webhook.rs:222` |
| `gen_ai.client.inference.duration`, `gen_ai.execute_tool.duration`, `gen_ai.invoke_agent.duration` | Histogram | s | operation, provider, request model, response model (first 16 then `_OTHER`, ≤ `MAX_RESPONSE_MODEL_LEN`), `server.address/port`, `error.type`; tool name or `_OTHER` | `srv/rpc_span/tool_metrics.rs:100-176` |
| `gen_ai.client.inference.usage.{input,output}_tokens` | Counter | {token} | + `gen_ai.token.modality=text` | as above |
| `gen_ai.client.inference.operation.{input,output}_tokens` | Histogram | {token} | as above | as above |
| `gen_ai.invoke_agent.{inference_calls,tool_calls}` | Histogram | {inference_call}/{tool_call} | run outcome | as above |
| `a2a.telemetry.export.failed` | Observable counter | {batch} | `signal` | `srv/otel/telemetry/self_metrics.rs:11,45` |
| `process.cpu.time`, `process.memory.usage`, `process.memory.virtual`, `process.thread.count`, `process.unix.file_descriptor.count`, `tokio.runtime.*` (workers, alive tasks, global queue depth) | Observable | — | none | Linux only; `srv/otel/telemetry/process.rs:194,234`; ADR 0038 §7 |

Client, meter `a2a.client`: one instrument, `rpc.client.call.duration` (`cli/call_metric.rs:59-66`), labels
`rpc.system.name`, `rpc.method`, `rpc.status_code`, `error.type`, **`server.address`, `server.port`**
(`:118-137`). **Finding (VERIFIED by reading, effect CONJECTURED):** `server.address` is whatever peer the
client was built for; nothing bounds the number of distinct values, so a fleet client that discovers or
balances over many endpoints makes one series set per peer. Every other label in the SDK is bounded by a
constant set or an operator allow-list.

### 1.3 Logs

- Events go through `tracing` macros gated on the `tracing` feature (`cli/trace.rs:12-55`; `srv/trace.rs`).
  Targets used for spans: `a2a_protocol_server::rpc`, `::tool_loop`, `::store`, `a2a_protocol_client::rpc`.
- Console formats: text, or JSON with `timestamp, level, target, message, trace_id, span_id, trace_flags`
  via `TelemetryBuilder::with_console_format(ConsoleFormat::Json)` or `A2A_LOG_FORMAT=json` (ADR 0038 §6,
  `docs/adr/0038-observability-0-16.md:159-172`).
- OTLP logs carry the span's trace and span ids (handoff `docs/handoff.md:388-390`, test `telemetry_export`).
- Classes that exist only as log lines (no metric): client retries (`cli/retry.rs:457-629`), balancer endpoint
  refresh/skip (`cli/balance/members.rs:162,173`), ejection transitions (`cli/balance/transport.rs:159`,
  ADR 0050 `:158-160`), delegation lease-renewal failure (`cli/delegation/renewal.rs:72`), WebSocket upgrade
  refusals (handoff `:414-418`).
- Audit trail (`srv/audit/`) is a separate hash-chained record, not telemetry.

### 1.4 Trace and baggage propagation per binding (VERIFIED)

| Binding | Client sends | Server reads | Relation |
|---|---|---|---|
| JSON-RPC, HTTP+JSON, gRPC | `traceparent` naming the call's own CLIENT span (`cli/call_span.rs:406`; opt-out `ClientConfig::with_trace_propagation`, `cli/config.rs:284`) | `InboundTracePolicy` default `Continue` (`srv/handler/helpers.rs:148-164`): remote parent | **parent/child** |
| WebSocket | nothing per frame; one notice per connection that `traceparent` is dropped (`cli/transport/websocket.rs:499`) | the upgrade request's `traceparent`, kept as `:upgrade-traceparent` (`srv/dispatch/websocket.rs:1368`) | **span link**, not parent (`srv/rpc_span/link.rs:5-50`) |
| Push webhook | POST carries the webhook span's `traceparent` (`srv/rpc_span/webhook.rs:85`) | client webhook receiver span | parent/child |
| Baggage | opt-in, allow-listed keys, `ClientBuilder::with_baggage_propagation` | opt-in, `RequestHandlerBuilder::with_baggage_propagation`, only under `Continue`; ≤ 64 members, ≤ 8192 bytes | ADR 0038 §1 (`:31-72`); never a metric label |
| `OTEL_PROPAGATORS` | read when `ClientConfig` is built | read when the handler is built | without `tracecontext`, `Continue` acts as `Restart` (ADR 0038 `:61-68`) |

**How delegation links.** A delegated child is a normal client call made while the executor span is current,
so the child server's `a2a.rpc` span is a child of the parent's `a2a.rpc.client` span, in the same trace
(handoff `:393` "delegation spawns kept in the parent's trace"). ADR 0034 states the tree view "is by trace
id" and that a stored index by root is deferred (`docs/adr/0034-task-lineage.md` decision 5). Lineage
(`a2a.lineage.root`, `a2a.lineage.depth`) is on the executor span only and "no metric label carries either"
(ADR 0034 item 11; `srv/otel/control.rs:6-11`).

### 1.5 Dashboards, rules, alerts shipped (VERIFIED)

- Dashboards (`deploy/observability/grafana/dashboards/`): `a2a-server.json` (19 panels: calls, failed/caller
  errors, span p95 from Tempo, unary p50/p95/p99, errors by kind, unanswered, active event queues, persistence
  errors, push by outcome, HTTP connections, store ops and p95, executor duration and rate), `a2a-client.json`
  (5 panels), `a2a-genai.json` (11 panels: model calls, tokens, tools, runs).
- Recording rules: 22 in `prometheus/rules/a2a-recording.yml:38-155`.
- Alerts: 14 in `a2a-alerts.yml` (`A2AServerErrorRatioHigh`, `A2APersistenceErrors`, `A2APersistenceFailing`
  (the only `critical` besides `A2AAgentSilent`), `A2ARequestsUnanswered`, `A2AQueueDepthGrowing`,
  `A2APushDeliveryTruncated`, `A2APushDeliveryFailing`, `A2AUnaryLatencyHigh`, `A2AAgentSilent`,
  `A2AAgentInstancesLost`, `A2AStoreRetriesHigh`, `A2AExecutorFailureRatioHigh`, `A2AModelCallsFailing`,
  `A2AClientErrorRatioHigh`); 6 in `stack-alerts.yml` (Collector down/export failing/refusing/queue filling,
  Prometheus rule evaluation and notifications failing).
- `promtool test rules` files: `prometheus/tests/{a2a-alerts,a2a-failure-classes,a2a-signals,stack-alerts}.test.yml`;
  run by `.github/workflows/observability.yml:38-78` with `check_stack.py`.
- Kubernetes: `deploy/kubernetes/observability/{prometheusrule.yaml,grafana-dashboards.yaml,kustomization.yaml}`,
  generated; "not applied to a cluster" (ADR 0039 header).
- Alertmanager ships one receiver with no integration (ADR 0039 §4).
- `scripts/check_otel_wiring.py` fails when a binary installs no `Telemetry` or a manifest drops the client's
  `otel` feature (`:5-30`). `scripts/check_otel_metrics_coverage.py` exists (not read).

### 1.6 Gap list (a): open register items, status re-checked against the code

| Id | Register text | Status now | Evidence |
|---|---|---|---|
| H3 | SLIMRPC multicast fan-out has no span or duration | **OPEN** (VERIFIED) | `BindingSpan::open` appears only in `bindings/a2a-protocol-slimrpc/src/server/methods/mod.rs:157-399`; none in `src/multicast/` |
| M6 | WebSocket upgrade refusals and oversize closes logged, not metered | **OPEN** (VERIFIED) | `record_unrouted` callers in `srv/dispatch/websocket.rs` are `:729`, `:809`, `:833`, `:1203` (busy, parse error, invalid request); none on the upgrade path (`:672`, `:995`) |
| M7 | SLIMRPC refusals before the span | **Partly moved** (VERIFIED): `record_unrouted` and `RpcSystem` are now `pub use`d (`srv/rpc_span/mod.rs:86,91`), so the API precondition holds; no SLIMRPC call of it was found by the grep above | needs a check of `bindings/.../server` |
| M8 | Refusals tonic makes itself record nothing | **OPEN** (VERIFIED: no `tower`/`Layer` in `srv/dispatch/grpc/*.rs`) | grep returned nothing |
| M10 | Client retries not visible | **OPEN** (VERIFIED) | only `trace_*` lines at `cli/retry.rs:457-629`; the client has one instrument (`cli/call_metric.rs:61`) |
| M11 | Push-config, audit, halt, lease, retention stores have no DB span | **OPEN** (VERIFIED) | the only `Traced*` type is `TracedTaskStore` (`srv/store/traced.rs:60,194`) |
| M12 | Card fetch, JWKS, token exchange have no duration metric | **OPEN** on the client (VERIFIED: one client histogram); the server's card *serving* has one (`srv/rpc_span/card.rs:284`) | |
| M15 | Operation × binding matrix not asserted for axum `A2aRouter` or SLIMRPC | NOT RE-VERIFIED | handoff `:428-432` |
| L1 | `OTEL_*_EXPORTER=prometheus` panics every binary | **Appears changed** (VERIFIED that `prometheus` is handled per signal and otherwise pushed to a `dropped` list: `srv/otel/telemetry/selection.rs:69-87`); re-test before closing | |
| L3–L12 | no flush on SIGTERM for four binaries, stale `RpcCall::duration` doc, rest | NOT RE-VERIFIED; the list itself is not in the repository (handoff `:400-402`: "the report is not in the repository") | |
| store `error.type` is not SQLSTATE | **OPEN** (VERIFIED by the field comment) | handoff `:382-385` |
| S4 | Independent visibility of telemetry export failure; collector-outage and cardinality qualification | **OPEN** (VERIFIED): `a2a.telemetry.export.failed` exists (`self_metrics.rs:11`) but no rule or dashboard references `a2a_telemetry*` (grep over `deploy/observability`), and it is exported through the pipeline that is failing | `docs/audits/2026-11-12-adversarial-review/findings.json` S4 "Not examined this session"; ROADMAP `:144-145` |
| C-05 / KL-09 | SDK exemplars | **OPEN, upstream** | ADR 0038 `:198-206`; ROADMAP `:150` |
| C-08, NEW-c-5, C-17, NEW-c-2 | `OTEL_*` remainder | OPEN per ROADMAP `:146-147`; not re-verified | |
| W2-LIVE-8 | provider error body reaches `invoke_agent` span events and the log | OPEN per ROADMAP `:132-134`; not re-verified | |
| ADR 0046 "not built" | a gauge for the queue cap in `OtelMetrics` | **OPEN** (VERIFIED: no such instrument in `srv/otel/mod.rs`) | `docs/adr/0046-…md` Not built |
| ADR 0050 "not built" | ejection/hedge state as instruments | **OPEN** (VERIFIED) | ADR 0050 `:158-160`; `HttpPool::origins()` snapshot only |
| ADR 0032 "not built" | gauges of open and idle client connections | **OPEN** | ADR 0032 `:189-190` |

### 1.7 Gap list (b): swarm and fleet observability

| Topic | State | Evidence / note |
|---|---|---|
| Per-swarm / lineage trace view | **Exists only as one trace, and only when every hop is `Continue` and header-carrying.** A WebSocket hop becomes a link, a `Restart`/`Drop` hop or a peer with `with_trace_propagation(false)` splits the trace; a job resumed after a restart (ADR 0030) or handed off (ADR 0054) starts from a different call. `a2a.lineage.root` is on the executor span only, so a backend can re-join split traces by attribute query, but nothing documents or ships that query and the `a2a.rpc` SERVER span and client span do not carry it. | VERIFIED: `srv/rpc_span/agent.rs:171-172` vs field lists at `mod.rs:428-457`, `cli/call_span.rs:73-98` |
| Lineage-aware metrics without cardinality explosion | **Missing by decision.** Only `lineage.depth` would be bounded (depth has a cap, ADR 0034 item 1) and it is not a label either. No "delegations started/ended by outcome" counter exists; a supervisor's fan-out width is not metered. | ADR 0038 §4 `:140-145` |
| Per-tenant cost/token accounting | **Missing.** `with_tenant_dimension` labels the RPC, executor and refusal metrics only (`srv/otel/control.rs:155-171`); the GenAI token counters are built in `tool_metrics.rs:136-176` with no tenant dimension. Budget settlement is deliberately not a hook (ADR 0038 `:143-145`). S3 (fleet inference quota) is OPEN. | VERIFIED |
| GenAI semconv coverage | Targets `semantic-conventions-genai@9ffc7e0` / `4f85037`, all `development`. Present: inference span + duration + token usage/operation histograms, execute-tool span + duration, invoke-agent span + duration + call counts. Absent by decision: content on tool-loop spans (`tool.rs:32-37`: the opt-in "reaches no executor"), `gen_ai.client.operation.duration` (`tool_metrics.rs:25-27`). Not found by this read: request parameters (`gen_ai.request.temperature`, `max_tokens`), time-to-first-token / streaming chunk metrics, cached/reasoning token types, `gen_ai.response.finish_reasons` as a declared field at `tool.rs:100-116` (the doc names it; verify where it is recorded). Whether `9ffc7e0` is still current upstream: **NOT VERIFIED**. | VERIFIED for presence; CONJECTURED for the absences |
| Exemplars | SDK emits none (`opentelemetry_sdk` 0.33.1 builds `exemplars: vec![]`); stack derives them from Tempo span metrics. | ADR 0038 `:198-206` |
| Profiling hooks | **None in the crates.** Only the stable Tokio metrics (3) are exported; `tokio_unstable` poll/steal metrics are not (ADR 0038 `:189-194`). `benches/sdk-comparison/scripts/{callgrind_per_request.sh,memcpy_callers.py}` and an `strace -k` recipe exist; "no `perf` here" (`docs/sdk-comparison-2026-11-11.md:173-174`). | VERIFIED |
| Continuous SLO burn alerts | **None.** All 14 alerts are single-window thresholds with a `for:` (`a2a-alerts.yml:26-248`); no multi-window burn-rate pair, no SLO recording rules (`slo:` / error-budget). | VERIFIED |
| Event-log lag | **No gauge.** The only signal is `persistence_errors{operation="event_log_catchup",error="timed_out"}` after the bounded wait expired (`metrics/labels.rs:46-54`). The broadcast-vs-applied distance (`AppliedMark`, `srv/streaming/event_queue/applied.rs:60-106`) is not exported. | VERIFIED |
| Push outbox depth/age | **No metric.** `ClaimedDelivery.age` exists (`srv/push/outbox.rs:92`) and the worker polls every 1 s in batches of 32 (`outbox_worker.rs:34,38`), but only `push_deliveries{outcome}` is counted. | VERIFIED |
| Lease / halt propagation latency | **No histogram.** Counters only (`lease.events`, `halts`, `halt.stopped_tasks`). One-off numbers exist in the qualification matrix (cancel across replicas 0.86 s / 1.01 s). | VERIFIED |
| Admission queue depth / wait | **No metric.** `srv/admission/*.rs` contains no `metrics` reference (grep); `admission_load()` feeds `/load` and the `endpoint-load-metrics` header only (ADR 0045 §7, ADR 0046 §1-2). A queue-wait refusal is not among the six `refusal` causes (`labels.rs:184-194`). | VERIFIED |
| Store pool saturation | **No metric.** `a2a.server.pool.*` is the HTTP listener (`serve/connections.rs:69`). No `pool.size()`/`num_idle()` read and no `db.client.connection.*` instrument exists in `srv/` (grep). The qualification row "300 concurrent blocking sends against a pool of 2" held, with no signal an operator could have watched. | VERIFIED |
| Per-peer client metrics (ejection, hedge wins, retries) | **Logs only** (ADR 0050 `:158-160`; `cli/retry.rs`). | VERIFIED |
| Telemetry-failure visibility (S4) | In-band counter only; no out-of-band path (log line rate, `/ready` body field, Prometheus scrape of the same counter when OTLP is down). `Telemetry::prometheus()` gives a scrape handle (ADR 0038 §3) which could carry it; not wired into an alert. | VERIFIED |
| Sampling controls | Trace sampling is upstream's `OTEL_TRACES_SAMPLER[_ARG]` with notes for unsupported names (`srv/otel/telemetry/upstream.rs:25-79`); span level floor `with_span_level` (`builder.rs:174`); `InboundTracePolicy::Drop` and the `UNTRACED` task-local suppress per request (`rpc_span/mod.rs:131-146`). No log sampling/rate limiting, no tail-sampling guidance for long-lived streams, no per-tenant sampling. | VERIFIED |
| Sensitive data in telemetry | Content capture opt-in and capped (64 KiB); tool-loop spans carry no content; unknown tool names and methods collapse to `_OTHER`; baggage allow-listed. Open: W2-LIVE-8 (provider error body in span events/log); `jsonrpc.request.id`, `a2a.message.id`, `a2a.task.id`, `gen_ai.agent.description` and peer address are recorded unconditionally on spans; X-18 (CLI says nothing about telemetry). | VERIFIED for the fields; redaction completeness NOT VERIFIED |

---

## 2. Performance

### 2.1 The measured gap to a2a-rs (all numbers quoted from files)

Source: `docs/sdk-comparison-2026-11-11.md`; raw output in `benches/sdk-comparison/results/2026-11-11/`.
Host: Intel Xeon @ 2.10 GHz, 4 vCPU shared VM, load average 3.3 at start (`results/2026-11-11/host.txt`).
Workload: echo agent, closed-loop JSON-RPC, server pinned to 2 cores, loadgen to 2, harness release profile
without LTO, glibc malloc (`:20-29`).

| Cell (blocking `send`) | a2a-rs tuned | a2a-rust `da17606c` | a2a-rust 0.14.1 | Source |
|---|---|---|---|---|
| 1 conn, req/s median [min–max] | 7757 [7527–8121] | 4228 [3586–4823] | 6090 [5606–6278] (table 2) | `:46-48`, `:77-79` |
| 1 conn, server CPU µs/request | 107 | 228 | 157 (table 4) | `:47-48`, `:186` |
| 16 conn, req/s | 10072 | 7790 | 8354 | `:50-51`, `:81` |
| 64 conn, req/s | 9336 | 7496 | — | `:53-54` |
| 256 conn, req/s | 8718 | 6775 | — | `:56-57` |
| 16 conn, one core, req/s (boxed build) | 18203 | 9067 | 9427 | `:104-106` |
| 16 conn, one core, CPU µs/request | 52 | 104 | 100 | `:104-106` |
| RSS MiB, send @ 1–256 conn | 151–232 | 64–98 | — | `:46-57` |
| stream, 16 conn, req/s | 10509 | 6405 | — | `:62-63` |

The document's own verdicts (`:110-126`): a2a-rs serves 1.25–1.9× the blocking-send throughput; this tree was
slower than its own 0.14.1 at one connection; a2a-rust holds 2.5–3× less resident memory; variance on the
host is up to 20% per cell, so "a difference under about 15% [is] unproven unless the ranges are disjoint"
(`:33-37`).

After boxing two futures (the only performance change made, `:216-222`): 4919 req/s and 210 µs CPU at one
connection (`:88-90`); table 4's six interleaved repetitions show 4384 → 4719 req/s with overlapping ranges
(`:186-188`) — "not proven to be more than noise" (`:158-159`).

With a real model in the loop (Qwen3.5-2B, 10 samples): first-token median 197 ms direct, 198 ms a2a-rs,
212 ms a2a-rust; "a sample of ten cannot separate either SDK from the model's own variance" (`:200-212`).

Earlier in-process measurement, same question: `docs/readiness-bar.md:130-146` — 102.0 µs per send at HEAD
`858b21e` vs 106.2 µs at 0.14.1, one worker, one process; default `tracing`+`otel` with nothing installed cost
4.6% and 6.6% of a send (`:166-169`). Corrected at `:186-191`: over two processes on two cores the same
comparison is 232 vs 157 µs.

Other measured facts: `docs/swarm-scale-findings.md:290-302` — at one agent `/health` p50 21 µs vs a send's
206 µs, "about 90% of a send is work behind the dispatcher" (2026-09-20, `391f0df`, 4 cores @ 2.80 GHz);
binding p50 at concurrency one REST 206 µs, WebSocket 481 µs, gRPC 1,817 µs with the caveat that the gRPC arm
includes the full client (`:685-699`).

### 2.2 What the bisect found

Nothing usable. `git bisect` over 1,291 commits since 0.14.1 landed on a commit that moves tests in the
SLIMRPC binding (`e8e7d8bd`); "the signal (a 29% fall in all) is spread over many commits and the noise of
this host is larger than any one of them … is not a finding" (`docs/sdk-comparison-2026-11-11.md:167-171`;
log at `results/2026-11-11/bisect-inconclusive.log`). The earlier per-commit series in
`docs/readiness-bar.md:158-163` agrees: "no single step … stands out of the noise".

### 2.3 Where per-request CPU goes, per the profiles in the repository

- **Syscalls per request** (`strace -f -c`), 0.14.1 / this tree boxed / a2a-rs: `write` 0.11 / 2.2 / 0.04,
  `epoll_wait` 1.1 / 2.7 / 1.1, `futex` 2.0 / 2.1 / 2.0, `sched_yield` 0 / 0.5 / 0, `mprotect` 0.15 / 0.62 /
  0.19 (`:130-132`).
- **Who makes the extra 8-byte eventfd writes** (`results/2026-11-11/strace-k-wakeup-callers.txt`, 257 writes
  in ~2 s at one connection): 76 `Sleep::poll → time::Handle::reregister` inside hyper's HTTP/1 `Conn` (the
  header-read timer, VALIDATED as source / CONJECTURED as the specific timer); 73
  `transition_worker_from_searching`; 50 `reregister` under `tokio::time::timeout` around a
  `watch::Receiver` in `streaming::event_queue::appl…`; 32 + 16 tasks scheduled from another thread; 1 from
  `serve::graceful::idle::IdleTimeout`.
- **Instructions** (callgrind, `results/2026-11-11/callgrind-0.14.1-vs-da17606c.txt`): ~320,000 per send at
  0.14.1, ~604,000 at `da17606c`; 261,000 of the 284,000 added were `memcpy` under `tokio::spawn` wrappers
  moving ~36,000-byte futures four times; ~277,000 after boxing (`:147-155`). The document warns that
  callgrind counts `rep movsb` per byte, so this over-states the gain (`:153-156`).
- **Not established** (`:172-177`): the rest of per-request CPU ("allocation is about 25% of self time by
  callgrind, a `SipHash` map about 4%, glibc `malloc`"); no time profile exists ("no `perf` here"); other
  bindings, a real executor, other hardware, `default-features = false`, 64/256 connections after the change.

### 2.4 Open leads

1. Header-read timer armed and cancelled per request — `srv/serve/graceful/connection.rs:154-156`
   (`.timer(TokioTimer::new()).header_read_timeout(..)`, default 30 s `config.rs:32`). ROADMAP `:154-156`
   asks for "a cheaper bound on a header read". OPEN.
2. `timeout_at` around the `watch` receiver — `srv/streaming/event_queue/applied.rs:103`
   (`wait_through`), called from `InMemoryQueueWriter::settled` at `in_memory.rs:310-320` once per turn.
   OPEN.
3. Second-worker wake-ups (73 + 48 of 257): a blocking send is two spawned tasks plus the request future
   (2.5 below). The request-future-collects design was measured and rejected at −21% (`701b02a1`,
   `docs/sdk-comparison-2026-11-11.md:163-166`). OPEN, no accepted design.
4. Allocation (≈25% of self time) and the SipHash map (≈4%). OPEN, unattributed.
5. ROADMAP Performance items 2–4 (`:157-161`): audit appends at several replicas (E-F18), lease groups after
   a 10^4–10^5 leased-task measurement (H5), blob offload / SQL holds / SQL `save_appending_history` (H13).

### 2.5 Hot path, blocking JSON-RPC `SendMessage` (hops VERIFIED; cost candidates CONJECTURED)

| # | Hop | File:line | Candidate cost (CONJECTURED unless noted) |
|---|---|---|---|
| 1 | accept; `set_nodelay`; hyper `auto` builder with timer; `IdleTimeout` wrapper holding a boxed `Sleep` | `srv/serve/graceful/connection.rs:36,143-160`; `idle/mod.rs` | per-request header timer register/cancel → driver wake (VALIDATED as a wake source, §2.3) |
| 2 | `Dispatcher::dispatch` boxes the dispatch future; inner future boxed again | `srv/dispatch/jsonrpc/mod.rs:489`, `:145` | two heap allocations of a ~16 KiB future (comment at `:141-144`) |
| 3 | every request header copied into `HashMap<String,String>` (SipHash), each value joined through a `Vec<&str>`; `:path`, peer address and certificate inserted | `srv/dispatch/jsonrpc/mod.rs:185-190`; `srv/dispatch/mod.rs:303-333` | 2 allocations per header + map growth; candidate for the "SipHash map about 4%" |
| 4 | `requested_extensions` read as an owned `String` before dispatch, header value formatted after | `mod.rs:138-153` | allocation per request even with no extensions |
| 5 | body read with limit and timeout | `mod.rs:193-198`; `response.rs:217` | a `timeout` per request body (timer only if pending) |
| 6 | `serde_json::from_slice::<JsonRpcRequest>`; `params` is `Option<serde_json::Value>` | `mod.rs:222`; `crates/a2a-protocol-types/src/jsonrpc.rs:184-198` | builds a `Value` tree for the whole message (one allocation per string/object) |
| 7 | `parse_params`: typed deserialize from the borrowed tree, then a second walk for unknown keys | `srv/dispatch/jsonrpc/response.rs:53-67`, `:86-101` | every string copied a second time; pass two over the top-level keys |
| 8 | `ServerSpan::open_jsonrpc` (30 declared fields) and `run` | `srv/rpc_span/mod.rs:229,285,428-457` | span construction even when nothing records is "a level check"; measured cost of default telemetry 4.6–6.6% (`readiness-bar.md:166-169`) |
| 9 | `on_send_message`: `metrics.on_request`, tenant resolution, `TenantContext::scope_request`, boxed inner future | `srv/handler/messaging/mod.rs:80-139` | with OTel metrics: 4 recordings per send (`on_request`, `on_response`, `on_latency`, `on_rpc_call`), each allocating `String`s and a `Vec<KeyValue>` (`srv/otel/mod.rs:325-417`); `a2a.server.request.duration` duplicates `rpc.server.call.duration` |
| 10 | `build_call_context`, interceptor chain `begin`/`admit` | `mod.rs:150-157` | header map cloned into the context? (not read) |
| 11 | `validate_and_commit`: tenant slot, validation, admission slot | `srv/handler/messaging/commit.rs:35-76` | 2–3 awaits |
| 12 | `commit_task`: `resolve_context_id` → `keyed_lock` (per-context async mutex) → `stored_for` (store read) → `resolve_task_id` → `settled_task` → `gate_decision` (boxed) → `claimed_view` (boxed) | `commit.rs:102-152` | ≥ 7 awaits before any work; store `list`+`get` for a continuation (measured: `find_task_by_context` 511 µs at the history cap before ADR 0017, `swarm-scale-findings.md:440-451`) |
| 13 | inside the boxed `started` future: `refuse_if_halted`, `reject_in_flight_send`, `build_initial_task`, `build_request_context`, `resolve_references`, `lease_event_queue` (creates broadcast + mpsc channels), `register_cancellation_token`, `save_turn` (store write), `register_inline_push_config` | `commit.rs:188-310` | 8 more awaits, several async-mutex or map locks, channel allocation per turn, `task_id`/`context_id` clones throughout |
| 14 | executor: `spawn_executor` on a multi-worker runtime, `defer_executor` on one worker | `commit.rs:331-340`; `srv/handler/messaging/execute.rs:221-245,279-323`; `start.rs:17-19` | spawn #1 → cross-thread wake; future boxed once (the 2026-11-11 change) |
| 15 | each executor event: order lock, position, size measured by serializing into a counting writer, broadcast send, `try_hand_off` to the persistence mpsc | `srv/streaming/event_queue/in_memory.rs:713-723`, `:562-567`, `:97-110` | one full serde pass per event solely to measure size; an async mutex per event |
| 16 | `respond_blocking`: drops the reader, creates a oneshot, boxes the collector, spawns it in the current span, awaits the oneshot | `srv/handler/messaging/respond.rs:202-275` | spawn #2 + oneshot → two more cross-thread wakes per request |
| 17 | collector folds events and saves (status delta / artifact delta / event-log append per event) | `srv/handler/event_processing/sync_collector.rs`; store writes via `TracedTaskStore` (`srv/store/traced.rs:97-124`) | per store call: `format!` for the span name and a `Vec<KeyValue>` with cloned strings (`traced.rs:101,144-156`) |
| 18 | executor returns → `settled()` waits for the applied mark with `timeout_at` | `in_memory.rs:310-320`; `applied.rs:99-106` | timer registered whenever the collector has not yet applied the last event → driver wake (50 of 257 in §2.3) |
| 19 | `hydrate_response_history` (store `get` only when history was asked for), shape, sign | `respond.rs:296-299`; `mod.rs:110` | — |
| 20 | `success_response_bytes`: `serde_json::to_value(result)` then `to_vec(&envelope)` | `srv/dispatch/jsonrpc/response.rs:23-40` | the result is serialized twice (tree, then bytes); a `RawValue`/borrowing envelope would make it one pass |
| 21 | `json_response` → `Full<Bytes>` boxed body; extension header; CORS | `response.rs:254-264`; `mod.rs:146-157` | — |

Await count on the request future before the executor starts: at least 15 (hops 9–13). Tasks per blocking
send on a multi-worker runtime: request future + executor + collector.

### 2.6 Hot path, `SendStreamingMessage` (differences from 2.5)

| # | Hop | File:line | Candidate cost (CONJECTURED) |
|---|---|---|---|
| S1 | same hops 1–15; routed at `mod.rs:322-342` with `run_with` | `srv/dispatch/jsonrpc/mod.rs:322-342` | — |
| S2 | `respond_in_background`: `holding_log`, spawn the background event processor, `task.clone()` for it, three reader hooks boxed (`with_catch_up`, `with_final_snapshot`, `with_closed_early`), provisional snapshot | `srv/handler/messaging/respond.rs:127-190` | spawn #2; a whole-`Task` clone; three boxed closures per stream |
| S3 | `build_sse_response`: mpsc channel of frames, spawned forwarder in an `a2a.sse` child span, `yield_now`, keep-alive `sleep` | `srv/streaming/sse.rs:389-511` (`:398`, `:412`, `:422`, `:511`) | spawn #3; every event crosses broadcast → forwarder → mpsc → hyper body: two channel hops and two wakes per event |
| S4 | each event serialized for the SSE frame through a thread-local buffer | `sse.rs:65-93`, `:437` | second serde pass of the same event (first was the size check, hop 15) |
| S5 | background processor: log append, fold, save per event; SQL stores serialize the event a third time | `srv/handler/event_processing/background/processor.rs:164-355`; `state_machine.rs` | per event: 1 append statement + 1 delta write on SQL (statement counts in §3.4) |
| S6 | push delivery lanes / outbox reservation when a config exists | `…/background/push_delivery/` | off the stream's path (F1 measured: 0.23 s vs hang, `readiness-bar.md:181-184`) |

Tasks per streaming send: request future + executor + background processor + SSE forwarder (+ hyper's
connection task). Measured stream throughput gap at 16 connections: 6405 vs 10509 req/s (§2.1).

### 2.7 Benchmarks that exist, what the gate compares, what is not benchmarked

- Criterion suite `benches/benches/` — 15 targets (`benches/README.md:42-60`): transport throughput
  (JSON-RPC, REST, SSE drain, payload to 1 MB), send latency breakdown, protocol overhead (serde), task
  lifecycle, concurrent agents (1/4/16/64), cross-language (this SDK only), realistic workloads, error paths,
  backpressure (stream volume to 502 events, SQLite append volume), data volume (1K–100K tasks in memory),
  memory overhead (allocations per ser/de), enterprise, production (incl. `production_cold_start/first_request`
  475.2 µs vs `steady_state` 234.7 µs, `book/src/reference/benchmarks.md:406-407`), advanced scenarios,
  coordinator chain under fault (5 hops). `benches/Cargo.toml:18` enables only `sqlite` on the server crate.
- Regression gate: `.github/workflows/benchmarks.yml:267-437` runs `transport_throughput` and
  `protocol_overhead` on base and on the PR on one runner and fails when the 95% CI lower bound of the median
  change exceeds **50%** (`--threshold 0.50`, `:419-421`; `benches/README.md:332-338`), `*/from_str/16384`
  excluded. Push to `main` runs 13 targets and regenerates the book page (`:76-122`).
- Out-of-process harness: `benches/sdk-comparison/` (`scripts/run_perf.sh`, `run_mem.sh`, `llm_e2e.py`,
  `callgrind_per_request.sh`); pinned `=0.14.1` in the committed manifest (handoff `:449-452`). Not in CI.
- Test-shaped measurements: `crates/a2a-protocol-server/tests/{readiness_bar/,swarm_scale/,soak.rs,soak_multi_replica.rs,statement_count.rs,event_byte_budget.rs,push_hang_live_subscriber.rs,log_wake_e2e.rs,subscribe_no_idle_bound.rs}`,
  `handler/context_lookup_cost.rs`, `crates/a2a-protocol-sdk/tests/swarm_binding_grpc.rs`.
- Soak: `.github/workflows/soak.yml` nightly (`cron: '23 2 * * *'`, 90-minute job limit), `tests/soak` and a
  PostgreSQL soak (`:97-110`).

**Not benchmarked** (VERIFIED against the module table and the results directories unless noted):

- gRPC and WebSocket under the criterion suite or the out-of-process harness (one p50 each exists from
  2026-09-20, §2.1). No gate covers them.
- PostgreSQL in any benchmark; SQLite only in `backpressure` append volume. No gate covers a store.
- The client library on its own (retry, balancing, hedging, pool) — only as the driver of server benchmarks.
- Delegation fan-out (one supervisor → N children, lease renewals, journal writes).
- Process cold start (exec → first answered request, with and without PostgreSQL schema check). The existing
  "cold start" row is a fresh in-process server per iteration.
- Memory per idle task, per parked task, per open SSE/WebSocket/gRPC stream, per idle subscriber. Existing
  points: RSS 64–98 MiB under load (§2.1); "about 33 MiB" per replica after 10,000 tasks over 3 replicas
  (`qualification-matrix.md`); C1 56–58 B growth per request (`readiness-bar.md:232`).
- Multi-worker out-of-process throughput in CI: the gate's in-process single-runtime shape is the one
  `sdk-comparison` table 3 shows "hides the cost" (`:113-117`).
- TLS, HTTP/2, `default-features = false`, payload sizes above the echo, tenants, audit, encryption, content
  signing on the send path.
- 64/256 connections and the streaming matrix after the boxing change (`:176-177`).

---

## 3. Scaling

### 3.1 Measured (with date, hardware, source)

| What | Number | Date / host | Source |
|---|---|---|---|
| L1, 8 clients blocking send, JSON-RPC, in-memory, one process | 13,224 / 13,562 / 13,181 req/s; p99 1.155 / 1.083 / 1.163 ms; p50 ≈ 0.58 ms; 0 errors of ~400k | 2026-11-12, Xeon @ 2.30 GHz 4 vCPU shared, release with LTO off, `ba335f90` | `docs/readiness-bar.md:196-239` |
| R2 drift | 1.09 / 1.10 / 1.01 | same | `:229` |
| C1 RSS growth per request | 57.1 / 56.3 / 58.0 B (210 B in a 5 s run) | same | `:232`, `:243-246` |
| C2–C5 | fds 11→11, tokio tasks 1→1, 0 non-terminal, 0 queues | same | `:233-236` |
| R4 overload | 4 served, 12 refused, 0 other; refusal is HTTP 200 with JSON-RPC `-32603` + `RetryInfo` | same | `:237`, `:247-250` |
| F1 (push target hung) | stream ends in 0.23 s; inline push fails the test | 2026-11-11 | `:181-184` |
| One channel, 1,000 agents | peaks 9,377 posts/s at 4 writers; 522 posts/s, p50 1.31 s at 1,000 | 2026-09-20, 4 cores @ 2.80 GHz, in-memory, `391f0df` | `docs/swarm-scale-findings.md:59-72` |
| Sharded by context | 3,299 posts/s at 1,000 contexts; 2,592 at 64 (16 agents/channel) | same | `:190-207` |
| Live tail fan-out | 1,000 tails on one channel receive every post when turns outlast 250 ms | same | `:220-234` |
| Burst of 1,024 events in one turn | every tail ended by `streamLagged`, zero gaps | same | `:245-262` |
| Card fetch by 512 fresh clients | p50 77.6 ms, p95 1.04 s | same | `:639-648` |
| Store cost per send at 300 posts | in-memory ~1.7 ms, SQLite 12.3 ms, PostgreSQL 22.9 ms | same | `:588-599` |
| Status delta, 100 events × 200 messages | memory 639 µs, SQLite 37.2 ms, PostgreSQL 120.1 ms | same | `:504-508` |
| Lease checks | 150 → 3 store reads per three intervals for 50 leased turns | 2026-11-04 | `docs/adr/0044-…md` Measured |
| Reaper query, 500,000 rows / 2,000 tenants | index scan, 1,215 buffers, 3.0 ms | PostgreSQL 16.15 | ADR 0044, reaper section |
| History append WAL, 1.04 MB history | 200,712 B (`jsonb_set`) vs 190,448 B (whole document) | PostgreSQL 16.15 | ADR 0044, last section |
| Cross-replica log wake | 260 ms (8 s wait ended) | one run | ADR 0057 `:112-116` |
| Replica killed holding 20 of 60 claims | all 60 terminal within 40 s | 2026-11-14, one 4-vCPU host, PostgreSQL 16 | `book/src/reference/qualification-matrix.md` |
| 30 replicas (pool 1) on one database | all up; 300 tasks completed | same | same |
| 10,000 tasks over 3 replicas | held; ~33 MiB RSS each | same | same |
| 300 concurrent blocking sends, pool of 2 | all answered | same | same |
| DB link cut 20 s past the 15 s claim TTL | own turn stopped at 13.1 s; reaper failed the task at 16.0 s | PostgreSQL 17 in Docker | same |
| Rolling upgrade 0.15 → 0.16 | 4 of 4 scenarios; cancel across versions 1.01 s | same | same |
| PostgreSQL round trips per send | 8 (new task), 11 (continuation) | CI constant | `crates/a2a-protocol-server/tests/statement_count.rs:46-48` |
| Layer-4 balancer + one HTTP/2 client | every call lands on one backend | property test | ADR 0050 context; `tests/balancing/premise.rs` |

### 3.2 Required by the readiness bar and unmeasured

- **R5, R6, R7, F2, F3, F4, F5; every row under L2 and L5; every row on gRPC, HTTP+JSON, WebSocket; every
  row on SQLite or PostgreSQL; every row on two replicas** — `docs/readiness-bar.md:261-263` and the
  machine-readable block `:288`.
- **F1: the document disagrees with itself.** `:181` says "**F1 is measured**" (2026-11-11); `:88` ("R5, R6,
  R7 and F1 to F5 have not been measured"), `:262` and the `not_measured` array at `:288` list F1 as not
  measured. `:193` ("F2 to F5 are still unmeasured") is consistent with `:181`. The block at `:265-291` is
  copied into the release evidence manifest by `scripts/release_evidence.py`, so the published evidence
  understates. Also only F1's first half is measured ("retried within its budget and then dropped with a log
  line … was not re-measured", `:183-184`).
- Qualification "Not covered" (`qualification-matrix.md`, last section; ROADMAP `:68-73`): more than one
  host; SQLite under the faults; PostgreSQL rollback 0.16 → 0.15; audit and push-config stores across the
  upgrade; disk full, OOM kills, clock skew; **60 replicas at once**; a provider other than llama.cpp.
- ROADMAP `:158-159`: 10^4–10^5 leased tasks per replica (H5); `:157`: audit appends at several replicas
  (E-F18, "conjecture, not measured").
- Kubernetes: never run ("`kind` could not start a pod sandbox here", handoff `:316-319`).

### 3.3 Known ceilings and defaults (VERIFIED in code or records)

| Ceiling | Value | Source |
|---|---|---|
| Concurrent turns (event queues) per replica | 1,024 (`DEFAULT_MAX_CONCURRENT_STREAMS`) | ADR 0045 context |
| Events buffered per queue / max event size | 256 / 16 MiB; product unbounded unless `with_max_buffered_event_bytes` is set (default `None`) | `srv/streaming/event_queue/mod.rs:97,100`; ADR 0047 A |
| Subscribers | 256 per task, 10,000 per replica | ADR 0044, Subscribers |
| In-memory live log per task | 512 events | `srv/store/task_store/config.rs:53` |
| History per task | 1,024 messages (`MAX_TASK_HISTORY_MESSAGES`) | `swarm-scale-findings.md:327-328` |
| Listener `max_connections` | unset by default; `ServeConfig::production` only warns | ADR 0046 §5 |
| Writers per channel (task) | throughput peaks at 4; operating point 4–16 | `swarm-scale-findings.md:204-207` |
| Single-writer refusal and context lock | per replica only; two replicas both admit a continuation unless turn claims are configured | `swarm-scale-findings.md:601-627`; claims: `srv/store/claim/mod.rs` |
| `max_concurrent_tasks`, admission gate, rate limits | per replica (E-F12; ADR 0045 Consequences); a shared PostgreSQL rate limiter exists at `srv/rate_limit/shared/postgres.rs` (not read) | register E-F12 |
| PostgreSQL connections | 97 usable by default; a replica opens task-store pool + 2 (push pool fixed at 2) (A15); **60 replicas × 2 = 120 > 97, not run** | `qualification-matrix.md`; `examples/deploy-agent/src/main.rs:104,329`; register A15 |
| Audit pool | fixed `max_connections(10)` | `srv/store/postgres_store/pool.rs:20-25` |
| Turn-claim TTL / renewal | 15 s / 1 s, one statement per replica per renewal | `srv/handler/claims/mod.rs:59,63`; `srv/store/claim/mod.rs:29` |
| Halt poll | 1 s per handler, plus LISTEN/NOTIFY push | `srv/handler/halt/sync.rs:44`; `srv/store/halt/postgres.rs:161-193` |
| Subscribe reattach poll | 250 ms backing off to 4 s; optional NOTIFY wake | ADR 0057 `:19-22` |
| Push outbox worker | poll 1 s, batch 32, lease 30 s renewed every 10 s | `…/push_delivery/outbox_worker.rs:34-66` |
| Lease renewal | one `GetTask` per child per ttl/3 (status-only answer) | ADR 0044 decision 1 |
| Card cache | 1,024 URLs LRU, 10% jitter | ADR 0051 `:87-94` |

### 3.4 Store bottlenecks (read from the SQL)

- **Schema.** `tasks(id PK, context_id, state, data JSONB, created_at, updated_at)` plus `applied_seq`
  (`srv/store/postgres_store/mod.rs:230-241`); indexes `idx_tasks_context_id`, `idx_tasks_state`,
  `idx_tasks_context_id_state`, `idx_tasks_updated_at(updated_at DESC, id DESC)` (`:249-254`).
  `task_events(task_id, seq, payload JSONB, created_at, PK(task_id, seq), FK → tasks ON DELETE CASCADE)`
  (`postgres_store/event_log.rs:28-35`). Tenant variants add `tenant_id` as the leading column
  (`tenant_postgres_store/mod.rs:153-187`). Outbox: `a2a_push_outbox` with four indexes including partial
  ones (`srv/push/outbox/postgres.rs:34-66`).
- **Whole-document rewrites.** A task is one JSONB document; every status or artifact delta rewrites the row
  (TOAST included), and `save_appending_history` is the trait default on both SQL stores (read, append, write
  whole) — O(history) twice per send (ADR 0047 "not built"; ADR 0044 last section: 190–200 KB of WAL per
  appended message at 1.04 MB of history). Measured consequence: PostgreSQL 9.5 → 22.9 ms per send over 300
  posts (§3.1).
- **Four secondary indexes on a hot row.** Every state change updates `state` and `updated_at`, so
  `idx_tasks_state`, `idx_tasks_context_id_state` and `idx_tasks_updated_at` are written on each transition
  and HOT updates are impossible. `idx_tasks_context_id` is a prefix of `idx_tasks_context_id_state`.
  CONJECTURED: dropping the redundant index and/or a partial index on live states reduces write
  amplification; ADR 0044 already weighed a partial index for the reaper and declined it at 500k rows.
- **Round trips.** 8 per new task, 11 per continuation (`tests/statement_count.rs:46-48`), each fenced by a
  claim subquery (`fence_sql!`, `event_log.rs:41-46`). One append per event (`APPEND_SQL`), not batched; a
  turn of N events is ≥ N appends + N delta writes. CONJECTURED: batching the appends of events already
  queued in the persistence channel would cut round trips for bursty executors without changing ordering.
- **LISTEN/NOTIFY.** Halt store: always (`halt/postgres.rs`), pooler caveat documented (`:50`). Event log:
  opt-in `LogListen::Pool|Direct`, `pg_notify` folded into the append as a CTE so the statement count is
  unchanged (`event_log.rs:52-66`; ADR 0057 §4-6). Not announced: status saves — a `CancelTask` of a parked
  task is seen by another replica's stream only by the poll (2.01 s at a 2 s ceiling, ADR 0057 `:122-125`;
  register X-11 OPEN). NOTIFY takes a global commit-order lock in PostgreSQL; cost at high append rates NOT
  MEASURED (ADR 0057 `:92` names it).
- **Pools.** Task store pool sized by the caller, no ping before acquire (`pg_retry.rs:139-141`); push pool
  fixed 2; audit 10. No `acquire_timeout`/`min_connections` tuning visible in `pool.rs`; readiness checks the
  task pool only (A15, KL-18).
- **Event-log growth and retention.** SQL logs grow without bound for a live task unless
  `RetentionPolicy::max_events_per_live_task` is set and `purge_expired` is scheduled by the deployment
  (ADR 0047 C); terminal tasks are purged by age only when the deployment calls the sweep ("the SDK's
  PostgreSQL stores keep everything for ever unless something deletes", `examples/deploy-agent/deployment.yaml`
  comment on `TASK_RETENTION_HOURS`). No partitioning of `task_events`; purge is `DELETE` (bloat/vacuum cost
  NOT MEASURED).
- **Fan-out limits.** Broadcast ring of 256; a subscriber that lags gets `streamLagged` and resumes from the
  snapshot (`swarm-scale-findings.md:255-262`); SQL stores have no holds (ADR 0047 C "not built").
- **Memory per task/stream.** NOT MEASURED directly (see §2.7).

### 3.5 Backpressure end to end (VERIFIED by reading the records)

client → listener `max_connections` (unset by default) → rate limit (per replica) → tenant
`max_concurrent_tasks` (`try_acquire`, refuse) → optional `Admission` gate (bounded wait, DRR, priority) →
queue cap 1,024 (refuse `Overloaded`) → optional byte budget (new turns refused under 10% free; writers wait
`write_timeout` 5 s then fail the task) → persistence mpsc (writer waits `write_timeout`) → store (retried for
`store_write_retry` 30 s, then the event is **lost and the status reverted**, X-10 OPEN,
`srv/handler/limits/mod.rs:48`, handoff `:49-51`) → SSE mpsc → slow reader → `streamLagged`. The client side:
`Overloaded` carries a retry delay; retries off by default; retry budget, in-flight cap and ejection opt-in
(ADR 0050). Refusal on JSON-RPC is `-32603`, indistinguishable from an internal error to the alert rules
(ADR 0039 "Known gap").

### 3.6 Multi-region

**None.** No ADR, document or code path addresses more than one PostgreSQL primary, replication lag,
region-affine claims or cross-region NOTIFY; the qualification matrix states "no network between replicas".
Claims fence on the database's clock for PostgreSQL and the host clock for SQLite (E-F26). Anything
multi-region is NOT MEASURED and undesigned.

### 3.7 Shard-key guidance

Shard by **context**, 4–16 writers per channel, never by task; never post without a `taskId`
(`docs/swarm-scale-findings.md:565-577`). The guidance is per replica: the lock that enforces it is
in-process (`:619-624`), so across replicas it holds only with turn claims. No guidance exists for
partitioning the database itself (tenant, context hash).

### 3.8 Kubernetes manifests

- `deploy/kubernetes/` contains only observability objects. The only workload manifest is the example
  `examples/deploy-agent/deployment.yaml`: Deployment (2 replicas, `maxSurge 1 / maxUnavailable 0`,
  topology spread, startup/liveness `/health`, readiness `/ready`, `terminationGracePeriodSeconds: 60`
  with the arithmetic in comments, `DRAIN_DELAY_SECONDS=5`, `TOKIO_WORKER_THREADS=2`, jemalloc arenas,
  restricted security context), Service, PodDisruptionBudget `maxUnavailable: 1`.
- **Missing** (VERIFIED by reading the file and `git ls-files`): HorizontalPodAutoscaler (and nothing exports
  the `/load` utilization as a metric an HPA could read — ADR 0046 "not built: a gauge for the queue cap");
  NetworkPolicy; ServiceMonitor/PodMonitor; a headless Service for the client-side balancer of ADR 0050;
  `preStop` hook (the drain is in-process via `DRAIN_DELAY_SECONDS`); PgBouncer guidance in the manifest
  (LISTEN caveat).
- **ADR 0054 hand-off is not wired in the example**: no `hand_off`, `with_load_reporting`,
  `with_saturation_readiness`, `with_admission`, `LogListen` or byte-budget call appears in
  `examples/deploy-agent/src/` (grep); the manifest's comment still describes claims "left to expire (15 s,
  then another replica takes the tasks over)". Whether the default shutdown path performs the hand-off
  without opt-in was not read.
- Never applied to a cluster (handoff `:316-319`; ADR 0039 header).

---

## 4. Proposed target sheet (Phase 2)

All thresholds are **PROPOSED**; none is a measurement. "Container" = the 4-vCPU / 15-GB shared cloud
container. Rule used: the container can measure *counts, ratios within one run, correctness under faults and
memory*; it cannot measure *absolute latency or throughput tails* (recorded variance up to 20% per cell and
2× between consecutive runs of one build, `docs/sdk-comparison-2026-11-11.md:33-37`).

### Scale

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Hardware | Container? |
|---|---|---|---|---|---|
| S-1 turns per replica | concurrent running turns admitted with 0 non-overload errors | 1,024 (the shipped cap) on every binding and on PostgreSQL | extend `tests/readiness_bar/` with an L4-style arm at the cap; `scripts/live/burst_live.py` for PostgreSQL | 4 vCPU | Yes (count/correctness) |
| S-2 idle streams per replica | open SSE subscribers idle for 10 min, RSS and store statements/s | 10,000 (the shipped `max_subscribers`); statements/s ≤ 10,000 × 2 / 4 s from ADR 0057's "one poll of two statements every 4 s", and ≤ 1/s per replica with `LogListen` | new harness (none opens 10k streams); count statements with `scripts/live/pgcount.py` | 4 vCPU, `ulimit -n` ≥ 32k | Yes for memory and statement counts |
| S-3 replicas on one PostgreSQL | replicas started at once, tasks completed, connections used | 60 replicas (the unrun row) with a stated pooler or `max_connections` setting; then 100 | `c1_chaos.py` from the qualification evidence (`docs/audits/2026-11-14-qualification/`) with pool 1 + PgBouncer, or raised `max_connections` | 4 vCPU suffices for start-up correctness; ≥ 16 vCPU for throughput | Partly (30 ran here; 60 needs the connection plan, not more CPU) |
| S-4 leased children per supervisor | renewals/s and server CPU at 10^4 leased tasks | renewals/s = children × 3 / ttl exactly (ADR 0044), lease-store reads = 1 per interval per replica | extend `handler::lease::cost_tests` (paused clock) to 10^4; live arm in `scripts/live/delegation/` | any | Yes (counts) |
| S-5 fan-out per task | subscribers per task receiving every event of a turn that outlasts the reattach interval | 256 (shipped cap), 0 gaps | `tests/swarm_scale/` tail arm, add gRPC and WebSocket | 4 vCPU | Yes |
| S-6 event-log size | rows and bytes per live task under `max_events_per_live_task` after N sweeps | exactly `max` rows after each sweep (ADR 0047 C contract) at 10^5 live tasks; sweep duration recorded | new: populate + `purge_expired` timing on PostgreSQL | 4 vCPU, 20 GB disk | Partly (disk allowance) |

### Failure detection and recovery

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| F-1 replica SIGKILL mid-turn | time until every held task is terminal or resumed | ≤ 3 × claim TTL = 45 s (the register's "three claim lifetimes", ROADMAP `:78-79`; measured 40 s) | `c1_chaos.py` kill row | Yes |
| F-2 PostgreSQL restart under load (bar F2) | calls failing during, succeeding after, events lost | 0 lost events for outages < `store_write_retry` (30 s); for longer, a *counted and alerted* loss until X-10 is fixed | `scripts/live/restart_live.py`, `terminate_live.py` | Yes |
| F-3 resume on the other replica (bar F3) | events after `Last-Event-ID` delivered exactly once | 0 missing, 0 duplicated, on SSE/gRPC/WebSocket | `tests/multi_replica.rs` + `scripts/live/replica/` | Yes |
| F-4 peer gone mid-stream (bar R7/F4) | time until queue, token and connection are released | ≤ 30 s SSE keep-alive; ≤ `idle_timeout` WebSocket; immediate on gRPC channel close (the bar's own wording) | `tests/readiness_bar/` L3 extended per binding | Yes |
| F-5 graceful shutdown (bar R6) with ADR 0054 | streams receiving a terminal event or hand-off mark within `completion_grace` + 1 s; continuations succeeding elsewhere | 100% of streams; continuation after 1, 2, 4 s back-off (ADR 0054 `:105-106`) | `tests/graceful_shutdown_bindings/`, `scripts/live/upgrade_live.py` | Yes |
| F-6 halt propagation | time from halt call on replica A to executor stopped on replica B | ≤ 2 × halt poll interval = 2 s without NOTIFY; p99 ≤ 500 ms with it (rationale: measured cross-replica cancel 0.86–1.01 s) | `tests/halt_e2e.rs` on two processes + a new histogram (work item O-3) | Yes for the bound; No for p99 |
| F-7 telemetry outage | request success rate and RSS with the collector down or black-holed for 10 min | 0 failed requests, RSS growth ≤ exporter queue bound, `a2a.telemetry.export.failed` visible out of band | `deploy/observability/scripts/drive.sh` + stop the collector container | Yes |
| F-8 slowloris and malformed input (bar F5) | refusal within the binding's bound, RSS bounded | header read ≤ 30 s; body ≤ configured limit; RSS growth ≤ limit × connections | `scripts/adversarial/`, campaign `hostile/` | Yes |

### Latency

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| L-1 server CPU per blocking send, echo, 2 server cores, 1 conn | µs CPU / request, median of ≥ 6 interleaved repetitions, with range | ≤ 157 µs (0.14.1's own figure, table 4) as the first gate; ≤ 120 µs (a2a-rs tuned, table 2 after) as the stretch | `benches/sdk-comparison/scripts/run_perf.sh` with the pin moved | **CPU time per request: marginally** (the document calls it "good to a few percent"); throughput and p99: **No** — needs a dedicated or bare-metal host, or CI's `ubuntu-latest` only for relative A/B |
| L-2 wake-ups per request | eventfd `write` calls per request under `strace -f -c` | ≤ 0.2 (0.14.1 measured 0.11, a2a-rs 0.04, this tree 2.2) | the strace recipe of §2.3 | **Yes** (a count, not a time) |
| L-3 instructions per request | callgrind Ir per blocking send, memcpy share reported separately | ≤ 320,000 (0.14.1) | `benches/sdk-comparison/scripts/callgrind_per_request.sh` | **Yes** (deterministic) |
| L-4 allocations per request | allocations and bytes per blocking send and per streamed event | PROPOSED after a first count; gate at +5% (the tolerance `memory_overhead.rs` already uses) | new bench arm using the counting allocator already in `benches/` | **Yes** |
| L-5 R3 on every binding and store | p99 of a trivial blocking send, 8 clients | the bar's < 25 ms, kept; add PostgreSQL < 100 ms (rationale: 22.9 ms p50-class cost at 300 posts in finding 7, ×4 headroom) | `tests/readiness_bar/` parameterised | Only as pass/fail against a loose bar |
| L-6 PostgreSQL round trips per send | statements per new task / continuation / streamed event | ≤ 8 / ≤ 11 (current constants), then lower as work lands; per event ≤ 2 | `tests/statement_count.rs` (needs `PGSSLMODE=disable`, handoff `:76-79`) | **Yes** |
| L-7 time to first event, streaming | request sent → first SSE frame, p50 | PROPOSED after measurement; with a real model the SDK overhead must stay inside the model's variance (`:210-212`) | `scripts/llm_e2e.py` with ≥ 100 samples | p50 marginal; tails No |

### Memory

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| M-1 per completed request | RSS growth per request (bar C1) | ≤ 1 KiB kept; report the measured 56–58 B | `tests/readiness_bar/` | Yes |
| M-2 per parked task (in-memory store) | bytes per `input-required` task with a 10-message history | PROPOSED after first measurement | new arm with the counting allocator from `tests/event_byte_budget.rs` | Yes |
| M-3 per idle subscriber stream, per binding | heap bytes per open idle SSE / WebSocket / gRPC stream | PROPOSED after first measurement; budget 10,000 × value ≤ 25% of the pod limit | new | Yes |
| M-4 buffered events | peak heap with a stalled subscriber, budget set | ≤ budget + one event (ADR 0047 A) | `tests/event_byte_budget.rs` | Yes |
| M-5 replica at rest and after 10^4 tasks | RSS | ≤ 64 MiB at rest (the manifest's request), ≤ 128 MiB after 10^4 (measured ~33 MiB) | `benches/sdk-comparison/scripts/run_mem.sh`; qualification row | Yes |

### Cold start

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| C-1 process start to first 200 on `/ready`, in-memory | wall time, median of 20 | PROPOSED after measurement | new script: exec `deploy-agent`, poll `/ready` | Median only |
| C-2 the same on PostgreSQL with an existing schema, and 30/60 replicas at once | wall time; schema-check statements | all replicas ready within the startup probe budget 90 s (`deployment.yaml` 2 s × 45) | `c1_chaos.py` 30-replica row, timed | Yes for the bound |
| C-3 fleet first contact | card fetch p95 for 512 fresh clients | ≤ 1 s (measured 1.04 s p95) with a shared pool / cache (ADR 0051) | `tests/swarm_scale/` card arm | Bound only |

### Telemetry cardinality

| Target | Quantity | PROPOSED threshold and rationale | Procedure | Container? |
|---|---|---|---|---|
| T-1 server series per replica | active series from one replica after the full operation × binding × outcome matrix | a fixed, published number with a CI gate at +0 without a changelog line | drive `tests/operation_matrix`, scrape `Telemetry::prometheus()`, count lines | **Yes** |
| T-2 hostile inputs | series after 10^4 distinct methods, tool names, models, tenants, peers | unchanged from T-1 except documented `_OTHER` rows (17 per model-labelled metric, measured) | campaign `hostile/` + a new client arm for `server.address` | **Yes** |
| T-3 per-tenant dimension | series growth with `with_tenant_dimension(n)` | ≤ (n + 1) × base of the three labelled metrics (ADR 0038 §4) | unit test in `srv/otel/control_tests.rs` | **Yes** |
| T-4 spans per request | spans exported per blocking send / per streamed event / per store call | published count; no per-event span | in-memory span exporter test | **Yes** |
| T-5 telemetry overhead | CPU per send with default features and nothing installed vs `default-features = false` | ≤ 5% (measured 4.6% and 6.6%) by callgrind Ir, not time | `send_latency_breakdown` under callgrind | **Yes** via Ir |

---

## 5. Ranked work items

Ordering: correctness of the record first, then removing work from the hot path, then the missing signals a
fleet operator needs, then scale qualification. Each item names an acceptance check that can fail.

### Performance

**P-1. Stop arming a timer per request for the header read.**
Evidence: 76 of 257 wake-up writes from `Sleep::poll → reregister` in hyper's `Conn`
(`results/2026-11-11/strace-k-wakeup-callers.txt`); `write` 2.2 vs 0.11 per request (`sdk-comparison:130-132`).
Change: keep the slowloris bound without a per-request `Sleep` — e.g. extend the existing per-connection
`IdleTimeout` (`srv/serve/graceful/idle/mod.rs`, one boxed `Sleep` reset by deadline arithmetic, already
1 of 257) to bound "bytes of a request head not complete by T", and pass `header_read_timeout(None)` to hyper
when it does. Files: `srv/serve/graceful/connection.rs:129-160`, `idle/mod.rs`, `config.rs`.
Acceptance: (a) target L-2: eventfd writes per request ≤ 0.5 by `strace -f -c` on the container, before and
after, three runs each with the range; (b) the existing slowloris tests (`tests/hardening_tests`, ADR 0042's)
pass unchanged, plus a test that a head trickled at 1 byte/s is closed within the bound; (c) L-1 CPU µs per
request reported, not gated, from the container.
Risk: this is a security control (the document says so, `:145-146`); do not ship without (b).

**P-2. Remove the timer from `settled()`.**
Evidence: 50 of 257 wake-ups under `timeout` around the `watch::Receiver`; code at
`srv/streaming/event_queue/applied.rs:99-106`, caller `in_memory.rs:310-320`.
Change: check `position >= through` synchronously first (no future), and bound the remaining wait with the
handler's existing coarse deadline source or by racing `persistence.closed()` only, arming a `Sleep` solely
when the first poll is pending *and* after a `yield_now`; or have the consumer signal completion of the last
position through the existing `terminal_gate` so no timer is needed. Files: `applied.rs`, `in_memory.rs`.
Acceptance: L-2 count; the tests named in the `settled` doc comment (continuation numbering after
`input-required`, 2026-10-13) pass; a new test that a consumer which never applies still releases the turn
within `write_timeout`.

**P-3. Serialize once: JSON-RPC response envelope and request params.**
Evidence (CONJECTURED cost): `success_response_bytes` serializes to a `Value` tree and then to bytes
(`srv/dispatch/jsonrpc/response.rs:23-40`); `JsonRpcRequest.params` is a `Value` tree deserialized a second
time (`jsonrpc.rs:184-198`, `response.rs:53-67`). Allocation is ~25% of self time (`sdk-comparison:172-173`).
Change: a private borrowing envelope `{jsonrpc, id, result: &T}` serialized straight to `Vec<u8>`; for
requests, `Box<RawValue>` params on the server's private request type (the public `JsonRpcRequest` stays).
Files: `srv/dispatch/jsonrpc/{response.rs,mod.rs,params.rs}`, WebSocket's equivalent.
Acceptance: L-4 allocations per send and L-3 Ir before/after (both deterministic on the container); byte-for-
byte golden tests of responses (`tests/golden`) unchanged; unknown-params warning test still passes.
Over-engineering risk: do not introduce a zero-copy request model across the handler; stop at the envelope.

**P-4. One serde pass per event.**
Evidence (CONJECTURED): size check serializes into a counting writer (`in_memory.rs:562-567`), SSE serializes
again (`sse.rs:65-93,437`), SQL stores a third time. Change: when a queue has a streaming reader or a SQL
log, keep the bytes of the first pass in the shared event (`Shared`) and reuse them for the SSE frame and the
log payload; with neither, keep the counting pass. Files: `srv/streaming/event_queue/{mod.rs,in_memory.rs}`,
`sse.rs`, `store/event_log_sql.rs`. Acceptance: `scripts/check_streaming_linearity.py` still passes;
allocations and Ir per streamed event (L-3/L-4) before/after; `tests/event_byte_budget.rs` bound unchanged
(the retained bytes must be charged to the budget).
Risk: retaining bytes raises memory per buffered event; gate on M-4.

**P-5. Drop duplicate and allocating metric recordings on the send path.**
Evidence: four recordings per send, each allocating (`srv/otel/mod.rs:325-417`; call sites
`srv/handler/messaging/mod.rs:95,130-135`, `rpc_span/mod.rs:405-412`); default telemetry cost 4.6–6.6%
(`readiness-bar.md:166-169`). Change: pass `&'static str` method/outcome constants as `KeyValue` static
values (no `to_owned`), pre-build attribute sets per (system, method, status) in a small fixed table, and
deprecate `a2a.server.request.duration` in favour of `rpc.server.call.duration` (dashboards read the latter:
`a2a-recording.yml`). Same for `store/traced.rs:101,144-156` (`format!` per store call).
Acceptance: T-5 (Ir with default features vs none ≤ 5%); `check_otel_metrics_coverage.py` and
`tests/operation_matrix` pass; `promtool test rules` unchanged.

**P-6. Make the regression gate see what the comparison saw.**
Evidence: the in-process, one-runtime benchmark "hides the cost" (`sdk-comparison:113-117`); the gate's
threshold is 50% (`benchmarks.yml:421`). Change: add deterministic gates that do not depend on runner speed —
callgrind Ir per send (L-3), allocations per send (L-4), eventfd writes per request (L-2), statements per
send (L-6, exists) — each with a ±5% band, in `benchmarks.yml`; move the `sdk-comparison` pin to a path
dependency in a nightly, non-gating job that publishes medians with ranges. Files:
`.github/workflows/benchmarks.yml`, `benches/scripts/check_regression.py`, `benches/sdk-comparison/`.
Acceptance: `scripts/prove_workflow_gates_fail.py` proves each new gate fails on a planted regression
(e.g. an extra `to_value`).

**P-7. Header map: stop copying every header.**
Evidence (CONJECTURED): `header_fields` (`srv/dispatch/mod.rs:303-333`) allocates two `String`s per header
and a SipHash map per request; "a `SipHash` map about 4%". Change: this is a public-API-shaped type
(`Option<&HashMap<String,String>>` runs through every handler method), so first measure with L-3; if
confirmed, build the map lazily (only when an interceptor, tenant resolver or span asks) behind the same
signature. Acceptance: L-3/L-4; interceptor and tenant tests unchanged.
Over-engineering risk: high if done as an API redesign; do the lazy form or nothing.

**P-8. (Decision needed) third-task wake-ups on the blocking path.**
Evidence: 73 + 48 of 257 wake-ups are scheduler hand-offs; one core closes most of the gap to 0.14.1
(table 3). The in-request collection was rejected at −21% (`701b02a1`). Change: none proposed without a
design; candidate to measure is running the collector on the executor's task when the executor is spawned
(one hand-off instead of two). Acceptance: L-2 and L-1 interleaved against the current tree on a dedicated
host. Not measurable meaningfully on the container beyond the L-2 count.

### Observability

**O-1. Fix the record: F1 in `docs/readiness-bar.md`.**
Evidence: `:181` vs `:88`, `:262`, `:288`. Change: state F1 as "first half measured 2026-11-11, retry/drop
half not", in the prose and in the `measured-limits` JSON block. Acceptance:
`scripts/release_evidence.py --self-test` passes and a new check fails when an id appears both in a
"measured" section and in `not_measured`.

**O-2. Telemetry-failure visibility out of band (S4).**
Evidence: §1.6 S4. Change: (1) `stack-alerts.yml`: `A2ATelemetryExportFailing` on
`rate(a2a_telemetry_export_failed_total[10m]) > 0` and an absence rule per job (the counter is only useful
when scraped, so document `Telemetry::prometheus()` as the path that survives an OTLP outage); (2) one WARN
log line per failed batch class, rate-limited; (3) a `telemetry` field in the `/ready` body (informational,
never 503). Files: `deploy/observability/prometheus/rules/stack-alerts.yml`, `tests/stack-alerts.test.yml`,
`srv/otel/telemetry/{self_metrics.rs,diagnostics.rs}`, `check_stack.py --write-kubernetes`.
Acceptance: `promtool test rules` case that fires and one that does not; target F-7 run on the container.

**O-3. The missing fleet gauges and histograms (bounded labels only).**
Evidence: §1.7. Change, each a defaulted `Metrics` hook forwarded by `Arc`/`Tee`/global like ADR 0038 §4:
- `a2a.server.queue.capacity` gauge and `a2a.server.admission.{running,queued}` gauges,
  `a2a.server.admission.wait` histogram, refusal causes `admission_queue_full`, `admission_wait` (ADR 0046
  "not built"; `srv/admission/gate.rs`, `srv/handler/load.rs:119`).
- `db.client.connection.count{state=idle|used}`, `db.client.connection.max`,
  `db.client.connection.wait_time` from `sqlx::Pool::{size,num_idle}` sampled by an observable gauge
  (`srv/store/postgres_store/`, `sqlite_pool.rs`); extend `TracedTaskStore`-style wrapping to push-config,
  halt, lease, claim, outbox and audit stores (M11).
- `a2a.server.push.outbox.{depth,oldest_age}` sampled by the worker's existing 1 s poll
  (`…/push_delivery/outbox_worker.rs:38`), one `COUNT`/`MIN(recorded_at)` per poll per replica — gate on L-6.
- `a2a.server.event_log.lag` histogram: broadcast position minus applied position at each apply
  (`srv/streaming/event_queue/applied.rs`).
- `a2a.server.halt.propagation` and `a2a.server.lease.lapse_detection` histograms (time from the store's
  write timestamp to the local stop).
- Client: `a2a.client.retries{outcome}`, `a2a.client.hedges{won}`, `a2a.client.endpoint.ejections`,
  `a2a.client.endpoints{state}` gauge from `HttpPool::origins()` (M10; ADR 0050 "not built"), and card/JWKS/
  token durations (M12). **Bound `server.address`** on `rpc.client.call.duration` with the same
  first-N-then-`_OTHER` scheme as `tool_metrics.rs:117-128`, or an opt-in.
Acceptance: T-1 and T-2 series-count gates; `check_otel_metrics_coverage.py`; one `promtool` rule test per
new alert (`A2AAdmissionQueueSaturated`, `A2AStorePoolSaturated`, `A2APushOutboxAging`,
`A2AEventLogLagging`); dashboard panels checked by `check_dashboards.py`.
Over-engineering risk: do not add per-task, per-root or per-peer-unbounded labels; do not add a second
metrics facade.

**O-4. SLO recording rules and multi-window burn-rate alerts.**
Evidence: no burn-rate rule exists (§1.7). Change: `a2a:slo_*` recording rules for availability (server-class
failures per ADR 0039 §1) and latency (the exact `le="5"` bucket of ADR 0039 §2) at 5m/30m/1h/6h, and the
standard fast/slow pairs replacing `A2AServerErrorRatioHigh` and `A2AUnaryLatencyHigh` as pages; SLO target
left as a rule-file variable. Files: `a2a-recording.yml`, `a2a-alerts.yml`, tests, generated Kubernetes
objects. Acceptance: `promtool test rules` with a fast-burn series that pages within the short window and a
slow leak that only tickets; `check_stack.py` passes.

**O-5. Swarm trace view that survives a split trace.**
Evidence: §1.4, §1.7 row 1. Change: record `a2a.lineage.root` and `a2a.lineage.depth` on the SERVER
`a2a.rpc` span and on the client `a2a.rpc.client` span when the call carries the lineage extension; add a
span *link* from a resumed/handed-off turn to the trace of the turn it continues (store the previous
`traceparent` with the journal entry, ADR 0030); ship a Tempo/TraceQL query and a Grafana panel "job by
root". Files: `srv/rpc_span/{mod.rs,agent.rs,link.rs}`, `cli/call_span.rs`, `cli/delegation/journal.rs`,
`deploy/observability/grafana/dashboards/`. Acceptance: an in-memory-exporter test over three hops with one
WebSocket hop and one `Restart` hop asserting every span carries the same `a2a.lineage.root`; a resumed
delegation's span has one link.
Risk: lineage is a caller's claim (ADR 0034 item 4) — span attribute only, never a label.

**O-6. Per-tenant token accounting.**
Evidence: §1.7 row 3; S3 OPEN. Change: let `GenAiMeter` take the same allow-listed `a2a.tenant` dimension as
`OtelMetrics::with_tenant_dimension` (`srv/rpc_span/tool_metrics.rs`, `srv/otel/control.rs:92-103`).
Acceptance: T-3; a recording rule `a2a:gen_ai_tokens:rate5m` by tenant with a rule test. This is accounting
visibility only; a quota (S3) needs the maintainer's decision.

**O-7. Close the small register rows that have a mechanical test.** M6 (meter WebSocket upgrade refusals with
`record_unrouted`), H3 (span + duration on SLIMRPC multicast), M8 (a tower layer counting tonic's own
refusals), store `error.type` = SQLSTATE class, W2-LIVE-8 (redact provider error bodies). Acceptance: one row
each in `tests/operation_matrix` or the binding's `tests/telemetry.rs`.

**O-8. GenAI convention refresh.** Re-read the pinned commits (`9ffc7e0`, `4f85037`, PR #195 `842a839`)
against upstream and record the diff in an ADR amendment before changing names; add time-to-first-token for
streaming models if the conventions define it. Acceptance: the ADR lists each attribute as kept/renamed/
added with the upstream commit; `check_stack.py` and the GenAI dashboard queries updated in the same change.
NOT VERIFIED in this audit: what changed upstream.

### Scale

**S-A. Finish the readiness bar on the bindings and stores it names** (targets F-2..F-5, L-5; register S5,
X-15). Parameterise `tests/readiness_bar/` by binding and store; add L2 and L5. Acceptance: the
`measured-limits` block lists no row under `not_measured`; each row printed before asserted, three runs.

**S-B. Run 60 replicas with a stated connection plan** (target S-3). Evidence: the unrun row. Change:
document and test the pool arithmetic with PgBouncer in session mode for LISTEN, or `LogListen::Direct`
(ADR 0057 §6). Acceptance: the qualification invariants (SQL checks) hold; connections used recorded.

**S-C. Fix X-10 (event lost after `store_write_retry`).** Evidence: handoff `:49-51`;
`state_machine.rs:197-215`. This is a correctness item that scale makes likelier; it precedes any throughput
work on the stores. Acceptance: a test that pauses the store for 40 s and asserts no event is missing
afterwards, or the task ends `failed` with the loss counted.

**S-D. SQL write amplification, measured before changed.** Evidence: §3.4. Steps: (1) add a PostgreSQL arm
to `statement_count` for a 20-event streaming turn (target L-6 "per event ≤ 2"); (2) measure WAL bytes per
status transition with the four indexes vs without `idx_tasks_context_id`; (3) only then batch appends of
already-queued events into one statement. Acceptance: statement and WAL counts before/after on PostgreSQL
16/18/19 (CI's versions), all store contract tests unchanged.
Over-engineering risk: splitting `tasks.data` into columns or a history table is a migration across four
stores; ADR 0047 already declined the SQL history splice with a measurement. Do not start there.

**S-E. Idle-stream and parked-task memory** (targets M-2, M-3, S-2). New harness; no code change expected.
Acceptance: numbers with the command, three runs.

**S-F. Kubernetes example completeness.** Add to `examples/deploy-agent/deployment.yaml`: an HPA on an
exported utilization metric (after O-3's capacity gauge exists), a headless Service for client-side
balancing, a NetworkPolicy, a PodMonitor; wire `with_load_reporting` and the ADR 0054 hand-off in
`examples/deploy-agent/src/main.rs` and correct the manifest comment. Acceptance: `kubeconform` (already used
in CI per register X-03) on the manifest; `scripts/check_example_servers.py`; a two-process shutdown test
showing the continuation lands on the other replica. A real cluster run remains NOT MEASURED until a host
that can run `kind` exists.

**S-G. Announce status saves or document the poll bound (X-11 / E-F25).** Acceptance: the two-replica test
in `tests/log_wake_e2e.rs` extended with a cancel of a parked task ending the other replica's stream in
< 1 s with NOTIFY on.

**Deferred, with reasons (do not build now):** multi-region (undesigned, §3.6); fleet-wide admission and
inference quota (S3, decision first); lease groups (wire change, measurement S-4 first); blob offload
(ADR 0047 B); a Helm chart (ADR 0039 §10 declined it).
