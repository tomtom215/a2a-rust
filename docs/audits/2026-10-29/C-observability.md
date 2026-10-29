# C — Observability audit (a2a-rust, tree at 8dc4331)

Static reading only; no cargo build/test was run. "Verified" = I opened and quoted the code or config; "Conjecture" = an inference about runtime or third-party behaviour that I did not confirm.
Upstream crates were read from `~/.cargo/registry/src/*/` (opentelemetry 0.33.1, opentelemetry_sdk 0.33.1, opentelemetry-otlp 0.33.1, opentelemetry-appender-tracing 0.33.1, tracing-opentelemetry 0.34.0, tracing-subscriber 0.3.23).

Telemetry entry point: `crates/a2a-protocol-server/src/otel/telemetry/` (`Telemetry`, `TelemetryBuilder`), re-exported by `crates/a2a-protocol-sdk/src/lib.rs:127`. Legacy entry point: `crates/a2a-protocol-server/src/otel/pipeline.rs` (`init_otlp_pipeline`).

Items marked KNOWN overlap `docs/handoff.md` "Next, in order" item 3.

---

## 1. Env-var support table

| Variable | Supported? | Where | Notes |
|---|---|---|---|
| `OTEL_SDK_DISABLED` | Yes (`true`, any case) | `telemetry/config.rs:145-147`, `builder.rs:285` | Side effect: `install_subscriber` then installs **no** subscriber at all, so stderr logging goes too (C-07). Ignored by `init_otlp_pipeline` (C-12). |
| `OTEL_SERVICE_NAME` | Yes | SDK `Resource::builder`; precedence over code default `config.rs:259-261`, `builder.rs:462-466` | |
| `OTEL_RESOURCE_ATTRIBUTES` | Yes, percent-decoded | `telemetry/resource_env.rs` (decode + apply), `builder.rs:449-455` | A malformed `%` is kept and reported, not discarded (deliberate deviation). The legacy pipeline does not decode. |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}ENDPOINT` | Yes | `config.rs:80-109`; HTTP passed to upstream `resolve_http_endpoint` (otlp `exporter/http/mod.rs:762-795`); gRPC resolved by the crate `config.rs:89-99` | Generic endpoint: `/v1/<signal>` appended on HTTP (`exporters.rs` `endpoint_from_base`). Per-signal: used as written. gRPC: no path. A generic endpoint that already ends in `/v1/traces` gets a warning (`builder.rs:332-364`). An HTTP endpoint without a scheme is a hard error (`builder.rs:308-321`). |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}HEADERS` | Yes (upstream) | HTTP: otlp `exporter/http/mod.rs:293-297`; gRPC: tonic `build_channel` merges env headers before the custom-channel return | Values set in code are merged in. A variable that names the same header wins. |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}PROTOCOL` | Yes: `grpc`, `http/protobuf`; `http/json` only with the `otlp-json` feature | `config.rs:152-201` | An unknown value fails the build (spec: warn and ignore), see C-08. |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}TIMEOUT` | Yes (ms, default 10 s) | `config.rs:205-214` | A non-integer is a hard error (C-08). |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}COMPRESSION` | `gzip` and `none` only | `telemetry/compression.rs:55-68` | `zstd` fails the build: "zstd is not supported" (C-08). |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}CERTIFICATE` | Yes | `config.rs:219-221`, `builder.rs:384-386` | Silently unused on a plaintext `http://` gRPC endpoint. |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}CLIENT_{KEY,CERTIFICATE}` | Yes; each falls back independently, and only one of the pair is an error | `config.rs:234-255` | |
| `OTEL_EXPORTER_OTLP_{,SIGNAL_}INSECURE` | gRPC only, and only for an endpoint without a scheme | `config.rs:89-99` | Matches the spec. |
| `OTEL_TRACES_SAMPLER` / `_ARG` | Yes (SDK `Config::default`) | opentelemetry_sdk reads it; the builder does not override the sampler (`assemble.rs:121`) | Default `parentbased_always_on`. |
| `OTEL_PROPAGATORS` | `tracecontext`, `baggage`, `none`; other names get a notice | `telemetry/propagators.rs:39-66` | Affects only the **global** propagator. A2A wire injection and extraction ignore it, and baggage never crosses A2A (C-10). |
| `OTEL_TRACES_EXPORTER` | `otlp`, `console`/`logging`, `none`; a list is allowed | `telemetry/selection.rs:51-106` | An unknown value (`zipkin`, `jaeger`) fails the build (C-08). |
| `OTEL_METRICS_EXPORTER` | `otlp`, `console`, `none`; `prometheus` only with `otel-prometheus` (off by default in the SDK, `a2a-protocol-sdk/Cargo.toml:31,61`) | `selection.rs:70-91` | **KNOWN L1, changed:** no longer a panic; every shipped binary now does `eprintln!` + `std::process::exit(2)` (e.g. `examples/hello-agent/src/main.rs:66-77`). Still fatal, see C-08. |
| `OTEL_LOGS_EXPORTER` | `otlp`, `console`, `none` | `selection.rs` | |
| `OTEL_EXPORTER_PROMETHEUS_HOST` / `_PORT` | Yes (feature `otel-prometheus`) | `telemetry/prometheus.rs:56-66` | Default host `localhost`, which a scraper in another container or pod cannot reach (C-16). |
| `OTEL_METRIC_EXPORT_INTERVAL` / `_TIMEOUT` | Yes (SDK `PeriodicReader`) | used for display only in `assemble.rs:18-22` | |
| `OTEL_METRICS_EXEMPLAR_FILTER` | **No** | not read anywhere; opentelemetry_sdk 0.33.1 always emits `exemplars: vec![]` (`metrics/internal/histogram.rs:195,249`) | No exemplars exist at all (C-05). |
| `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE` | Yes (upstream) | otlp `metric.rs:127-147` | `delta` breaks the shipped Prometheus OTLP path (C-21). |
| `OTEL_EXPORTER_OTLP_METRICS_DEFAULT_HISTOGRAM_AGGREGATION` | No | | |
| `OTEL_BSP_*` | Yes (SDK; includes `MAX_CONCURRENT_EXPORTS`) | via `with_batch_exporter` (`assemble.rs:127-128`) | Queue default 2048: memory is bounded. |
| `OTEL_BLRP_*` | Yes (SDK) | `assemble.rs:196-197` | Queue default 2048: bounded. |
| `OTEL_SPAN_{ATTRIBUTE,EVENT,LINK}_COUNT_LIMIT` | Yes (SDK) | | |
| `OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT`, `OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT`, `OTEL_LOGRECORD_*` | **No** | not in the SDK | Opt-in `gen_ai.*.messages` content is unbounded (C-20). |
| `OTEL_LOG_LEVEL`, `OTEL_CONFIG_FILE` | No | | `RUST_LOG` is used instead, and it controls stderr only (C-01). |

---

## 2. Dashboard / rule query vs. instrument cross-check

Translation assumed: Prometheus 3 OTLP `UnderscoreEscapingWithSuffixes` (`prometheus/prometheus.yml`), or the Collector `prometheus` exporter with the same strategy (`otel-collector/config.yaml`). Units in braces are dropped, `s` becomes `_seconds`, counters gain `_total`, histograms gain `_bucket`/`_count`/`_sum`.

| Instrument (code) | Unit / kind | Prometheus series | Labels emitted | Used by |
|---|---|---|---|---|
| `rpc.server.call.duration` (`otel/mod.rs:196`) | s, histogram | `rpc_server_call_duration_seconds_*` | `rpc_system_name`, `rpc_method` (normalised to `lf.a2a.v1.A2AService/*`, `rpc_span/names.rs:122-140`), `rpc_status_code`, `error_type` | recording rules: server calls, errors, ratio, p50/p95/p99. **Match.** |
| `a2a.server.requests` / `.responses` / `.errors` (`otel/mod.rs:140-156`) | counter | `a2a_server_{requests,responses,errors}_total` | `method`; `errors` adds `error` (bounded `metric_label()`) | `a2a:server_unanswered`, `a2a:server_errors_by_kind`. **Match.** |
| `a2a.server.request.duration` (`otel/mod.rs:206`) | s, histogram | `a2a_server_request_duration_seconds_*` | `method` | **No panel or rule** (duplicates `rpc.server.call.duration`) |
| `a2a.server.executor.duration` (`otel/mod.rs:222`) | s, histogram | `a2a_server_executor_duration_seconds_*` | `outcome` | panels. **Match.** |
| `a2a.server.queue_depth` (`otel/mod.rs:160`) | gauge | `a2a_server_queue_depth` | none | panel + `A2AQueueDepthGrowing`. **Match.** |
| `a2a.server.pool.{active,idle}` (`otel/mod.rs:236,242`) | gauge | `a2a_server_pool_{active,idle}` | none | panel. Fed only by `serve/connections.rs:69` (`Server`), so it is **always empty** for `A2aRouter`/axum, gRPC, WebSocket and SLIMRPC listeners. |
| `a2a.server.pool.created` (`otel/mod.rs:248`) | counter | `a2a_server_pool_created_total` | none | **No panel** |
| `a2a.server.pool.closed` | counter | `a2a_server_pool_closed_total` | none | panel. Same `Server`-only caveat. |
| `a2a.server.persistence_errors` | counter | `a2a_server_persistence_errors_total` | `operation`, `error` | panel + alert. **Match.** |
| `a2a.server.push_deliveries` | counter | `a2a_server_push_deliveries_total` | `outcome` | rules + alerts. **Match.** |
| `a2a.server.store_retries` (`otel/mod.rs:294`) | counter | `a2a_server_store_retries_total` | `operation`, `cause` | **No panel, no alert** (C-14) |
| `db.client.operation.duration` (`store/traced.rs:137`) | s, histogram | `db_client_operation_duration_seconds_*` | `db_system_name`, `db_operation_name`, `db_namespace`, `server_address`, `server_port`, `error_type` | panels. **Match.** Only with traced stores. |
| `http.server.request.duration` (agent card, `rpc_span/card.rs:281`) | s | `http_server_request_duration_seconds_*` | method, scheme, route, status, version, `error_type` | **No panel** |
| `http.client.request.duration` (push webhook, `rpc_span/webhook.rs:191`) | s | `http_client_request_duration_seconds_*` | `server_address` = **client-supplied webhook host** (C-03), port, status, `error_type` | **No panel** |
| `rpc.client.call.duration` (`a2a-protocol-client/src/call_metric.rs:61`) | s | `rpc_client_call_duration_seconds_*` | `rpc_system_name`, `rpc_method`, `rpc_status_code`, `error_type`, `server_address`, `server_port` | client dashboard + rules. **Match.** |
| `gen_ai.client.inference.duration`, `gen_ai.execute_tool.duration` (`tool_metrics.rs:200-209`) | s | `gen_ai_client_inference_duration_seconds_*`, `gen_ai_execute_tool_duration_seconds_*` | `gen_ai_request_model`, `gen_ai_response_model`, `gen_ai_tool_name` (**model-chosen**, C-02), `error_type` | GenAI dashboard. **Match.** |
| `gen_ai.client.inference.operation.{input,output}_tokens` | `{token}` histogram | `..._tokens_bucket` | as above | **Match.** |
| `gen_ai.client.inference.usage.{input,output}_tokens` | `{token}` counter | `..._tokens_total` | + `gen_ai_token_modality` | **Match.** |
| `gen_ai.invoke_agent.duration`, `.inference_calls`, `.tool_calls` | s / `{..}` histograms | `gen_ai_invoke_agent_duration_seconds_*`, `gen_ai_invoke_agent_{inference,tool}_calls_bucket` | `gen_ai_request_model`, `gen_ai_agent_name`, `error_type` | **Match.** |
| `target_info` (resource) | n/a | `target_info` | `job`, `instance` (= `service.instance.id`, a random UUID per process, `builder.rs:474-479`) | template variables + `A2AAgentSilent` |

**Result:** every metric and label name the dashboards and rules query is one the code emits (the README's claim that the names were read from a live Prometheus holds up against the code). The query gaps are semantic instead:
- (a) the pool panels can never show data on the non-`Server` listeners;
- (b) `$instance` filters only 4 of the server panels, and the recording rules aggregate `instance` away (`sum by (job, ...)`), so per-replica drill-down of calls, errors and latency is impossible;
- (c) no panel or alert covers `store_retries`, `pool.created`, `a2a.server.request.duration`, `http.server.request.duration` or `http.client.request.duration`;
- (d) no process or runtime metrics exist at all (C-13).

Script used: [path to the reviewer's local, unpublished script removed].

---

## 3. Findings

### C-01 — High — `RUST_LOG` cannot reduce what is exported; spans and span-events from every crate at every level, DEBUG and TRACE included, are exported
- **Files:** `crates/a2a-protocol-server/src/otel/telemetry/mod.rs:183-187`, `:222-229`; `diagnostics.rs:72-78`.
- **Evidence (verified):**
  - The span layer filter is `tracing_opentelemetry::layer().with_tracer(...).with_filter(filter_fn(|meta| !is_self_target(meta.target())))`: no level at all.
  - `install_subscriber` applies `RUST_LOG` only to the stderr layer: `.with(self.layer()).with(diagnostics::layer(filter, ...))`, and in `diagnostics.rs:78` `.with_filter(filter.and(shown))`.
  - tracing-opentelemetry 0.34 `on_event` (`layer.rs:1345-1385`) turns **every** event inside a span into an OTel span event carrying `level`.
  - The doc at `builder.rs:131-134` admits "Spans are not filtered by level here; a subscriber-wide filter ... applies to both", but `install_subscriber` installs no subscriber-wide filter.
- **Failure scenario:**
  - Every `trace_debug!`/`debug!`/`trace!` callsite in the process becomes live, because the global max level hint becomes TRACE. Each one is attached as a span event and exported: a2a's own debug events, sqlx statement events, and any LLM crate's spans.
  - The operator sets `RUST_LOG=warn` to cut volume: it has no effect on OTLP.
  - CPU and egress grow with debug chatter. Any dependency that logs payloads at DEBUG leaks them to the collector.
  - INFO events are exported twice, as log records and as span events.
- **Fix:** give the span layer a level filter (`LevelFilter` / `Targets` from `OTEL_*`- or `RUST_LOG`-style config, default INFO). Apply the same parsed `Targets` to the OTLP layers. Consider `with_level(false)` and dropping event→span-event for levels below INFO.
- **Verified by reading.** The CPU cost magnitude is conjecture.

### C-02 — High — Model-controlled tool name is a metric label (unbounded cardinality, prompt-injectable)
- **Files:** `crates/a2a-protocol-server/src/tool_loop/run.rs:295-298`, `:365-366`; `rpc_span/tool_metrics.rs:151-153`.
- **Evidence (verified):**
  - `fn tool_span(&self, call: &ToolCall) -> GenAiSpan { ... GenAiSpan::tool(&call.name, &call.id, description, &self.meter) }` runs **before** the lookup fails (`let Some(tool) = tool else { span.failed("unknown_tool"); ...`).
  - `Measure::tool` pushes `KeyValue::new("gen_ai.tool.name", name.to_owned())` onto `gen_ai.execute_tool.duration`.
- **Failure scenario:** a user message such as "call the tool named <uuid>" (or a hallucinating model) creates a new series per call: about 15 buckets × N per instance. After 2000 attribute sets the SDK's cardinality limit sends everything into the `otel.metric.overflow` series, which corrupts the GenAI dashboard for legitimate tools.
- **Fix:** record `gen_ai.tool.name="_OTHER"` (or omit it) when the tool is not registered.
- **Verified by reading.**

### C-03 — High — Push-webhook host (client-supplied) is a metric label
- **Files:** `crates/a2a-protocol-server/src/rpc_span/webhook.rs:154-170` (`fn target(url)`), `:183-205`.
- **Evidence (verified):** `KeyValue::new("server.address", host.clone())` on `http.client.request.duration`. The host is parsed from the push-config URL, which any authenticated caller sets via `CreateTaskPushNotificationConfig` or inline in a send.
- **Failure scenario:** a tenant registers webhooks on `a1.evil.example`, `a2.evil.example`, and so on. Every delivery attempt mints a new 14-bucket histogram series. This hits the same overflow as C-02 and is a Prometheus cardinality DoS that one tenant can cause.
- **Fix:** drop `server.address`/`server.port` from the metric (keep them on the span), or bucket them against an allowlist.
- **Verified by reading.**

### C-04 — High — `A2AAgentSilent` pages critical on every restart or rollout, and auto-resolves for a truly dead agent
- **Files:** `deploy/observability/prometheus/rules/a2a-alerts.yml` (A2AAgentSilent); `crates/a2a-protocol-server/src/otel/telemetry/builder.rs:474-479`.
- **Evidence (verified):**
  - The rule is `group by (job, instance) (present_over_time(target_info[1h])) unless group by (job, instance) (present_over_time(target_info[10m]))`, with `for: 0m` and `severity: critical`.
  - `instance` is `service.instance.id = uuid::Uuid::new_v4()` per process.
  - The README admits that it "also fires for an instance a rollout retired".
- **Failure scenario:**
  - Every deploy, crash-restart or HPA scale-in raises one critical alert per old pod for about 50 minutes.
  - Conversely, a fleet that is really down stops alerting after 1 h, because the series leaves the `[1h]` window.
  - A job that never exported anything never alerts. There is no `absent()` on an expected job.
- **Fix:** alert at the `job` level, e.g. `absent_over_time(target_info{job="X"}[10m])` per expected job, or `count by (job)` dropping below a floor. Use a stable `service.instance.id` (pod name / `HOSTNAME`) when one is available. Downgrade the per-instance alert to `warning`.
- **Verified by reading.**

### C-05 — Medium — No metrics↔traces correlation: the SDK emits no exemplars, and the stack does not store or link them
- **Files:** opentelemetry_sdk 0.33.1 `metrics/internal/histogram.rs:195,249`; `deploy/observability/docker-compose.yml` (prometheus command); `grafana/provisioning/datasources/prometheus.yml`.
- **Evidence (verified):**
  - The SDK sets `exemplars: vec![]` unconditionally.
  - Prometheus runs without `--enable-feature=exemplar-storage`.
  - The data source has no `exemplarTraceIdDestinations`.
  - The dashboards have no `"exemplar": true` (count 0 in all three).
  - The Tempo data source sets `tracesToMetrics: {datasourceUid}` with no `queries` and `serviceMap: {datasourceUid: a2a-prometheus}`, but `tempo.yaml` has no `metrics_generator` and Prometheus has no remote-write receiver.
- **Failure scenario:** a latency spike on a dashboard cannot be clicked through to a trace. The Tempo "Service graph" and span→metrics links are empty.
- **Fix:**
  - Document the gap.
  - Add Tempo `metrics_generator` (service-graphs + span-metrics) with `remote_write` to Prometheus and `--web.enable-remote-write-receiver`.
  - Add `tracesToMetrics.queries`.
  - Add exemplars when the Rust SDK supports them.
- **Verified by reading.** Grafana's behaviour with an empty `queries` is conjecture.

### C-06 — Medium — No trace_id/span_id or JSON in stdout/stderr logs
- **Files:** `crates/a2a-protocol-server/src/otel/telemetry/diagnostics.rs:72-78`; `crates/a2a-protocol-server/Cargo.toml:222`.
- **Evidence (verified):** `tracing_subscriber::fmt::layer().with_writer(std::io::stderr)` is the only console layer. tracing-subscriber is built with features `["registry","std","fmt"]`: no `json`, no `env-filter`.
- **Failure scenario:** on a platform that scrapes container stdout (Kubernetes + Fluent Bit / Promtail / CloudWatch), logs carry no trace id and are not JSON. Log↔trace correlation exists only for OTLP-exported records, and the default stderr level is `warn` anyway (`DEFAULT_FILTER = "warn,a2a_protocol_server::otel::telemetry=info"`).
- **Fix:** offer `OTEL_LOGS_EXPORTER=console` JSON or a `A2A_LOG_FORMAT=json` option with a field formatter that injects `trace_id`/`span_id` from the current OTel context.
- **Verified by reading.**

### C-07 — Medium — `OTEL_SDK_DISABLED=true` also silences all stderr diagnostics
- **File:** `crates/a2a-protocol-server/src/otel/telemetry/mod.rs:221-223`.
- **Evidence (verified):** `pub fn install_subscriber(&self) -> bool { ... if self.disabled { return false; }`.
- **Failure scenario:** an operator disables OTel to rule it out during an incident. Every binary that relies on `install_subscriber` (all the examples, `tck/sut`, `itk`, `a2a-cli`, `slim_node`) now prints **no** warnings or errors at all, and the incident goes dark.
- **Fix:** still install the stderr diagnostics layer when disabled; skip only the OTel layers.
- **Verified by reading.**

### C-08 — Medium — Unknown or unsupported enum values are fatal at startup (spec says warn and ignore)
- **Files:** `selection.rs:86,102` (unknown exporter, `prometheus` without the feature); `config.rs:167-171` (protocol); `config.rs:212` (timeout); `compression.rs:61,66` (zstd or other); plus every binary's `std::process::exit(2)` (e.g. `examples/hello-agent/src/main.rs:66-77`, `examples/deploy-agent/src/main.rs:321-331`).
- **Evidence (verified):**
  - The selection error reads: "only `otlp`, `console`, `prometheus` (metrics) and `none` are supported".
  - The compression error reads: "zstd is not supported (it needs a C library)".
- **Failure scenario:** a fleet-wide `OTEL_METRICS_EXPORTER=prometheus` or `OTEL_EXPORTER_OTLP_COMPRESSION=zstd` (common defaults in OTel Operator injection and in other languages' SDK presets) puts every a2a-rust pod into CrashLoopBackOff. The crate's own propagator handling chose "report, not refuse" for exactly this fleet reason (`propagators.rs:10-13`), so the policy is inconsistent.
- **Fix:** warn and fall back (the signal off, or the default) for enum and number parse failures. Keep hard errors for TLS file errors only.
- **KNOWN (L1), partially.** The panic is gone, but a fatal exit remains.

### C-09 — Medium — Streams the peer abandons count as server errors, so the error-ratio alert fires on normal UI behaviour and on every deploy
- **Files:** `crates/a2a-protocol-server/src/rpc_span/stream.rs:35-55`; `deploy/observability/prometheus/rules/a2a-recording.yml` (`a2a:rpc_server_error_ratio:rate5m`); `a2a-alerts.yml` (`A2AServerErrorRatioHigh`).
- **Evidence (verified):** `Self::Abandoned => Some(Cow::Borrowed(CANCELLED))`, where Abandoned is documented as "the peer went away, or the server dropped it". The ratio rule counts `error_type!=""`.
- **Failure scenario:**
  - A browser UI that closes `SubscribeToTask` or `SendStreamingMessage` early, or graceful shutdown dropping streams during a rollout, pushes the ratio over 5%.
  - There is also no minimum-traffic guard: 1 failure in 2 calls is 50%.
  - The same applies client-side: `Status::cancelled()` (`a2a-protocol-client/src/call_span.rs:269-275`) feeds `a2a:rpc_client_error_ratio`.
- **Fix:** add `error_type!="cancelled"` in the ratio rule (keep a separate cancelled-rate panel), and `and sum by (job)(rate(...[5m])) > 0.1` as a traffic floor.
- **Verified by reading.**

### C-10 — Medium — W3C baggage is never propagated across A2A, and `OTEL_PROPAGATORS` has no effect on the A2A wire
- **Files:** `crates/a2a-protocol-client/src/call_span.rs:365-414` (`propagate` writes only `traceparent`/`tracestate`); `crates/a2a-protocol-server/src/rpc_span/webhook.rs:96-110` (push: same two headers); `crates/a2a-protocol-server/src/otel/telemetry/propagators.rs:7-10`.
- **Evidence (verified):**
  - `propagators.rs` says "A2A's own wire handling does not use the global propagator — it reads and writes `traceparent` itself".
  - A grep for `baggage` outside `telemetry/propagators.rs` and `builder.rs` docs finds nothing.
- **Failure scenario:**
  - Tenant or user baggage (e.g. `tenant.id`, `session.id`, used for Tempo/Loki filtering) dies at the first A2A hop, even though `Telemetry` installs a baggage propagator.
  - A B3-only mesh (`OTEL_PROPAGATORS=b3`) still gets W3C headers and nothing it can read.
  - `OTEL_PROPAGATORS=none` does not stop injection; only `ClientConfig::with_trace_propagation(false)` does.
- **Fix:** inject and extract through `opentelemetry::global::get_text_map_propagator` (or at least add `baggage`) on the client, push and listener paths.
- **Verified by reading.** WebSocket (no per-request headers, documented at `a2a-protocol-client/src/transport/websocket.rs:40-61`) remains a by-design trace break.

### C-11 — Medium — Hot-path INFO logs are exported as OTLP log records by default (volume), and the client logs full endpoint URLs
- **Files:** `crates/a2a-protocol-server/src/otel/telemetry/builder.rs:59` (`log_level: tracing::Level::INFO`); server `dispatch/jsonrpc/mod.rs:317`, `dispatch/rest/mod.rs:126`, `handler/messaging/mod.rs:80`, `handler/lifecycle/get_task.rs:31`; client `transport/jsonrpc.rs:236,358`, `transport/rest/request.rs:175`, `transport/websocket.rs:739,809`.
- **Evidence (verified):**
  - Server: `trace_info!(method = %rpc_req.method, "dispatching JSON-RPC request")` and `trace_info!(http_method = %method, %path, "dispatching REST request")`.
  - Client: `trace_info!(method, endpoint = %self.inner.endpoint, "sending JSON-RPC request")`.
- **Failure scenario:**
  - At 1k rps the server exports at least 2k log records per second (dispatch + handling) to Loki. Loki's default per-tenant ingestion limits then return 429 and the collector drops records.
  - The client endpoint is logged raw, so a URL carrying `?key=` or `user:pass@` lands in Loki.
  - The REST `path` carries task ids and tenant.
  - `rpc_req.method` is attacker-controlled free text, and the fmt layer does not escape newlines, so lines can be forged when `RUST_LOG=info`.
- **Fix:**
  - Demote per-request dispatch lines to DEBUG; spans already carry them.
  - Scrub URLs the way `webhook::origin_for_log` does.
  - Log `method` only after normalisation.
- **Verified by reading.** Loki 429 under that load is conjecture.

### C-12 — Medium — The legacy `init_otlp_pipeline` is still public and violates several rules the new path fixed
- **File:** `crates/a2a-protocol-server/src/otel/pipeline.rs:146-170`, re-exported at `otel/mod.rs:71`; used by `examples/incident-response/src/hardening/observability.rs:371`.
- **Evidence (verified):**
  - It is built with `MetricExporter::builder().with_tonic()` and has no `service.instance.id`.
  - It does not check `OTEL_SDK_DISABLED` or `OTEL_METRICS_EXPORTER`.
  - It does not percent-decode resource attributes.
  - Its doc says it "panics outside a Tokio runtime".
  - It is not `#[deprecated]`.
  - The doc at `pipeline.rs` (gRPC-only section) says the crate compiles otlp with `["grpc-tonic","metrics"]`, which is stale: `Cargo.toml:210` includes `http-proto`.
- **Failure scenario:** N replicas all push the same series without an `instance` label. Prometheus OTLP sees interleaved cumulative counters from different processes, so rates are garbage (false counter resets).
- **Fix:** `#[deprecated]` pointing to `Telemetry`, or reimplement it on top of `TelemetryBuilder` (metrics only).
- **Verified by reading.** The rate corruption is conjecture, based on how Prometheus handles identical series.

### C-13 — Medium — No process or runtime metrics and no telemetry self-metrics; exporter health is not observable except in logs
- **Evidence (verified):**
  - A grep for `process.memory|process.cpu|tokio.runtime|observable_gauge` in crates/bindings finds nothing.
  - Export losses are counted in `Telemetry.failures` (`telemetry/mod.rs`, `counting.rs`) but surfaced only through a rate-limited `warn!` (`counting.rs` `REPORT_EVERY = 30s`) and the `shutdown()` result. No metric exists, and no health endpoint reports it. The `/health` endpoints (`dispatch/rest/response.rs:76`, `handler/introspection.rs:60`) cover the store only.
  - Over OTLP push there is no `up` series.
- **Failure scenario:** memory leaks, FD exhaustion or tokio starvation are invisible. Spans being dropped (BSP queue full) cannot be alerted on.
- **Fix:**
  - Emit `process.*` semconv metrics (e.g. via the `sysinfo` crate or `opentelemetry-system-metrics`) and tokio runtime metrics.
  - Expose the lost-export counters as an observable counter `a2a.telemetry.exports_lost{signal}`.
  - Optionally report them in readiness.
- **Verified by reading.**

### C-14 — Medium — Alert coverage gaps and an alert threshold that falls inside a coarse bucket
- **Files:** `deploy/observability/prometheus/rules/a2a-alerts.yml`; `crates/a2a-protocol-server/src/otel/mod.rs` (`RPC_DURATION_BUCKETS = [... 1.0, 5.0, ...]`).
- **Evidence (verified):**
  - **(a)** `A2AUnaryLatencyHigh` uses threshold 2.5 s, which sits inside the (1, 5] bucket. If 98% of calls are ≤1 s and 2% take 1.1 s, `histogram_quantile(0.99)` interpolates to 1 + 4×0.5 = 3.0 s and the alert fires although no call exceeded 1.1 s.
  - **(b)** `prometheus.yml` has no `alerting:` block and the compose stack has no Alertmanager, so every alert goes nowhere.
  - **(c)** No alert exists for `store_retries`, executor `outcome` failure or abort rates, GenAI model failure ratio, client error ratio, or Collector drops.
  - **(d)** `A2APersistenceErrors` is `critical` with `for: 1m` on `rate(...[5m]) > 0`, so a single write error pages for about 6 minutes, then resolves silently.
- **Fix:**
  - Put a bucket boundary at the threshold (e.g. add 2.5) or alert on `rate(bucket{le="5"})` ratios.
  - Ship an Alertmanager service and config.
  - Add the missing alerts.
  - Use `increase(...[15m]) > 0` with an annotation describing the data-loss runbook.
- **Verified by reading.**

### C-15 — Medium — The Collector itself is unobserved, and the base compose has no memory limit for `memory_limiter`
- **Files:** `deploy/observability/otel-collector/config-compose.yaml`, `config.yaml`, `docker-compose.yml`.
- **Evidence (verified):**
  - There is no `service.telemetry` config, and nothing scrapes the Collector's internal metrics (otelcol default internal endpoint, not published, and Prometheus has no scrape job for it).
  - `memory_limiter` uses `limit_percentage: 80`, but `mem_limit` is set only in `docker-compose.prod.yml`.
  - No `sending_queue` / `file_storage` is configured.
- **Failure scenario:** Tempo or Loki rejects data (429, disk full) and `otelcol_exporter_send_failed_*` rises unseen. In the base stack the limiter computes 80% of **host** memory (conjecture: cgroup without a limit), so it never protects the container. A Collector restart loses its in-memory queue.
- **Fix:** add `service.telemetry.metrics` readers with a pull endpoint on 8888, a Prometheus scrape job, and alerts on `otelcol_exporter_send_failed_*` and `otelcol_processor_refused_*`. Set `mem_limit` in the base compose too. Add `file_storage` for exporter queues.
- **Verified by reading,** except the limiter's behaviour without a limit (conjecture).

### C-16 — Low — The Prometheus pull endpoint binds `localhost` by default and runs a separate unauthenticated listener
- **File:** `crates/a2a-protocol-server/src/otel/telemetry/prometheus.rs:59` (`let host = env(HOST).unwrap_or_else(|| "localhost".to_owned());`), `serve()` loop with no connection cap.
- **Failure scenario:** in Kubernetes, a scrape from Prometheus fails until `OTEL_EXPORTER_PROMETHEUS_HOST=0.0.0.0` is set. The default matches the spec, but the feature is also off by default in the SDK (`a2a-protocol-sdk/Cargo.toml:61`), so a user without a Collector must rebuild with `--features otel-prometheus`. There is no `/metrics` route on the A2A server or router itself.
- **Fix:** document this prominently. Consider mounting `/metrics` on the axum `A2aRouter` / `Server` behind an option.
- **Verified by reading.**

### C-17 — Low — WebSocket per-call credential warning is not rate-limited (log spam)
- **File:** `crates/a2a-protocol-client/src/transport/websocket.rs:934-940`.
- **Evidence (verified):** `if other_dropped > 0 { trace_warn!(... "per-request headers are not sent over an established WebSocket connection" ...) }` runs on every call. Only the trace-header warning is once per connection (`trace_warned.swap`).
- **Failure scenario:** a client with an auth interceptor on WebSocket emits one WARN per request to stderr and to OTLP logs.
- **Fix:** gate it with an `AtomicBool`, like the trace warning.
- **Verified by reading.**

### C-18 — Low — Grafana trace→logs link likely queries the log line, not Loki structured metadata
- **File:** `deploy/observability/grafana/provisioning/traces-and-logs.yml` (`tracesToLogsV2: {datasourceUid: a2a-loki, filterByTraceID: true}`, with no `customQuery`/`tags`).
- **Failure scenario:** Grafana's generated Loki query uses a line filter `|="<traceId>"`. OTLP-ingested records hold `trace_id` in structured metadata, not in the body, so "Logs for this span" returns nothing.
- **Fix:** `customQuery: true`, `query: '{service_name="${__span.tags["service.name"]}"} | trace_id="${__trace.traceId}"'`.
- **Conjecture** about Grafana's generated query; the config was read.

### C-19 — Low — Retention and persistence gaps in the deploy stack
- **Evidence (verified):**
  - `loki/loki.yaml` has no `compactor` / `limits_config.retention_period`, so retention is infinite and the disk grows without bound.
  - Tempo keeps 24 h; Prometheus keeps 7 d with no size cap.
  - Grafana has no volume, so UI-made dashboards and alerts are lost on recreate (and `allowUiUpdates: true` invites them).
  - Images are pinned by tag, not digest (`otel/opentelemetry-collector-contrib:0.163.2`, `prom/prometheus:v3.15.1`, `grafana/tempo:2.9.0`, `grafana/loki:3.7.9`, `grafana/grafana:13.2.4`).
  - Tempo runs as `user: "0"`.
  - The base compose publishes 9090/3100/3200 with Prometheus's OTLP receiver writable from the LAN (documented as a laptop stack).
- **Fix:** add Loki compactor retention, a Grafana volume and digest pins.

### C-20 — Low — No attribute-length limit for opt-in content capture
- **Evidence (verified):** `OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT` is unsupported by opentelemetry_sdk 0.33.1 (absent from its env reads). `rpc_span/agent.rs:197` records `gen_ai.input.messages` as `input.to_string()`, unbounded, when opted in.
- **Failure scenario:** a large file part or a long conversation produces multi-MB spans. These exceed the OTLP/gRPC 4 MiB default receive limit, so the whole batch is rejected and other spans are lost with it.
- **Fix:** truncate in the crate (e.g. 64 KiB) with a `…truncated` marker.
- **Verified by reading;** the batch rejection is conjecture.

### C-21 — Low — `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=delta` silently breaks the shipped stack
- **Evidence (verified):** upstream honours it (otlp `metric.rs:127-147`); Prometheus runs without `--enable-feature=otlp-deltatocumulative` (`docker-compose.yml`).
- **Fix:** document this, or add the feature flag, or add `deltatocumulative` in the Collector.

### C-22 — Low — No Kubernetes / Helm equivalent
- **Evidence (verified):** `ls deploy` shows `observability` only. No Chart.yaml, kustomization, ServiceMonitor/PodMonitor or PrometheusRule CRD. The README mentions Kubernetes probes only.
- **Fix:** ship a PrometheusRule rendering of the rules, Grafana dashboard ConfigMaps, an OTel Collector Helm values file, and a sample Deployment with `OTEL_*` env, `HOSTNAME`-based `service.instance.id` and probes.

### C-23 — Low — Instruments are rebuilt on every measurement
- **Files:** `rpc_span/tool_metrics.rs:200-209`, `:240-256`, `:348-375`; `store/traced.rs:136-141`; `rpc_span/card.rs:281`; `rpc_span/webhook.rs:191`.
- **Evidence (verified):** `self.meter.f64_histogram(...).with_boundaries(...).build().record(...)` runs per call.
- **Impact:** the SDK de-duplicates, but this takes a lock, a hash and a `Vec` clone of the boundaries per DB operation, model turn and webhook attempt. Hot-path overhead; conjecture on magnitude.
- **Fix:** cache the instruments, keyed by meter provider as `CallMetric` does.

### KNOWN (listed in handoff, not re-reported)
- H3: SLIMRPC multicast has no span or metric.
- M6–M8: WebSocket, SLIMRPC and tonic refusals are not metered.
- M10: client retries are not visible. Confirmed: `a2a-protocol-client/src/retry.rs:410,470` only `trace_warn!`, with no attempt counter and no `http.request.resend_count` on the RPC span.
- M11: push-config, audit, halt, lease and retention stores have no DB span.
- M12: card fetch, JWKS and token exchange have no duration metric.
- M15: the matrix does not cover the axum router or SLIMRPC.
- L1: the prometheus env behaviour is now `exit(2)`, see C-08.
- No SIGTERM flush in four binaries.
- `error.type` is not the SQLSTATE.

### Checked and fine (to save the next reader time)
- Batch queues are bounded (BSP/BLRP 2048).
- Export failures are rate-limited to one warn per 30 s per signal (`counting.rs`).
- Shutdown flushes all three providers with a 5 s timeout each (`telemetry/mod.rs` `shutdown_inner`).
- Inbound trace policy is `Continue`/`Restart` (`handler/helpers.rs:195-230`).
- The sampler is parent-based by default.
- Push `url.full` is scrubbed (`rpc_span/webhook.rs:225,239`), and push log lines use `origin_for_log` (`push/sender.rs:567`).
- GenAI message content is opt-in (`rpc_span/agent.rs:29-30`, `builder.rs:925-926`).
- `rpc.method` is normalised to a closed set (`rpc_span/names.rs`, client `call_span.rs` `qualified`), so it carries no cardinality from the wire.
- Collector pipelines are ordered `memory_limiter` → `batch`.
- `honor_labels: true` is set on the Collector scrape.
- Prometheus OTLP has `out_of_order_time_window: 30m`.
- OTLP log records carry trace and span ids (appender-tracing 0.33 built-in; handoff test `telemetry_export`).
