# Installation

## Requirements

- **Rust 1.88+** (stable; CI tests the MSRV and current stable)
- A working internet connection for downloading crates
- **`protoc`** (Protocol Buffers compiler) — bundled automatically: the `grpc`/`proto` build scripts use a vendored `protoc` (`protoc-bin-vendored`), so a clean `cargo build --features grpc` works with no system install. Set the `PROTOC` environment variable only to override with your own binary (or on a platform the vendored binaries don't cover).

## Adding to Your Project

The easiest way to use a2a-rust is through the umbrella SDK crate, which re-exports everything you need:

```toml
[dependencies]
a2a-protocol-sdk = "0.14"
tokio = { version = "1", features = ["full"] }
```

The SDK crate re-exports `a2a-protocol-types`, `a2a-protocol-client`, and `a2a-protocol-server` so you only need one dependency.

## Individual Crates

If you prefer fine-grained control, depend on individual crates:

```toml
# Types only (no I/O, no async runtime)
a2a-protocol-types = "0.14"

# Client only
a2a-protocol-client = "0.14"

# Server only
a2a-protocol-server = "0.14"
```

This is useful when:
- You're building a **client only** and don't need server types
- You're building a **server only** and don't need client types
- You want to minimize compile times and dependency trees

## Feature Flags

Features are off by default to minimize compile times and dependency trees, with
two exceptions. **`tls-rustls` is on by default** for `a2a-protocol-client` and
`a2a-protocol-sdk`, because the A2A spec serves agents over HTTPS and the client
(and the bundled push sender) must reach them out of the box. **`tracing` is on
by default** for all three, so a default build logs through whatever `tracing`
subscriber the application installs.
`default-features = false` removes a crate's defaults — on the SDK too, which
takes the client and server without theirs: an SDK built that way has no rustls
and no logging.

### `a2a-protocol-types`

| Feature | Description |
|---------|-------------|
| `signing` | JWS/ES256 agent card signing (RFC 8785 canonicalization) |
| `proto` | Canonical protobuf message types and the JSON⇄proto conversions (turned on by `grpc`) |

### `a2a-protocol-client`

| Feature | Description |
|---------|-------------|
| `tls-rustls` | HTTPS via rustls (no OpenSSL required) |
| `signing` | Agent card signing verification |
| `tracing` | Structured logging via the `tracing` crate |
| `websocket` | WebSocket transport via `tokio-tungstenite` |
| `grpc` | gRPC transport via `tonic` (plaintext) |
| `grpc-tls` | gRPC over TLS — `grpc` + tonic's rustls connector (independent of `tls-rustls`); needed for `https://` gRPC endpoints and for the default dialling of a bare non-loopback `host:port` target |
| `testing` | A scripted hostile peer (`testing::ScriptedPeer`) that stalls, cuts off, mis-frames or refuses on each binding, for testing code that calls agents |

### `a2a-protocol-server`

| Feature | Description |
|---------|-------------|
| `signing` | Forwards `a2a-protocol-types/signing`; the server itself neither signs nor verifies the card it serves |
| `tracing` | Structured logging via the `tracing` crate |
| `tls-rustls` | HTTPS delivery for the bundled push-notification sender |
| `sqlite` | SQLite-backed task and push config stores via `sqlx` |
| `postgres` | PostgreSQL-backed task and push config stores via `sqlx` |
| `websocket` | WebSocket transport via `tokio-tungstenite` |
| `grpc` | gRPC transport via `tonic` |
| `grpc-tls` | TLS on the gRPC listener (`GrpcDispatcher::with_tls`); implies `grpc` |
| `otel` | OpenTelemetry metrics via `opentelemetry-otlp` |
| `conformance` | A harness that grades an `AgentExecutor` against the protocol's invariants |
| `axum` | Axum framework integration (`A2aRouter`) |
| `auth-jwt` | JWT bearer-token authentication (`JwtAuthInterceptor`) |

### `a2a-protocol-sdk` (umbrella)

| Feature | Description |
|---------|-------------|
| `signing` | Enables signing across types, client, and server |
| `tracing` | Enables tracing across client and server |
| `tls-rustls` | Enables HTTPS in the client and TLS push delivery in the server |
| `grpc` | Enables gRPC across client and server |
| `grpc-tls` | Enables gRPC over TLS in the client and on the server's gRPC listener |
| `websocket` | Enables WebSocket across client and server |
| `sqlite` | Enables SQLite stores in the server |
| `postgres` | Enables PostgreSQL stores in the server |
| `otel` | Enables OpenTelemetry metrics in the server |
| `axum` | Enables Axum integration in the server |
| `auth-jwt` | Enables JWT bearer-token authentication in the server |

Enable features in your `Cargo.toml`:

```toml
[dependencies]
a2a-protocol-sdk = { version = "0.14", features = ["tracing", "signing"] }

# Or with individual crates:
a2a-protocol-server = { version = "0.14", features = ["tracing", "sqlite"] }
a2a-protocol-client = { version = "0.14", features = ["tls-rustls"] }
```

## Verifying the Installation

Create a simple `main.rs` to verify everything compiles:

```rust,no_run
use a2a_protocol_sdk::prelude::*;

fn main() {
    // Create a task status
    let status = TaskStatus::new(TaskState::Submitted);
    println!("Task state: {:?}", status.state);

    // Create a message with a text part
    let part = Part::text("Hello, A2A!");
    println!("Part: {:?}", part);

    // Verify agent capabilities builder
    let caps = AgentCapabilities::none()
        .with_streaming(true)
        .with_push_notifications(false);
    println!("Capabilities: {:?}", caps);
}
```

Run it:

```bash
cargo run
```

If this compiles and runs, you're ready to go.

## Next Steps

- **[Quick Start](./quick-start.md)** — Run the echo agent example in 5 minutes
- **[Your First Agent](./first-agent.md)** — Build an agent from scratch
