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
| `push_suspend_after` | `usize` | 5 | Failed deliveries to one webhook in a row before it is suspended; zero turns suspension off. A suspended webhook is skipped (`suspended`) for `push_suspend_for`, then one event is tried; the events that end a turn are always tried. Per replica, in memory |
| `push_suspend_for` | `Duration` | 30s | How long a suspended webhook is left alone before one event is tried again; each failed probe doubles it, up to 32 times as long |
| `push_remove_gone` | `bool` | `true` | Remove the config of a webhook that answers `410 Gone` (`gone`). Only `410`: a `404` counts as any other failure |
| `push_max_in_flight` | `usize` | 512 | Webhook deliveries in flight at once across every task. A delivery over the bound waits, so every config is still attempted. The wait is bounded by `push_delivery_timeout` but does not spend it: the webhook's time starts when the delivery is let through, and one never let through is the outcome `throttled` (until 0.16.0 the wait counted against the timeout). Each delivery that waited is counted once as the error `push_throttled` on method `PushDelivery`. Covers any sender behind the handler, a custom one included. Zero is read as one. A choice, not a measurement |
| `push_max_in_flight_per_host` | `usize` | 64 | Deliveries in flight at once to one destination (host and port). Bounds how many requests a third party sees from this agent when a caller registers many webhooks at its address; not a rate limit. The same bound holds on each address the bundled sender connects to, so names that resolve to one address (a wildcard record) share its count. Raise it for one receiver that takes every agent's callbacks. Zero is read as one |
| `push_max_in_flight_per_partition` | `usize` | 256 | Deliveries in flight at once for one store partition (a tenant, or a tenant and caller where callers are isolated), so one caller's slow webhooks cannot hold every overall permit. A share, not a reservation: a deployment with one partition runs at most this many at once, so raise it with `push_max_in_flight` there. Zero is read as one |
| `push_outbox_max_age` | `Duration` | 24h | With `with_push_outbox`: a delivery still not delivered this long after it was recorded is dropped when next due and counted `expired`. A choice, not a measurement |
| `push_outbox_backoff_max` | `Duration` | 1h | With `with_push_outbox`: the longest wait between two attempts of one delivery; the wait doubles from 1 s up to this, less up to a fifth of jitter |
| `executor_drain_timeout` | `Duration` | 5s | Bound on the blocking path's wait for the queue to close after the executor finished; also how long a continuation waits for a parked (`input-required`/`auth-required`) turn to finish: its executor to return and its events to be stored |
| `max_artifacts_per_task` | `usize` | 1,000 | Maximum artifacts per task (prevents O(n²) serialization) |
| `max_context_locks` | `usize` | 10,000 | Max per-context locks before cleanup |
| `max_push_configs_per_task` | `usize` | 100 | Maximum push configs per task (uniform across store backends) |
| `max_parts_per_artifact` | `usize` | 10,000 | Maximum parts a single artifact may accumulate |
| `max_total_push_configs` | `usize` | 100,000 | Global push-config ceiling across all tasks |
| `subscribe_reattach_interval` | `Duration` | 250ms | The first wait of a `SubscribeToTask` stream between turns before it checks the store: for the task finishing, the next turn's queue, or events logged meanwhile, which it replays. It doubles while nothing changes, up to the next row, and starts over at any change |
| `subscribe_reattach_max_interval` | `Duration` | 4s | The longest wait between two checks of an idle `SubscribeToTask` stream (audit E-F25); below the interval, no back-off. The first frame after a long quiet can be this late. `subscribe_max_idle`, when set, is still met at its own time |
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
| `max_capacity` | `Option<usize>` | 10,000 | Maximum stored tasks; oldest terminal tasks are evicted on overflow, then, as a last resort, the oldest *working* ones (counted: `InMemoryTaskStore::live_evictions`); a task waiting for input or authorization is never evicted, and a full store of them refuses a new task with `503` (size it above your concurrent in-flight and parked task count) |
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
| `base_path` | `Option<String>` | none | `with_base_path("/agents/billing")`: a path prefix a reverse proxy keeps, taken off a request's path before routing (the card is then under the prefix, a tenant segment after it); a request outside it is routed as before |

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
| `trusted_proxies` | `Vec<TrustedProxy>` | `[]` | Address ranges the proxies connect from; when set, XFF is believed only from a peer in one (no effect while `trusted_proxy_hops` is 0). `RateLimitInterceptor::new` logs a warning when hops are set and this is empty |
| `forwarded_header` | `ForwardedHeader` | `XForwardedFor` | The header the proxies write the client address into: `X-Forwarded-For` or RFC 7239 `Forwarded` (its `for=` parameter); only that one is read |
| `max_buckets` | `usize` | 10,000 | Bound on tracked caller buckets. When full of live buckets, callers without one share a single `overflow` bucket holding one caller's limit (`RateLimitInterceptor::overflowed()` counts its requests); they used to be refused outright, which locked every new caller out while `max_buckets` callers stayed active |

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
| `WebSocketDispatcher` | handshake timeout 10s (`with_handshake_timeout`); write timeout 30s (`with_write_timeout`: a peer that stops reading is dropped at it, and the connection with it); idle timeout `DEFAULT_WS_IDLE_TIMEOUT`, 75s, pinging at half (`with_idle_timeout`, `without_idle_timeout`; off until 0.16.0); `with_max_connections` unbounded; 64 concurrent requests per connection; 4 MiB messages |
| `GrpcDispatcher` | HTTP/2 keep-alive: a PING after 60s idle (`DEFAULT_GRPC_KEEPALIVE_INTERVAL`), closing at 20s unanswered (`DEFAULT_GRPC_KEEPALIVE_TIMEOUT`; `with_http2_keepalive`, `without_http2_keepalive`; off until 0.16.0); `with_max_connections` unbounded; `with_max_connection_age` off; under `with_rustls` a 10s TLS handshake bound |
| `ReloadingCertResolver` | nothing re-read until `reload()`, or every interval given to `watch(interval)` |
| `HttpPushSender` | 10s request timeout (`HttpPushSender::with_timeout`), 5s DNS lookup bound (`with_dns_timeout`), `PushRetryPolicy` above; SSRF guard on unless `allow_private_urls()` |
| `JwtValidator`, `JwtAuthInterceptor` | clock-skew leeway 60s (`JwtValidator::with_leeway`); a remote JWKS is cached for 3,600s (`JwtAuthInterceptor::with_jwks_ttl`) and refetched for an unknown key id at most every 10s (`with_jwks_refetch_interval`) |

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
| `with_grpc_tls_config` | `ClientTlsConfig` | Mozilla roots plus `SSL_CERT_FILE` / `SSL_CERT_DIR`; one you give is exact (`transport::grpc::tls_config_from_pem` keeps the defaults) | `grpc-tls` feature: the TLS settings `build_grpc` uses for an `https://` endpoint, explicit or policy-chosen — a private CA, a client certificate, or a server name that differs from the host |
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
| `http2_keep_alive_interval` | `Option<Duration>` | None (off) | Interval of an HTTP/2 PING on the connection, quiet or not, so a load balancer's idle timeout does not close a long silent stream (`with_http2_keep_alive`, or `ClientBuilder::with_grpc_keep_alive`); off because a PING is hop-by-hop and grpc-go refuses a short interval |
| `http2_keep_alive_timeout` | `Duration` | 20s | How long to wait for the answer to such a PING before the connection is dropped; read only with the interval |
| `bare_address_scheme` | `GrpcBareAddressScheme` | `HttpsExceptLoopback` | Scheme used for a bare `host:port` target (an `http://`/`https://` URL is used as-is) |
| `tls_config` | `Option<ClientTlsConfig>` | None (bundled Mozilla roots) | `grpc-tls` feature: pin a private CA or present a client certificate for `https://` endpoints |

### RetryPolicy

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_retries` | `u32` | 3 | Maximum retry attempts |
| `initial_backoff` | `Duration` | 500ms | Backoff before first retry |
| `max_backoff` | `Duration` | 30s | Caps exponential growth |
| `backoff_multiplier` | `f64` | 2.0 | Multiplier per retry |
| `budget` | `Option<RetryBudget>` | None (off) | Retries as a bounded fraction of calls, so retries cannot multiply an overload (`with_budget`); a retry the budget cannot pay for is not made |
| `max_elapsed` | `Option<Duration>` | None (off) | Longest from a call's first attempt to the start of its last retry, backoffs included (`with_max_elapsed`) |

### HttpPool (`HttpPool::builder()`)

One set of HTTP connections that many clients share
(`ClientBuilder::with_pool`; see [Sharing connections](../client/builder/pool.md)).
Every setting is fixed when `build()` is called. A zero duration is refused.

| Setter | Type | Default | Description |
|--------|------|---------|-------------|
| `with_tls_config` | `ClientConfig` | Mozilla roots plus `SSL_CERT_FILE` / `SSL_CERT_DIR` | The roots every connection of the pool trusts and any client certificate it presents. A client given the pool and its own TLS configuration fails at `build()`. |
| `with_connect_timeout` | `Duration` | 10s | TCP connect timeout. A client given the pool and `with_connection_timeout` fails at `build()`. |
| `with_idle_timeout` | `Duration` | 50s | How long an unused connection is kept before it is closed in the background |
| `with_max_idle_per_host` | `usize` | no limit | Idle connections kept per host |
| `with_max_connection_lifetime` | `Duration` | not recycled | Use new connections once the current ones are this old, spread by up to 10% either way; calls and streams already running are not cut |
| `with_http2_keep_alive` | `(Duration, Duration)` | no PING | `(interval, timeout)`: an HTTP/2 PING every `interval`, idle or not, and a connection that does not answer within `timeout` is closed. Applies to connections that speak HTTP/2 |
| `with_http2_prior_knowledge` | — | negotiate | Speak HTTP/2 without negotiating it, including over plain `http://` |

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
| `Last-Modified` | Static handler: the running executable's modification time (replicas of one build agree); `with_last_modified` sets it | Not sent by the dynamic handler, which has no modification date |

## Feature Flags

### `a2a-protocol-server`

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | Forwards `a2a-protocol-types/signing`; the server itself neither signs nor verifies the card it serves |
| `tracing` | **On** | Structured logging via `tracing` crate; `default-features = false` compiles it out |
| `tls-rustls` | **On** | HTTPS delivery for the bundled push-notification sender, JWKS/OIDC fetches and the model client, `Server::with_tls` / `with_tls_pem` (HTTPS on its own listener), and `auth::MutualTlsAuthInterceptor`; on by default since 0.16.0 |
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
| `A2A_PROXY_LOOPBACK` | Read by the client and the server's outbound HTTP, gRPC and WebSocket paths. Loopback (`localhost`, `127.0.0.0/8`, `::1`) is never sent to the proxy named by `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` unless this is `1`, `true`, `yes` or `on`. `NO_PROXY` entries are read as Go reads them: `*.example.com` works, an entry with a port is ignored with a warning. |
| `OTEL_*` | Read by `Telemetry` (server `otlp` feature, on in the SDK) and the OpenTelemetry crates beneath it: `OTEL_SDK_DISABLED`, `OTEL_TRACES_EXPORTER` / `OTEL_METRICS_EXPORTER` / `OTEL_LOGS_EXPORTER` (a list of `otlp`, `console` and, for metrics with the `otel-prometheus` feature, `prometheus`; or `none`), `OTEL_EXPORTER_OTLP_ENDPOINT` / `_PROTOCOL` / `_HEADERS` / `_COMPRESSION` / `_TIMEOUT` (10 s) / `_CERTIFICATE` / `_CLIENT_CERTIFICATE` / `_CLIENT_KEY` and their per-signal forms, `_INSECURE` (only for a scheme-less OTLP/gRPC endpoint), `OTEL_EXPORTER_PROMETHEUS_HOST` / `_PORT`, `OTEL_PROPAGATORS`, `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`; and `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` for the export; see [Observability](../deployment/observability.md) |
| `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY`, `LANGFUSE_BASE_URL` | Read by `Langfuse::from_env()`; the base URL defaults to `http://localhost:3000` |
| `PROTOC` | The `proto`/`grpc` build scripts use a vendored `protoc` unless this names another |
| `SSL_CERT_FILE`, `SSL_CERT_DIR` | A PEM file, and a directory of PEM files, of CA certificates trusted beside the bundled Mozilla roots: by the client (every binding, gRPC included, card discovery, JWKS and the token provider) and by the server's push delivery, JWKS and OIDC discovery and the tool loop's model client (telemetry export takes `OTEL_EXPORTER_OTLP_CERTIFICATE` instead). Read once per process; a file that cannot be read is reported and skipped, never replacing the bundled roots |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` (and lower-case) | Not only the telemetry export's: the client's HTTP, gRPC and WebSocket transports, and the server's push delivery, JWKS and OIDC fetches go through them; loopback never does unless `A2A_PROXY_LOOPBACK` says so |
| `HOSTNAME` (`COMPUTERNAME` on Windows) | The replica's name in a turn claim, and the telemetry resource's `host.name`; Kubernetes sets it to the pod's name |
| `NO_COLOR` | Set, the subscriber `Telemetry` installs writes no colour |
| `A2A_TEST_POSTGRES_URL` | Tests only: the PostgreSQL the `#[ignore]`d live tests run against (`scripts/run_postgres_suites.py`) |

### What each binary in the repository reads

The SDK's crates read only the variables above. Each example and tool reads
its own, and they were named one binary at a time, so the same setting has
more than one spelling ([ADR 0040](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0040-first-run-configuration-and-scope.md)
records why none is renamed in 0.16.0, and the rule for new ones: a platform's
name where a platform injects it, `A2A_` otherwise). Every one of them also
reads `OTEL_*`. Read from each crate's source (`std::env::var` calls) on
2026-11-04.

| Setting | Variable | Read by |
|---|---|---|
| Where to listen | `PORT` (all addresses) | `deploy-agent` |
| | `A2A_BIND_ADDR` (`host:port`) | `echo-agent`, `genai-agent`, `rig-agent`, `mcp-agent`, `multi-lang-team` (each starts its server mode when it is set) |
| | `SUT_HOST` | `tck/sut` |
| | none: `127.0.0.1:3000` | `hello-agent` |
| Extra listeners | `A2A_GRPC_BIND_ADDR`, `A2A_WS_BIND_ADDR` | `echo-agent` |
| | `A2A_GRPC_ADDR`, `A2A_WS_ADDR` | `genai-agent` |
| | `SUT_GRPC_HOST`, `SUT_WS_HOST` | `tck/sut` |
| The URL on the agent card | `AGENT_URL` | `deploy-agent` |
| | `A2A_PUBLIC_URL` | the five `A2A_BIND_ADDR` examples; unset, the card names the bound address, or `http://localhost:<port>` for a bind to every address (`0.0.0.0`, `[::]`), which a client elsewhere cannot use |
| | `SUT_ADVERTISE_URL`, `SUT_GRPC_ADVERTISE_URL`, `SUT_WS_ADVERTISE_URL` | `tck/sut` |
| Another agent to call | `A2A_AGENT_URL` (the remote agent: not the card URL above, despite the name) | `mcp-bridge` |
| Database | `A2A_DATABASE_URL`, else `DATABASE_URL` with a warning; `DATABASE_POOL_SIZE`, `DATABASE_PUSH_POOL_SIZE`, `TASK_RETENTION_HOURS` | `deploy-agent` |
| | `A2A_TEST_POSTGRES_URL` | `resilient-agent`, `incident-response` (their PostgreSQL acts) |
| Shutdown and version | `DRAIN_DELAY_SECONDS`, `AGENT_VERSION` | `deploy-agent` |
| Model endpoint | `OPENAI_BASE_URL`, `OPENAI_API_KEY` | `rig-agent`, `mcp-agent` |
| | `TOOL_LOOP_BASE_URL`, `TOOL_LOOP_API_KEY` | `tool-loop-agent` |
| | `LIVE_BASE_URL` | `live-model` |
| | `SWARM_LLM_URL` | `swarm` |
| Model, thinking, output cap | `GENAI_MODEL`, `GENAI_THINKING`, `GENAI_MAX_TOKENS` | `genai-agent` |
| | `RIG_MODEL`, `RIG_THINKING`, `RIG_MAX_TOKENS` | `rig-agent` (`mcp-agent` reads `RIG_MODEL` too, with `MCP_THINKING`, `MCP_MAX_TOKENS`) |
| | `INCIDENT_MODEL`, `INCIDENT_THINKING`, `INCIDENT_MAX_TOKENS` | `incident-response` |
| | `LIVE_MODEL`, `LIVE_THINKING`, `LIVE_EXTRA_BODY`, `LIVE_MODEL_TIMEOUT_SECS` | `live-model` |
| | `TOOL_LOOP_MODEL`, `TOOL_LOOP_THINKING`, `TOOL_LOOP_EXTRA_BODY`, `TOOL_LOOP_MODEL_TIMEOUT_SECS` | `tool-loop-agent` |
| Authentication and limits | `A2A_API_KEY`, `A2A_BEARER_TOKEN`, `A2A_JWT_HS256_SECRET`, `A2A_JWT_ISS`, `A2A_JWT_AUD`, `A2A_MAX_CONCURRENT_TASKS`, `A2A_ALLOW_PRIVATE_WEBHOOKS`, `A2A_ALLOW_FALLBACK` | `genai-agent` |
| | `A2A_INTEROP_API_KEY`, `A2A_INTEROP_CARD` | `echo-agent` (the Go interop legs) |
| Test-run switches | `RESILIENT_REQUIRE_ALL`, `INCIDENT_REQUIRE_ALL`, `INCIDENT_EXIT_WHEN_DONE`, `SWARM_*` sizes, `SWARM_DEBUG`, `SUT_PROFILE` | `resilient-agent`, `incident-response`, `swarm`, `tck/sut` |
| Other | `MCP_SERVER_BIN` | `mcp-agent` (the tool server it spawns) |
| | `LANGFUSE_*` | `langfuse-agent`, through `Langfuse::from_env()` |
| | none of its own | `a2a` (the CLI is configured by flags; it reads the SDK's variables above) |

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
