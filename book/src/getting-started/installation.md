# Installation

## Requirements

- **Rust 1.93+** (stable; also tested on 1.94)
- A working internet connection for downloading crates
- **`protoc`** (Protocol Buffers compiler) — required only when enabling the `grpc` feature. Install via `apt-get install protobuf-compiler` (Debian/Ubuntu), `brew install protobuf` (macOS), or download from the [protobuf releases page](https://github.com/protocolbuffers/protobuf/releases). Not needed for default features.

## Adding to Your Project

The easiest way to use a2a-rust is through the umbrella SDK crate, which re-exports everything you need:

```toml
[dependencies]
a2a-protocol-sdk = "0.3"
tokio = { version = "1", features = ["full"] }
```

The SDK crate re-exports `a2a-protocol-types`, `a2a-protocol-client`, and `a2a-protocol-server` so you only need one dependency.

## Individual Crates

If you prefer fine-grained control, depend on individual crates:

```toml
# Types only (no I/O, no async runtime)
a2a-protocol-types = "0.3"

# Client only
a2a-protocol-client = "0.3"

# Server only
a2a-protocol-server = "0.3"
```

This is useful when:
- You're building a **client only** and don't need server types
- You're building a **server only** and don't need client types
- You want to minimize compile times and dependency trees

## Feature Flags

All features are off by default to minimize compile times and dependency trees.

### `a2a-protocol-types`

| Feature | Description |
|---------|-------------|
| `signing` | JWS/ES256 agent card signing (RFC 8785 canonicalization) |

### `a2a-protocol-client`

| Feature | Description |
|---------|-------------|
| `tls-rustls` | HTTPS via rustls (no OpenSSL required) |
| `signing` | Agent card signing verification |
| `tracing` | Structured logging via the `tracing` crate |
| `websocket` | WebSocket transport via `tokio-tungstenite` |
| `grpc` | gRPC transport via `tonic` |

### `a2a-protocol-server`

| Feature | Description |
|---------|-------------|
| `signing` | Agent card signing |
| `tracing` | Structured logging via the `tracing` crate |
| `sqlite` | SQLite-backed task and push config stores via `sqlx` |
| `postgres` | PostgreSQL-backed task and push config stores via `sqlx` |
| `websocket` | WebSocket transport via `tokio-tungstenite` |
| `grpc` | gRPC transport via `tonic` |
| `otel` | OpenTelemetry metrics via `opentelemetry-otlp` |
| `axum` | Axum framework integration (`A2aRouter`) |

### `a2a-protocol-sdk` (umbrella)

| Feature | Description |
|---------|-------------|
| `signing` | Enables signing across types, client, and server |
| `tracing` | Enables tracing across client and server |
| `tls-rustls` | Enables HTTPS in the client |
| `grpc` | Enables gRPC across client and server |
| `websocket` | Enables WebSocket across client and server |
| `sqlite` | Enables SQLite stores in the server |
| `postgres` | Enables PostgreSQL stores in the server |
| `otel` | Enables OpenTelemetry metrics in the server |
| `axum` | Enables Axum integration in the server |

Enable features in your `Cargo.toml`:

```toml
[dependencies]
a2a-protocol-sdk = { version = "0.4", features = ["tracing", "signing"] }

# Or with individual crates:
a2a-protocol-server = { version = "0.4", features = ["tracing", "sqlite"] }
a2a-protocol-client = { version = "0.4", features = ["tls-rustls"] }
```

## Verifying the Installation

Create a simple `main.rs` to verify everything compiles:

```rust
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
