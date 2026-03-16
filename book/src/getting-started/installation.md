# Installation

## Requirements

- **Rust 1.93+** (stable)
- A working internet connection for downloading crates

## Adding to Your Project

The easiest way to use a2a-rust is through the umbrella SDK crate, which re-exports everything you need:

```toml
[dependencies]
a2a-protocol-sdk = "0.2"
tokio = { version = "1", features = ["full"] }
```

The SDK crate re-exports `a2a-protocol-types`, `a2a-protocol-client`, and `a2a-protocol-server` so you only need one dependency.

## Individual Crates

If you prefer fine-grained control, depend on individual crates:

```toml
# Types only (no I/O, no async runtime)
a2a-protocol-types = "0.2"

# Client only
a2a-protocol-client = "0.2"

# Server only
a2a-protocol-server = "0.2"
```

This is useful when:
- You're building a **client only** and don't need server types
- You're building a **server only** and don't need client types
- You want to minimize compile times and dependency trees

## Feature Flags

### `a2a-protocol-server`

| Feature | Default | Description |
|---------|---------|-------------|
| `tracing` | Off | Structured logging via the `tracing` crate |

### `a2a-protocol-types`

| Feature | Default | Description |
|---------|---------|-------------|
| `signing` | Off | JWS/ES256 agent card signing (RFC 8785 canonicalization) |

### `a2a-protocol-client`

| Feature | Default | Description |
|---------|---------|-------------|
| `tls-rustls` | Off | HTTPS via rustls (no OpenSSL required) |

Enable features in your `Cargo.toml`:

```toml
[dependencies]
a2a-protocol-server = { version = "0.2", features = ["tracing"] }
a2a-protocol-types = { version = "0.2", features = ["signing"] }
a2a-protocol-client = { version = "0.2", features = ["tls-rustls"] }
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
