# Building a Client

The `ClientBuilder` creates an `A2aClient` configured for your target agent. It handles transport selection, timeouts, authentication, and interceptors.

## Basic Client

```rust,no_run
use a2a_protocol_sdk::client::ClientBuilder;

let client = ClientBuilder::new("http://agent.example.com")
    .build()
    .expect("build client");
```

Without a binding the builder uses JSON-RPC; it does not look at the URL to choose. To choose another:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
// Force JSON-RPC transport
let client = ClientBuilder::new("http://agent.example.com")
    .with_protocol_binding("JSONRPC")
    .build()
    .unwrap();

// Force REST transport
let client = ClientBuilder::new("http://agent.example.com")
    .with_protocol_binding("REST")
    .build()
    .unwrap();
```

## Configuration Options

### Timeouts

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
use std::time::Duration;

let client = ClientBuilder::new(url)
    .with_timeout(Duration::from_secs(60))              // Per-request timeout (default: 30s)
    .with_connection_timeout(Duration::from_secs(5))     // TCP connect timeout (default: 10s)
    .with_stream_connect_timeout(Duration::from_secs(15)) // stream headers (default: 30s)
    .with_stream_first_event_timeout(Duration::from_secs(60)) // first event after that (default: 5 min)
    .with_stream_idle_timeout(Some(Duration::from_secs(120))) // silence allowed mid-stream (default: 5 min)
    .build()
    .unwrap();
```

### Output Modes

Specify which MIME types the client can handle:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_accepted_output_modes(vec![
        "text/plain".into(),
        "application/json".into(),
        "image/png".into(),
    ])
    .build()
    .unwrap();
```

Default: `["text/plain", "application/json"]`

### History Length

Control how many historical messages are included in responses:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_history_length(10)  // Include last 10 messages
    .build()
    .unwrap();
```

### Interceptors

Add request/response hooks:

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
use std::sync::Arc;
use a2a_protocol_client::{
    BearerAuthInterceptor, StaticTokenProvider, TracePropagationInterceptor,
};

let token = Arc::new(StaticTokenProvider::new("my-token"));
let client = ClientBuilder::new(url)
    .with_interceptor(BearerAuthInterceptor::new(token))
    .with_interceptor(TracePropagationInterceptor)
    .build()
    .unwrap();
```

Your own hooks implement `CallInterceptor` and are added the same way.

### Retry Policy

Enable automatic retries on transient failures (connection errors, timeouts, HTTP 429/502/503/504):

```rust
# use a2a_protocol_sdk::client::ClientBuilder;
# use std::time::Duration;
# let url = "http://agent.example.com";
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
# use a2a_protocol_sdk::client::ClientBuilder;
# let url = "http://agent.example.com";
let client = ClientBuilder::new(url)
    .with_return_immediately(true)
    .build()
    .unwrap();
```

## Builder Reference

| Method | Default | Description |
|--------|---------|-------------|
| `new(url)` | — | Base URL of the agent (required) |
| `from_card(&AgentCard)` | — | Build from an agent card, preferring `ClientConfig`'s default binding order (`["JSONRPC"]`), falling back to the card's first compatible interface. Only interfaces whose `protocolVersion` has major 1 count (empty counts; `v1.0` counts), so a v0.3 endpoint listed first is skipped; a card with none is refused with what it offers. If the chosen interface cannot be built (gRPC under sync `build()`, an unknown binding, a bad URL), `build()` moves to the next one |
| `from_card_preferring(&AgentCard, &[String])` | — | Same, with your own binding order. The first preference the card offers wins; matching is case-insensitive |
| `with_protocol_binding(str)` | `"JSONRPC"` | Force transport: `"JSONRPC"`, `"HTTP+JSON"` (or `"REST"`), or `"GRPC"`, in any case. On a builder made from a card, moves the endpoint and tenant to that binding's interface too, and turns off `build()`'s fallback to other interfaces |
| `with_custom_transport(impl Transport)` | None | Use a custom transport (e.g., `GrpcTransport`) |
| `with_timeout(Duration)` | 30s | Per-request timeout |
| `with_connection_timeout(Duration)` | 10s | TCP connection timeout |
| `with_stream_connect_timeout(Duration)` | 30s | Establishing a stream (headers, or an error body) |
| `with_stream_first_event_timeout(Duration)` | 5 min | Wait for a stream's first data once established |
| `with_max_event_size(usize)` | 16 MiB | Largest single stream event accepted; larger ones are refused and skipped |
| `with_stream_idle_timeout(Option<Duration>)` | 5 min | Silence allowed between chunks after a stream's first data; SSE keep-alives reset it; `None` disables |
| `with_retry_policy(RetryPolicy)` | None | Retry on transient errors with jittered exponential backoff |
| `with_accepted_output_modes(Vec<String>)` | `["text/plain", "application/json"]` | MIME types the client handles |
| `with_history_length(u32)` | None | Messages to include in responses |
| `with_return_immediately(bool)` | false | Don't wait for task completion |
| `with_tenant(str)` | None (auto from `AgentCard`) | Default tenant for multi-tenancy |
| `with_interceptor(impl CallInterceptor)` | Empty chain | Add request/response hook |

## Client Reuse (Best Practice)

**Create clients once and reuse them across requests.** Each `A2aClient` holds
a connection pool internally (via hyper), so reuse avoids repeated DNS
resolution, TCP handshakes, and TLS negotiation on every call.

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
// ✅ Good: build once, reuse across requests
struct MyOrchestrator {
    analyzer: A2aClient,
    builder: A2aClient,
}

impl MyOrchestrator {
    fn new(analyzer_url: &str, builder_url: &str) -> Self {
        Self {
            analyzer: ClientBuilder::new(analyzer_url).build().unwrap(),
            builder: ClientBuilder::new(builder_url)
                .with_protocol_binding("REST")
                .build()
                .unwrap(),
        }
    }

    async fn run(&self, params: MessageSendParams) {
        // Reuse the same client for every request
        let _ = self.analyzer.send_message(params.clone()).await;
        let _ = self.builder.send_message(params).await;
    }
}
```

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
// ❌ Avoid: rebuilding the client on every call
async fn bad_pattern(url: &str, params: MessageSendParams) {
    // This works but wastes resources — connection pool is discarded each time
    let client = ClientBuilder::new(url).build().unwrap();
    let _ = client.send_message(params).await;
}
```

## gRPC Client

> Requires the `grpc` feature: `a2a-protocol-client = { version = "0.14", features = ["grpc"] }`

For gRPC transport, use `GrpcTransport::connect()` with `with_custom_transport()`:

```rust,no_run
# use a2a_protocol_sdk::client::ClientBuilder;
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_client::GrpcTransport;

let transport = GrpcTransport::connect("http://agent.example.com:50051").await?;
let client = ClientBuilder::new("http://agent.example.com:50051")
    .with_custom_transport(transport)
    .build()?;
# Ok(())
# }
```

Configure with `GrpcTransportConfig`:

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_client::transport::grpc::{GrpcTransport, GrpcTransportConfig};
use std::time::Duration;

let config = GrpcTransportConfig::default()
    .with_timeout(Duration::from_secs(60))
    .with_max_message_size(8 * 1024 * 1024);

let transport = GrpcTransport::connect_with_config(
    "http://agent.example.com:50051",
    config,
).await?;
# Ok(())
# }
```

## Thread Safety

`A2aClient` is `Send + Sync` and can be shared across tasks via `Arc`:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# #[tokio::main]
# async fn main() {
# let url = "http://agent.example.com";
# let params = MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
# let (params1, params2) = (params.clone(), params);
use std::sync::Arc;

let client = Arc::new(
    ClientBuilder::new(url).build().unwrap()
);

// Share across async tasks
let c1 = Arc::clone(&client);
tokio::spawn(async move { c1.send_message(params1).await });

let c2 = Arc::clone(&client);
tokio::spawn(async move { c2.send_message(params2).await });
# }
```

## Next Steps

- **[Sending Messages](./sending-messages.md)** — Synchronous message send
- **[Streaming Responses](./streaming.md)** — Real-time event consumption
- **[Task Management](./task-management.md)** — Querying and canceling tasks
