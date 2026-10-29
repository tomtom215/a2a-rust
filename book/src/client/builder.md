# Building a Client

The `ClientBuilder` creates an `A2aClient` configured for your target agent. It handles transport selection, timeouts, authentication, and interceptors. This page builds a client; the pages under it cover each group of options.

## Basic client

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
    .with_protocol_binding("HTTP+JSON")
    .build()
    .unwrap();
```

To build from the agent's card instead of a URL, so that the client picks the
binding the card offers and checks the card's capabilities and required
extensions, use `ClientBuilder::from_card(&card)`; the
[builder reference](./builder/reference.md) describes how it chooses.

## Which page do I need

| You want to | Page |
|---|---|
| Change timeouts, retry transient failures, choose output modes, or return before the task finishes | [Timeouts, retries and delivery](./builder/options.md) |
| Send a token or API key, add your own hooks around each call, or ask for extensions | [Interceptors, API keys and extensions](./builder/interceptors.md) |
| Look up a builder method and its default | [Builder reference](./builder/reference.md) |
| Call an agent over gRPC | [gRPC client](./builder/grpc.md) |
| Share one client across requests and tasks | [Reusing and sharing a client](./builder/reuse.md) |

## Next Steps

- **[Sending Messages](./sending-messages.md)** — Synchronous message send
- **[Streaming Responses](./streaming.md)** — Real-time event consumption
- **[Task Management](./task-management.md)** — Querying and canceling tasks
