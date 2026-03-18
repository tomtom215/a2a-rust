# ADR 0004: Transport Abstraction Design

**Date:** 2026-03-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

The A2A v1.0 specification defines transport bindings including:
1. **JSON-RPC 2.0** — HTTP POST to a single endpoint; primary binding.
2. **HTTP+JSON (REST)** — RESTful paths and verbs.
3. **WebSocket** — persistent connections with JSON-RPC frames.
4. **gRPC** — protobuf, HTTP/2 streaming.

For the client, the `AgentCard` declares `supported_interfaces` — a list of `AgentInterface` entries, each with a `protocol_binding` and `url`. A complete client implementation must be able to select the best transport for a given agent.

For the server, operators must be able to expose one or more transports without re-implementing protocol logic.

Both sides share the same protocol logic (`RequestHandler`/`A2aClient`). The protocol logic must not be duplicated across transport implementations.

## Decision

### Client-Side Transport Trait

```rust
pub trait Transport: Send + Sync + 'static {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>>;

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>>;
}
```

`A2aClient` holds a `Box<dyn Transport>`. The client API methods (`send_message`, `get_task`, etc.) are transport-agnostic — they build typed params, call `transport.send_request()`, and deserialize the result.

Four concrete implementations ship in `a2a-protocol-client`:
- `JsonRpcTransport` — HTTP POST to agent's `url` field.
- `RestTransport` — HTTP verbs to REST paths.
- `WebSocketTransport` — JSON-RPC over WebSocket (`websocket` feature).
- `GrpcTransport` — JSON-over-gRPC (`grpc` feature).

### Server-Side Transport Adapters

`RequestHandler` is transport-agnostic: it receives typed params and returns typed results. Because `AgentExecutor` is object-safe (methods return `Pin<Box<dyn Future>>`), `RequestHandler` stores the executor as `Arc<dyn AgentExecutor>` and is **not generic**. Transport adapters wrap it:

```rust
// JSON-RPC adapter
pub struct JsonRpcDispatcher {
    handler: Arc<RequestHandler>,
}
impl JsonRpcDispatcher {
    // Accepts raw hyper Request, returns hyper Response
    pub async fn dispatch(&self, req: Request<Incoming>) -> Response<BoxBody>;
}

// REST adapter
pub struct RestDispatcher {
    handler: Arc<RequestHandler>,
}
impl RestDispatcher {
    pub async fn dispatch(&self, req: Request<Incoming>) -> Response<BoxBody>;
}
```

Users integrate these into their HTTP framework of choice:

```rust
// Direct hyper integration
let json_rpc = Arc::new(JsonRpcDispatcher::new(request_handler));
let service = service_fn(move |req| {
    let h = Arc::clone(&json_rpc);
    async move { h.handle(req).await }
});
```

### Transport Selection Logic (Client)

`ClientBuilder` selects transport based on `AgentCard.supported_interfaces`:
1. If interface has `protocol_binding == "JSONRPC"` → use `JsonRpcTransport`.
2. If `protocol_binding == "REST"` → use `RestTransport`.
3. If `protocol_binding == "GRPC"` → use `GrpcTransport` (requires `grpc` feature).
4. User can override with `with_protocol_binding()`.
5. Default: `JsonRpcTransport` (simplest, most universally supported).

## Consequences

### Positive

- Protocol logic is written once in `RequestHandler` / `A2aClient`; transport adapters are thin.
- Users can implement custom `Transport` impls (WebSocket, unix socket, in-process) without modifying library code.
- Adding gRPC transport requires only a new feature-gated module; no changes to existing code.
- Server adapters work with any HTTP framework (hyper directly, actix, axum, etc.).

### Negative

- `serde_json::Value` as the intermediate format means each request is serialized and deserialized twice (typed → Value → typed). This is a correctness/flexibility tradeoff; benchmarks will quantify the overhead.
- Custom `Transport` implementations must handle SSE framing themselves for streaming.

## Alternatives Considered

### Generic Transport at Client Level: `A2aClient<T: Transport>`

Use generics instead of `Box<dyn Transport>`. Rejected for the initial release because:
- Complicates the public API (users must spell out the transport type parameter).
- Makes `A2aClient` non-object-safe, complicating test doubles.
- Dynamic dispatch overhead is negligible (one vtable lookup per request, amortized over network RTT).

Revisit in v0.2.0 if benchmarks show meaningful cost.

### Separate Client Structs per Transport

`JsonRpcClient`, `RestClient`. Rejected because:
- Code duplication for all 11 methods × 2 (or more) transports.
- Users who switch transports must change their code.
- Contradicts the protocol abstraction goal.
