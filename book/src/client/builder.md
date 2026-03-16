# Building a Client

The `ClientBuilder` creates an `A2aClient` configured for your target agent. It handles transport selection, timeouts, authentication, and interceptors.

## Basic Client

```rust
use a2a_protocol_sdk::client::ClientBuilder;

let client = ClientBuilder::new("http://agent.example.com".to_string())
    .build()
    .expect("build client");
```

The builder auto-selects the transport based on the URL. To explicitly choose:

```rust
// Force JSON-RPC transport
let client = ClientBuilder::new("http://agent.example.com".to_string())
    .with_protocol_binding("JSONRPC")
    .build()
    .unwrap();

// Force REST transport
let client = ClientBuilder::new("http://agent.example.com".to_string())
    .with_protocol_binding("REST")
    .build()
    .unwrap();
```

## Configuration Options

### Timeouts

```rust
use std::time::Duration;

let client = ClientBuilder::new(url)
    .with_timeout(Duration::from_secs(60))              // Per-request timeout (default: 30s)
    .with_connection_timeout(Duration::from_secs(5))     // TCP connect timeout (default: 10s)
    .with_stream_connect_timeout(Duration::from_secs(15)) // SSE connect timeout (default: 30s)
    .build()
    .unwrap();
```

### Output Modes

Specify which MIME types the client can handle:

```rust
let client = ClientBuilder::new(url)
    .with_accepted_output_modes(vec![
        "text/plain".into(),
        "application/json".into(),
        "image/png".into(),
    ])
    .build()
    .unwrap();
```

Default: `["text/plain"]`

### History Length

Control how many historical messages are included in responses:

```rust
let client = ClientBuilder::new(url)
    .with_history_length(10)  // Include last 10 messages
    .build()
    .unwrap();
```

### Interceptors

Add request/response hooks:

```rust
let client = ClientBuilder::new(url)
    .with_interceptor(MyAuthInterceptor::new())
    .with_interceptor(LoggingInterceptor)
    .build()
    .unwrap();
```

### Retry Policy

Enable automatic retries on transient failures (connection errors, timeouts, HTTP 429/502/503/504):

```rust
use a2a_protocol_client::RetryPolicy;

let client = ClientBuilder::new(url)
    .with_retry_policy(RetryPolicy::default())  // 3 retries, 500ms initial backoff
    .build()
    .unwrap();

// Custom retry configuration
let client = ClientBuilder::new(url)
    .with_retry_policy(
        RetryPolicy::default()
            .with_max_retries(5)
            .with_initial_backoff(Duration::from_secs(1))
            .with_max_backoff(Duration::from_secs(60))
            .with_backoff_multiplier(3.0),
    )
    .build()
    .unwrap();
```

### Return Immediately

For push notification workflows, return the task immediately without waiting for completion:

```rust
let client = ClientBuilder::new(url)
    .with_return_immediately(true)
    .build()
    .unwrap();
```

## Builder Reference

| Method | Default | Description |
|--------|---------|-------------|
| `new(url)` | — | Base URL of the agent (required) |
| `with_protocol_binding(str)` | Auto-detect | Force transport: `"JSONRPC"` or `"REST"` |
| `with_timeout(Duration)` | 30s | Per-request timeout |
| `with_connection_timeout(Duration)` | 10s | TCP connection timeout |
| `with_stream_connect_timeout(Duration)` | 30s | SSE stream connect timeout |
| `with_retry_policy(RetryPolicy)` | None | Retry on transient errors |
| `with_accepted_output_modes(Vec<String>)` | `["text/plain"]` | MIME types the client handles |
| `with_history_length(u32)` | None | Messages to include in responses |
| `with_return_immediately(bool)` | false | Don't wait for task completion |
| `with_interceptor(impl CallInterceptor)` | Empty chain | Add request/response hook |

## Thread Safety

`A2aClient` is `Send + Sync` and can be shared across tasks:

```rust
use std::sync::Arc;

let client = Arc::new(
    ClientBuilder::new(url).build().unwrap()
);

// Share across async tasks
let c1 = Arc::clone(&client);
tokio::spawn(async move { c1.send_message(params1).await });

let c2 = Arc::clone(&client);
tokio::spawn(async move { c2.send_message(params2).await });
```

## Next Steps

- **[Sending Messages](./sending-messages.md)** — Synchronous message send
- **[Streaming Responses](./streaming.md)** — Real-time event consumption
- **[Task Management](./task-management.md)** — Querying and canceling tasks
