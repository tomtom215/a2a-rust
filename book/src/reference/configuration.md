# Configuration Reference

Complete reference of all configuration options across a2a-rust crates.

## Server Configuration

### RequestHandlerBuilder

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `with_agent_card` | `AgentCard` | None | Discovery card for `/.well-known/agent.json` |
| `with_task_store` | `impl TaskStore` | `InMemoryTaskStore` | Custom task storage backend |
| `with_task_store_config` | `TaskStoreConfig` | No limits | TTL and capacity for default store |
| `with_push_config_store` | `impl PushConfigStore` | `InMemoryPushConfigStore` | Custom push config storage |
| `with_push_sender` | `impl PushSender` | None | Webhook delivery implementation |
| `with_interceptor` | `impl ServerInterceptor` | Empty chain | Server middleware |
| `with_executor_timeout` | `Duration` | None | Max time for executor completion |
| `with_event_queue_capacity` | `usize` | 64 | Bounded channel size per stream |
| `with_max_event_size` | `usize` | 16 MiB | Max serialized SSE event size |
| `with_max_concurrent_streams` | `usize` | Unbounded | Limit concurrent SSE connections |
| `with_metrics` | `impl Metrics` | `NoopMetrics` | Metrics observer for handler activity |
| `with_handler_limits` | `HandlerLimits` | See below | Configurable validation limits |

### HandlerLimits

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_id_length` | `usize` | 1,024 | Maximum task/context ID length |
| `max_metadata_size` | `usize` | 1 MiB | Maximum serialized metadata size |
| `max_cancellation_tokens` | `usize` | 10,000 | Cleanup sweep threshold |
| `max_token_age` | `Duration` | 1 hour | Stale token eviction age |

### TaskStoreConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `ttl` | `Option<Duration>` | None | Task expiry time |
| `max_capacity` | `Option<usize>` | None | Maximum number of stored tasks |

### DispatchConfig

Shared configuration for both JSON-RPC and REST dispatchers. Pass to
`JsonRpcDispatcher::with_config()` or `RestDispatcher::with_config()`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_request_body_size` | `usize` | 4 MiB | Larger bodies return 413 |
| `body_read_timeout` | `Duration` | 30s | Slow loris protection |
| `max_query_string_length` | `usize` | 4,096 | REST only; longer queries return 414 |

### PushRetryPolicy

Configurable retry policy for `HttpPushSender`. Pass via
`HttpPushSender::with_retry_policy()`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_attempts` | `usize` | 3 | Maximum delivery attempts |
| `backoff` | `Vec<Duration>` | `[1s, 2s]` | Backoff durations between retries |

### Internal Limits

| Limit | Value | Description |
|-------|-------|-------------|
| Eviction interval | Every 64 writes | Amortized cleanup |
| Write timeout | 5 seconds | Per-event queue write |

## Client Configuration

### ClientBuilder

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `with_protocol_binding` | `&str` | Auto-detect | Transport: `"JSONRPC"` or `"REST"` |
| `with_timeout` | `Duration` | 30s | Per-request timeout |
| `with_connection_timeout` | `Duration` | 10s | TCP connection timeout |
| `with_stream_connect_timeout` | `Duration` | 30s | SSE connect timeout |
| `with_accepted_output_modes` | `Vec<String>` | `["text/plain"]` | MIME types accepted |
| `with_history_length` | `u32` | None | Messages in responses |
| `with_return_immediately` | `bool` | false | Don't wait for completion |
| `with_interceptor` | `impl CallInterceptor` | Empty chain | Client middleware |

### SSE Parser Limits

| Limit | Value | Description |
|-------|-------|-------------|
| Buffer cap | 16 MiB | Max buffered SSE data (aligned with server) |
| Connect timeout | 30s (default) | Initial connection timeout |

## HTTP Caching (Agent Card)

| Header | Default | Description |
|--------|---------|-------------|
| `Cache-Control` | `public, max-age=60` | Configurable max-age |
| `ETag` | Auto-computed | Content hash |
| `Last-Modified` | Auto-set | Timestamp of last change |

## Feature Flags

### `a2a-protocol-server`

| Feature | Default | Description |
|---------|---------|-------------|
| `tracing` | Off | Structured logging via `tracing` crate |

### `a2a-protocol-client`

| Feature | Default | Description |
|---------|---------|-------------|
| `tracing` | Off | Structured logging via `tracing` crate |
| `tls-rustls` | Off | HTTPS via rustls (no OpenSSL dependency) |

### `a2a-protocol-types`

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | JWS/ES256 agent card signing (RFC 8785 canonicalization) |

### `a2a-protocol-sdk` (umbrella)

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | Enables `signing` in all sub-crates |
| `tracing` | Off | Enables `tracing` in client and server |
| `tls-rustls` | Off | Enables `tls-rustls` in client |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level filter (when `tracing` feature is enabled) |

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
