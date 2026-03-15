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

### TaskStoreConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `ttl` | `Option<Duration>` | None | Task expiry time |
| `max_capacity` | `Option<usize>` | None | Maximum number of stored tasks |

### Internal Limits

| Limit | Value | Description |
|-------|-------|-------------|
| Max task/context ID length | 1,024 chars | Prevents oversized IDs |
| Max metadata size | 1 MiB | Prevents oversized metadata |
| Max cancellation tokens | 10,000 | Hard cap, cleaned on overflow |
| Max token age | 1 hour | Stale tokens evicted |
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
| Buffer cap | 16 MiB | Max buffered SSE data |
| Connect timeout | 30s (default) | Initial connection timeout |

## REST Dispatcher Limits

| Limit | Value | Description |
|-------|-------|-------------|
| Max request body | 4 MiB | Larger bodies return 413 |
| Max query string | 4 KiB | Longer queries return 414 |
| Content types | `application/json`, `application/a2a+json` | Accepted content types |

## HTTP Caching (Agent Card)

| Header | Default | Description |
|--------|---------|-------------|
| `Cache-Control` | `public, max-age=60` | Configurable max-age |
| `ETag` | Auto-computed | Content hash |
| `Last-Modified` | Auto-set | Timestamp of last change |

## Feature Flags

### `a2a-server`

| Feature | Default | Description |
|---------|---------|-------------|
| `tracing` | Off | Structured logging via `tracing` crate |

### `a2a-types`

| Feature | Default | Description |
|---------|---------|-------------|
| `card-signing` | Off | Ed25519 agent card signatures |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level filter (when `tracing` feature is enabled) |

Examples:
```bash
RUST_LOG=info              # Info and above
RUST_LOG=debug             # Debug and above
RUST_LOG=a2a_server=debug  # Debug for server crate only
RUST_LOG=a2a_server=trace,a2a_client=debug  # Per-crate levels
```

## Next Steps

- **[API Quick Reference](./api-reference.md)** — All public types at a glance
- **[Pitfalls & Lessons Learned](./pitfalls.md)** — Known issues and workarounds
