# Configuration Reference

Reference of the configuration options across a2a-rust crates; the builder and each type's rustdoc list the rest.

## Server Configuration

### RequestHandlerBuilder

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `with_agent_card` | `AgentCard` | Required | Discovery card for `/.well-known/agent-card.json`; the handler checks requests against what it declares |
| `with_extended_agent_card` | `AgentCard` | None: a card that declares `extendedAgentCard` answers `ExtendedAgentCardNotConfiguredError` | What `GetExtendedAgentCard` serves to an authenticated caller (§3.1.11, §13.3); checked and advertised on like the public card |
| `with_extended_card_provider` | `impl ExtendedCardProvider` | None: every caller gets the extended card as set | Chooses each caller's extended card from the one set (§13.3 MAY); its card is checked like the configured one |
| `without_agent_card()` | — | — | Build without a card, which `build()` otherwise refuses (spec §8.1): for a card published elsewhere, or a handler that is not a public A2A server |
| `with_task_store` | `impl TaskStore` | `InMemoryTaskStore`; `TenantAwareInMemoryTaskStore` when tasks are isolated by caller | Custom task storage backend |
| `with_task_store_config` | `TaskStoreConfig` | 1hr TTL, 10k capacity | TTL and capacity for default store; per caller partition when tasks are isolated by caller, so the store holds up to 1,000 times the capacity |
| `with_push_config_store` | `impl PushConfigStore` | `InMemoryPushConfigStore`; the tenant-aware one when the task store partitions by tenant | Custom push config storage |
| `with_push_sender` | `impl PushSender` | None | Webhook delivery implementation. Without one, every push-config operation answers `PushNotificationNotSupported`, and `build()` refuses a card that declares `pushNotifications` (spec §3.3.4) |
| `with_interceptor` | `impl ServerInterceptor` | Empty chain | Server middleware |
| `isolate_tasks_by_caller()` | — | On, with an authenticating interceptor | A partition per caller within a tenant (spec §13.1). On by default for the callers an authenticating interceptor identifies; calling it also refuses a request with no identity. See [Multi-tenancy](../deployment/multi-tenancy.md#callers-within-one-tenant) |
| `share_tasks_across_callers()` | — | Off | Every caller in a tenant sees its tasks, and `build()` accepts task and push-config stores that do not partition by tenant (a request that names a tenant is still refused over them unless `accept_unisolated_tenants()` is also called) |
| `with_executor_timeout` | `Duration` | 1 hour | Max time for executor completion (`DEFAULT_EXECUTOR_TIMEOUT`); `without_executor_timeout()` removes the bound |
| `with_event_queue_capacity` | `usize` | 256 | Bounded channel size per event queue. A reader that falls further behind than this catches up from the event log; raise it to reduce how often a slow reader has to. |
| `with_max_event_size` | `usize` | 16 MiB | Max serialized SSE event size |
| `with_max_concurrent_streams` | `usize` | 1,024 | Limit concurrent event queues: one per task being run, whether its send blocks or streams; a send that would open one more is refused as overloaded (pass `usize::MAX` to disable) |
| `with_metrics` | `impl Metrics` | `OtelMetrics` on the global meter provider (`otel`, on by default); `NoopMetrics` without it | Metrics observer for handler activity |
| `with_handler_limits` | `HandlerLimits` | See below | Configurable validation limits |
| `allow_undeclared_input_modes()` | — | Off | Accept message parts whose `mediaType` the card's input modes do not declare; by default, when the card declares any, such a part is refused with `ContentTypeNotSupportedError` |
| `with_tenant_resolver` | `impl TenantResolver` | None (single-tenant) | Derives each request's tenant; it partitions the tenant-aware stores and keys `with_tenant_config` limits |
| `require_resolved_tenant()` | — | Off | Refuse a request the resolver returns no tenant for, instead of serving the default `""` partition |
| `accept_unisolated_tenants()` | — | Off | By default a request naming a tenant is refused (`UnsupportedOperation`) unless both stores partition by tenant |
| `with_tenant_config` | `PerTenantConfig` | None | Per-tenant `max_concurrent_tasks`, `executor_timeout`, `event_queue_capacity` and `rate_limit_rps` (the last also needs `RateLimitInterceptor::with_tenant_config`) |
| `allow_unauthenticated_extended_card()` | — | Off | Serve `GetExtendedAgentCard` with no authenticating interceptor (§13.3); by default it is refused |
| `authenticated_upstream()` | — | Off | Accept a card whose `securityRequirements` ask for a credential although no interceptor here authenticates (a gateway does) |
| `with_additional_metrics` | `impl Metrics` | — | Also report to this observer, keeping the default |
| `refuse_private_file_urls()` | — | Off | Refuse file-part URLs into private, loopback or link-local networks |

### HandlerLimits

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_id_length` | `usize` | 1,024 | Maximum task/context ID length |
| `max_metadata_size` | `usize` | 1 MiB | Maximum serialized metadata size |
| `max_cancellation_tokens` | `usize` | 10,000 | Cleanup sweep threshold, not a hard cap: a live task's token is never removed |
| `max_token_age` | `Duration` | 1 hour | Stale token eviction age |
| `push_delivery_timeout` | `Duration` | 30s | Per-webhook delivery timeout, retries included (the sender's own request timeout is 10s) |
| `push_delivery_budget` | `Duration` | 30s | Time one event's deliveries are budgeted across all configs. Every config is attempted; the budget sets how many deliveries of an event run at once (`ceil(configs / (budget / push_delivery_timeout))`) |
| `push_queue_capacity` | `usize` | 1024 | Deliveries that may wait behind the one under way for one task. A full lane drops the oldest that does not end a turn (`dropped`); a terminal or interrupted status is never dropped. Zero is read as one |
| `executor_drain_timeout` | `Duration` | 5s | Bound on the blocking path's wait for the queue to close after the executor finished; also how long a continuation waits for a parked (`input-required`/`auth-required`) turn to finish: its executor to return and its events to be stored |
| `max_artifacts_per_task` | `usize` | 1,000 | Maximum artifacts per task (prevents O(n²) serialization) |
| `max_context_locks` | `usize` | 10,000 | Max per-context locks before cleanup |
| `max_push_configs_per_task` | `usize` | 100 | Maximum push configs per task (uniform across store backends) |
| `max_parts_per_artifact` | `usize` | 10,000 | Maximum parts a single artifact may accumulate |
| `max_total_push_configs` | `usize` | 100,000 | Global push-config ceiling across all tasks |
| `subscribe_reattach_interval` | `Duration` | 250ms | How often a `SubscribeToTask` stream between turns checks the store: for the task finishing, the next turn's queue, or events logged meanwhile, which it replays |
| `subscribe_max_idle` | `Option<Duration>` | none | If set, how long a `SubscribeToTask` stream waits on a parked task before ending without a terminal state; the client resubscribes. Unset, the stream ends at the terminal state or when the client goes (§3.1.6) |
| `subscribe_replay_limit` | `usize` | 1,000 | Maximum logged events one read of the event log returns, for a lag's catch-up, a missed turn or a `Last-Event-ID` resumption. The offset is client-supplied, so the cap bounds each read; a longer replay is read a page at a time |
| `final_task_snapshot` | `bool` | `false` | Whether a stream sends the finished task once more, as a final `Task`, after its terminal state (§11.7); sent only when the store shows the finished task within `subscribe_replay_catchup` |
| `subscribe_replay_catchup` | `Duration` | 2s | How long a resuming stream waits for the event log to catch up with what was already broadcast. Zero disables the wait; on expiry the shortfall is logged and counted under `event_log_catchup` |
| `store_write_retry` | `Duration` | 30s | How long a turn's event consumer goes on writing one event, to the log or the task, while the store answers it is unavailable for now (a lost connection, a restart, a lock or statement timeout): after 50 ms, then doubling up to 1 s. The events after it wait, in order. Zero writes each once; an outage longer than this still loses the event it was writing |

> **Build-time validation:** `max_id_length`, `max_metadata_size`, and
> `push_delivery_timeout` must be non-zero. Zero values are rejected by
> `RequestHandlerBuilder::build()`, which refuses more than that: see
> [Build-Time Validation](../building-agents/handler/configuration.md#build-time-validation)
> for the full list.

### TaskStoreConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_capacity` | `Option<usize>` | 10,000 | Maximum stored tasks; oldest terminal tasks are evicted on overflow, then, as a last resort, the oldest non-terminal ones (size it above your concurrent in-flight task count) |
| `task_ttl` | `Option<Duration>` | 1 hour | TTL for completed/failed tasks |
| `eviction_interval` | `u64` | 64 | Writes between automatic eviction sweeps |
| `max_page_size` | `u32` | 1,000 | Cap a store applies to the page size of a `list` call. The `ListTasks` method itself refuses a `pageSize` outside 1 to 100 (`InvalidParams`) and lists 50 when none is given, so this matters for direct `TaskStore::list` callers and when set below 100 |
| `max_events_per_task` | `Option<usize>` | 512 | Events one task's log keeps for `Last-Event-ID` resumption; `None` removes the bound. An attached stream that still owes older events holds them past it (spec §3.5.2) |
| `max_held_events_factor` | `Option<usize>` | 8 | How many times `max_events_per_task` a task's log may keep while a stream still owes its oldest events (4,096 at the defaults); past it the stream ends with `streamLagged`. `None` is no ceiling; `Some(1)` holds nothing past the bound |
| `idempotency_key_ttl` | `Option<Duration>` | 24 hours | How long an idempotency key is honoured, matching the SQL stores' one day; `None` keeps keys forever |
| `max_idempotency_keys` | `Option<usize>` | 100,000 | How many keys the index holds at once (per store, so per tenant partition); a send with a new key past it is refused with `503` / `RESOURCE_EXHAUSTED` and runs nothing, and no held key is evicted; `None` is no limit |

### InMemoryPushConfigStore

| Constructor | Default | Description |
|-------------|---------|-------------|
| `::new()` | 100 | Default max push configs per task |
| `::with_max_configs_per_task(N)` | — | Custom per-task push config limit |
| `.with_max_total_configs(N)` | 100,000 | Global ceiling across all tasks |

### DispatchConfig

Shared configuration for JSON-RPC, REST, and Axum dispatchers. Pass to
`JsonRpcDispatcher::with_config()`, `RestDispatcher::with_config()`, or
`A2aRouter::with_config()`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_request_body_size` | `usize` | 4 MiB | Larger bodies return 413 |
| `body_read_timeout` | `Duration` | 30s | Slow loris protection |
| `max_query_string_length` | `usize` | 4,096 | `RestDispatcher` only (not `A2aRouter`, whose query parsing is Axum's); longer queries return 414 |
| `sse_keep_alive_interval` | `Duration` | 30s | Periodic keep-alive comment interval for SSE streams |
| `sse_channel_capacity` | `usize` | 64 | SSE response body channel buffer size |
| `max_batch_size` | `usize` | 100 | Maximum requests in a JSON-RPC batch |
| `require_version_header` | `bool` | true | Reject a data-plane request with no `A2A-Version` header as `VersionNotSupported` (spec §3.6.2: absent means 0.3) |
| `agent_card_max_age` | `u32` | 3600 | `Cache-Control: max-age`, in seconds, for the public agent card; set it to how often the card changes (spec §8.6.1) |

### GrpcConfig

Configuration for the gRPC dispatcher (requires `grpc` feature).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_message_size` | `usize` | 4 MiB | Maximum inbound/outbound message size |
| `concurrency_limit` | `usize` | 256 | Max concurrent gRPC requests per connection |
| `stream_channel_capacity` | `usize` | 64 | Bounded channel for streaming responses |
| `require_version_header` | `bool` | true | Reject a call with no `a2a-version` metadata as `VersionNotSupported` |

### PushRetryPolicy

Configurable retry policy for `HttpPushSender`. Pass via
`HttpPushSender::with_retry_policy()`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_attempts` | `usize` | 3 | Maximum delivery attempts |
| `backoff` | `Vec<Duration>` | `[1s, 2s]` | Backoff durations between retries |

### RateLimitConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `requests_per_window` | `u64` | 100 | Max requests per caller per window |
| `window_secs` | `u64` | 60 | Window duration in seconds |
| `trusted_proxy_hops` | `usize` | 0 | How many `X-Forwarded-For` hops to trust (0 = ignore XFF entirely) |
| `max_buckets` | `usize` | 10,000 | Hard bound on tracked caller buckets (fail-closed when full) |

### ServeConfig

Limits for a `Server` (`Server::bind(addr).await?.with_config(config)`). The
three shutdown phases run in order — `completion_grace`, then `task_grace`, then
`drain_timeout` — and the gRPC and WebSocket dispatchers' `serve_with_shutdown`
take the same three through their own `with_completion_grace`,
`with_task_grace` and `with_drain_timeout`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_connections` | `Option<usize>` | None | Ceiling on connections served at once; `None` is unbounded |
| `completion_grace` | `Duration` | 5s | On shutdown, how long in-flight tasks get to finish on their own before they are cancelled (`DEFAULT_COMPLETION_GRACE`) |
| `task_grace` | `Duration` | 10s | How long cancelled tasks get to cancel what they delegated and write a terminal event (`DEFAULT_TASK_GRACE`) |
| `drain_timeout` | `Duration` | 15s | How long connections get to finish before they are reported abandoned (`DEFAULT_DRAIN_TIMEOUT`) |
| `header_read_timeout` | `Option<Duration>` | 30s | How long a peer may take to send complete request headers; `None` disables |
| `idle_timeout` | `Option<Duration>` | 75s | How long a connection may carry no traffic before it is closed; `None` disables |
| `production` | `bool` | false | `ServeConfig::production()`: the server returns at once, accepting nothing, with `ServeReport::refused` saying why, when it would serve plaintext beyond loopback (unless `tls_terminated_in_front`) or a binding the handler's agent card does not declare |
| `tls_terminated_in_front` | `bool` | false | TLS ends before this listener (load balancer, ingress, mesh); silences the plaintext warning and lifts the `production` refusal for it (`with_tls_terminated_in_front()`) |

### Other server types

| Type | Defaults |
|------|----------|
| `CorsConfig::new(origin)` | `allow_methods` `GET, POST, PUT, DELETE, OPTIONS`; `allow_headers` `content-type, authorization, a2a-version, a2a-extensions, last-event-id, x-a2a-notification-token`; `max_age_secs` 86400 |
| `TenantStoreConfig` | `max_tenants` 1,000 partitions (each caller's partition counts when tasks are isolated by caller); `max_total_tasks` none |
| `WebSocketDispatcher` | handshake timeout 10s (`with_handshake_timeout`); idle timeout off (`with_idle_timeout`; `DEFAULT_WS_IDLE_TIMEOUT` is 75s); `with_max_connections` unbounded; 64 concurrent requests per connection; 4 MiB messages |
| `HttpPushSender` | 10s request timeout (`HttpPushSender::with_timeout`), 5s DNS lookup bound (`with_dns_timeout`), `PushRetryPolicy` above; SSRF guard on unless `allow_private_urls()` |
| `JwtValidator`, `JwtAuthInterceptor` | clock-skew leeway 60s (`JwtValidator::with_leeway`); a remote JWKS is cached for 3,600s (`JwtAuthInterceptor::with_jwks_ttl`) |

### Internal Limits

| Limit | Value | Description |
|-------|-------|-------------|
| Event queue type | `broadcast` | Fan-out to multiple subscribers; a reader that falls behind the ring catches up from the task's event log, and when the log cannot supply what it missed it receives a `streamLagged` error and its stream ends (no silent gap) |
| Rate limiter cleanup interval | 256 checks | Stale buckets (from departed callers) evicted every 256 `check()` calls |
| Rate limiter window CAS | Lock-free | Window transitions use `compare_exchange` to avoid TOCTOU races |
| Credential store poisoning | Fail-fast | `InMemoryCredentialsStore` panics on poisoned locks rather than returning `None` |

## Client Configuration

### ClientBuilder

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `with_protocol_binding` | `impl Into<String>` | `"JSONRPC"` (`new`); the card's first supported interface (`from_card`); the first ranked one the card offers (`from_card_preferring`) | Transport: `"JSONRPC"`, `"HTTP+JSON"` (alias `"REST"`), or `"GRPC"` — the last needs `build_grpc()`; `build()` refuses it |
| `with_timeout` | `Duration` | 30s | Per-request timeout, except a blocking send (below) |
| `with_send_timeout` | `Duration` | 600s | Bound on a `SendMessage` that waits for the turn to end (`return_immediately` false, not streaming): the agent's work, not a round trip |
| `with_connection_timeout` | `Duration` | 10s | TCP connection timeout |
| `with_stream_connect_timeout` | `Duration` | 30s | Establishing a stream: until the response headers (gRPC: until the call is accepted) |
| `with_stream_first_event_timeout` | `Duration` | 5 min | Wait for a stream's first data (an event or a keep-alive) once established |
| `with_max_event_size` | `usize` | 16 MiB | Largest single stream event accepted; larger ones are refused and skipped |
| `with_stream_idle_timeout` | `Option<Duration>` | 5 min | Longest an established stream may receive nothing — keep-alive comments count — after its first data; `None` disables |
| `with_retry_policy` | `RetryPolicy` | None | Retry on transient errors with jittered backoff |
| `with_accepted_output_modes` | `Vec<String>` | `["text/plain", "application/json"]` | MIME types accepted |
| `with_history_length` | `u32` | None | Messages in responses |
| `with_return_immediately` | `bool` | false | Don't wait for completion |
| `with_tenant` | `impl Into<String>` | None (auto from `AgentCard`) | Tenant sent on **every** request (spec §8.3.2 rule 4); a per-request `tenant` overrides it |
| `with_grpc_bare_address_scheme` | `GrpcBareAddressScheme` | `HttpsExceptLoopback` | How `build_grpc` dials a card's bare `host:port` gRPC target: TLS except for loopback (default), always TLS, or always plaintext |
| `with_tls_config` | `rustls::ClientConfig` | Mozilla roots plus `SSL_CERT_FILE` / `SSL_CERT_DIR` | `tls-rustls` feature: the TLS the JSON-RPC and HTTP+JSON bindings use — a private CA, a pinned self-signed certificate, or a client certificate (`tls::tls_config_from_pem` builds one from PEM). Proxies come from `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` / `NO_PROXY`, for every binding: WebSocket and gRPC through a `CONNECT` tunnel |
| `with_grpc_tls_config` | `ClientTlsConfig` | None (bundled Mozilla roots) | `grpc-tls` feature: the TLS settings `build_grpc` uses for an `https://` endpoint, explicit or policy-chosen — a private CA, a client certificate, or a server name that differs from the host |
| `with_interceptor` | `impl CallInterceptor` | Empty chain | Client middleware |
| `with_max_response_size` | `usize` | 32 MiB | Largest buffered (non-streaming) response body; JSON-RPC and REST enforce it and gRPC takes it as its `max_message_size`. Not applied to a transport given to `with_custom_transport` (`WebSocketTransport` has its own `max_message_size`) |
| `with_custom_transport` | `impl Transport` | None | Use this transport instead of the one built from the endpoint and binding |
| `with_trace_propagation` | `bool` | true | Whether each call's `traceparent` names the call's own span (`otel`) |
| `require_https_webhooks()` | — | Off | Refuse a push config whose webhook URL is plain `http://` to a non-loopback host (`ClientError::InsecureWebhook`, spec §13.2) |
| `require_https_credentials()` | — | Off | Refuse a call that carries a credential to a non-loopback `http://`/`ws://` endpoint (spec §13.4) |
| `without_tls()` | — | TLS on with `tls-rustls` | Plain HTTP only; `build` refuses an `https://` endpoint |
| `without_capability_checks()` | — | Off | A client built from a card refuses, before sending, a call that needs a capability the card does not declare (`ClientError::CapabilityNotDeclared`); this sends them anyway |
| `with_extensions` / `skip_required_extension_check()` | extension URIs | none | Ask the agent to use these extensions on every request (`A2A-Extensions`). A client built from a card refuses to build when the card marks an extension `required` that it did not ask for; `skip_required_extension_check()` builds anyway |

`build()` returns an error if `request_timeout`, `send_timeout`, `stream_connect_timeout` or `connection_timeout` is zero.

### GrpcTransportConfig

Configuration for the gRPC client transport (requires `grpc` feature). The
struct is `#[non_exhaustive]`: build it with `GrpcTransportConfig::default()`
and the `with_*` setters.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `timeout` | `Duration` | 30s | Per-request timeout, except a blocking send |
| `send_timeout` | `Duration` | 600s | Bound on a blocking `SendMessage` (see `with_send_timeout` above) |
| `connect_timeout` | `Duration` | 10s | Connection timeout |
| `max_message_size` | `usize` | 4 MiB | Maximum message size |
| `stream_channel_capacity` | `usize` | 64 | Streaming response buffer |
| `bare_address_scheme` | `GrpcBareAddressScheme` | `HttpsExceptLoopback` | Scheme used for a bare `host:port` target (an `http://`/`https://` URL is used as-is) |
| `tls_config` | `Option<ClientTlsConfig>` | None (bundled Mozilla roots) | `grpc-tls` feature: pin a private CA or present a client certificate for `https://` endpoints |

### RetryPolicy

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_retries` | `u32` | 3 | Maximum retry attempts |
| `initial_backoff` | `Duration` | 500ms | Backoff before first retry |
| `max_backoff` | `Duration` | 30s | Caps exponential growth |
| `backoff_multiplier` | `f64` | 2.0 | Multiplier per retry |

### SSE Parser Limits

| Limit | Value | Description |
|-------|-------|-------------|
| Event size | 16 MiB (`ClientBuilder::with_max_event_size`) | Largest single stream event; larger ones are refused with an error and skipped, and a line that outgrows it is refused as soon as it does (aligned with server) |
| Stream connect timeout | 30s (`with_stream_connect_timeout`) | Until the response headers; the first event then has its own 5-minute bound |

## HTTP Caching (Agent Card)

| Header | Default | Description |
|--------|---------|-------------|
| `Cache-Control` | `public, max-age=3600` | Configurable max-age (default 1 hour) |
| `ETag` | Auto-computed | Content hash |
| `Last-Modified` | Static handler: when it loaded the card | Not sent by the dynamic handler, which has no modification date |

## Feature Flags

### `a2a-protocol-server`

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | Forwards `a2a-protocol-types/signing`; the server itself neither signs nor verifies the card it serves |
| `tracing` | **On** | Structured logging via `tracing` crate; `default-features = false` compiles it out |
| `tls-rustls` | Off | HTTPS delivery for the bundled push-notification sender, `Server::with_tls` / `with_tls_pem` (HTTPS on its own listener), and `auth::MutualTlsAuthInterceptor` |
| `sqlite` | Off | SQLite-backed task and push config stores via `sqlx` |
| `postgres` | Off | PostgreSQL-backed task and push config stores via `sqlx` |
| `websocket` | Off | WebSocket transport via `tokio-tungstenite` |
| `grpc` | Off | gRPC transport via `tonic` (plaintext listener) |
| `grpc-tls` | Off | TLS on the gRPC listener itself: `GrpcDispatcher::with_tls(ServerTlsConfig)` with a server identity and, optionally, a client CA for mutual TLS; implies `grpc`; the TLS types are re-exported from `dispatch::grpc` |
| `otel` | On | `OtelMetrics` and the spans' OpenTelemetry bridge, through the OpenTelemetry API only (implies `tracing`) |
| `otlp` | Off | `Telemetry`: OTLP export of traces, metrics and logs (gRPC or HTTP/protobuf), the Langfuse preset, and `init_otlp_pipeline` (implies `otel`) |
| `otlp-json` | Off | OTLP/HTTP with JSON bodies (`OtlpProtocol::HttpJson`); off because `opentelemetry-otlp`'s `http-json` also makes JSON the default of any exporter built without a protocol (implies `otlp`) |
| `otel-prometheus` | Off | `OTEL_METRICS_EXPORTER=prometheus`: serve the metrics at `/metrics` for a Prometheus that scrapes, with no collector in between; off by default because it adds `opentelemetry-prometheus` and a listener (implies `otlp`) |
| `conformance` | Off | A harness that grades an `AgentExecutor` against the protocol's invariants |
| `axum` | Off | Axum framework integration (`A2aRouter`) |
| `tool-loop` | Off | Tool calling inside a task: `tool_loop::ToolLoop`, `ToolLoopExecutor`, `OpenAiChatModel` |
| `auth-jwt` | Off | JWT bearer-token authentication (`JwtAuthInterceptor`) |
| `encryption` | Off | Encryption at rest for task content (`EncryptedTaskStore`) |
| `audit` | Off | Tamper-evident audit trail (`RequestHandlerBuilder::with_audit`, ADR 0015): calls, runs, task events and cancel requests in a SHA-256 hash chain per tenant, signed checkpoints, legal holds, a six-month retention floor; SQLite/PostgreSQL audit stores with those features |

### `a2a-protocol-client`

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | Forwards `a2a-protocol-types/signing`; the client itself neither signs nor verifies anything |
| `tracing` | **On** | Structured logging via `tracing` crate, and a `CLIENT` span per call; `default-features = false` compiles both out |
| `otel` | On | Each call's `traceparent` names the call's own span when a `tracing-opentelemetry` layer records it (implies `tracing`) |
| `tls-rustls` | **On** | HTTPS via rustls (no OpenSSL dependency); `default-features = false` for a plaintext-only build |
| `websocket` | Off | WebSocket transport via `tokio-tungstenite` |
| `grpc` | Off | gRPC transport via `tonic` (plaintext; `https://` is refused with a message naming `grpc-tls`) |
| `grpc-tls` | Off | gRPC over TLS: implies `grpc` (not `tls-rustls`), verifies against the bundled Mozilla roots or a supplied `ClientTlsConfig` (re-exported from `transport::grpc`) |
| `testing` | Off | A scripted hostile peer (`testing::ScriptedPeer`) that stalls, cuts off, mis-frames or refuses on each binding, for testing code that calls agents |
| `webhook` | **On** | The push-notification webhook receiver (`webhook::WebhookReceiver`): token and `Authorization` checks, task-id check, duplicate suppression, optional rate limit (spec §4.3.3, §13.2) |

### `a2a-protocol-types`

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | JWS/ES256 agent card signing (RFC 8785 canonicalization) |
| `audit` | Off | Audit record schema, hash chains, ES256/Ed25519-signed checkpoints and chain verification (`audit::verify_chain`); implies `signing` |
| `proto` | Off | Canonical protobuf message types and the JSON⇄proto conversions (turned on by `grpc`) |

### `a2a-protocol-sdk` (umbrella)

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | Enables `signing` in all sub-crates |
| `tracing` | **On** | Enables `tracing` in client and server |
| `tls-rustls` | **On** | Enables `tls-rustls` in client and server |
| `grpc` | Off | Enables `grpc` in client and server |
| `grpc-tls` | Off | Enables `grpc-tls` in the client and the server (and `tls-rustls`): `https://` dialling on one side, `GrpcDispatcher::with_tls` on the other |
| `websocket` | Off | Enables `websocket` in client and server |
| `sqlite` | Off | Enables `sqlite` in the server |
| `postgres` | Off | Enables `postgres` in the server |
| `otel` | On | Enables `otel` in the server and the client |
| `otlp` | On | Enables the server's `otlp` (implies `otel`) |
| `otlp-json` | Off | Enables the server's `otlp-json` (implies `otlp`) |
| `otel-prometheus` | Off | Enables the server's `otel-prometheus`: `OTEL_METRICS_EXPORTER=prometheus` serves `/metrics` (off by default; implies `otlp`) |
| `axum` | Off | Enables `axum` in the server |
| `tool-loop` | Off | Enables `tool-loop` in the server |
| `auth-jwt` | Off | Enables `auth-jwt` in the server |
| `encryption` | Off | Enables `encryption` in the server |
| `audit` | Off | Enables `audit` in the server and in `a2a-protocol-types` |
| `webhook` | **On** | Enables `webhook` in the client |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level filter (when the `tracing` feature is enabled — the client's, server's and SDK's default — and the application's subscriber reads it) |
| `OTEL_*` | Read by `Telemetry` (server `otlp` feature, on in the SDK) and the OpenTelemetry crates beneath it: `OTEL_SDK_DISABLED`, `OTEL_TRACES_EXPORTER` / `OTEL_METRICS_EXPORTER` / `OTEL_LOGS_EXPORTER` (a list of `otlp`, `console` and, for metrics with the `otel-prometheus` feature, `prometheus`; or `none`), `OTEL_EXPORTER_OTLP_ENDPOINT` / `_PROTOCOL` / `_HEADERS` / `_COMPRESSION` / `_TIMEOUT` (10 s) / `_CERTIFICATE` / `_CLIENT_CERTIFICATE` / `_CLIENT_KEY` and their per-signal forms, `_INSECURE` (only for a scheme-less OTLP/gRPC endpoint), `OTEL_EXPORTER_PROMETHEUS_HOST` / `_PORT`, `OTEL_PROPAGATORS`, `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`; and `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` for the export; see [Observability](../deployment/observability.md) |
| `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY`, `LANGFUSE_BASE_URL` | Read by `Langfuse::from_env()`; the base URL defaults to `http://localhost:3000` |
| `PROTOC` | The `proto`/`grpc` build scripts use a vendored `protoc` unless this names another |

Examples:
```bash
RUST_LOG=info              # Info and above
RUST_LOG=debug             # Debug and above
RUST_LOG=a2a_protocol_server=debug  # Debug for server crate only
RUST_LOG=a2a_protocol_server=trace,a2a_protocol_client=debug  # Per-crate levels
```

## Next Steps

- **[API Quick Reference](./api-reference.md)** — All public types at a glance
- **[Pitfalls & Lessons Learned](./pitfalls.md)** — Known issues and workarounds
