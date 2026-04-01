<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Crates

The a2a-rust SDK is split into four crates with clear dependency boundaries.
Use the umbrella crate (`a2a-protocol-sdk`) for full applications, or depend
on individual crates when you only need a subset.

## Crate Map

```
a2a-protocol-sdk            ← umbrella re-export + prelude
  ├── a2a-protocol-server    ← server framework (agents)
  │     └── a2a-protocol-types
  ├── a2a-protocol-client    ← HTTP client (orchestrators)
  │     └── a2a-protocol-types
  └── a2a-protocol-types     ← wire types (serde only, no I/O)
```

## Overview

| Crate | Published Name | Purpose | When to Use |
|-------|---------------|---------|-------------|
| [a2a-types](a2a-types/) | `a2a-protocol-types` | Pure A2A v1.0 data types -- serde only, no I/O. Includes `serde_helpers` module with `SerBuffer` (thread-local buffer reuse) and `deser_from_str`/`deser_from_slice` (borrowed deserialization) for optimized serialization paths. | You need typed protocol definitions without HTTP dependencies |
| [a2a-client](a2a-client/) | `a2a-protocol-client` | Async HTTP client (hyper 1.x) with pluggable transports | Building orchestrators, gateways, or test harnesses |
| [a2a-server](a2a-server/) | `a2a-protocol-server` | Server framework with dispatchers, stores, and streaming | Building A2A-compliant agents |
| [a2a-sdk](a2a-sdk/) | `a2a-protocol-sdk` | Umbrella re-export + `prelude` module | Full applications that need client + server + types |

## Quick Start

**Full SDK (most users):**

```toml
[dependencies]
a2a-protocol-sdk = "0.4"
```

```rust
use a2a_protocol_sdk::prelude::*;
```

**Types only (custom integrations):**

```toml
[dependencies]
a2a-protocol-types = "0.4"
```

**Client only (orchestrators):**

```toml
[dependencies]
a2a-protocol-client = "0.4"
```

**Server only (agents):**

```toml
[dependencies]
a2a-protocol-server = "0.4"
```

## Feature Flags

Features are defined on individual crates and passed through by the SDK umbrella:

| Feature | Crate(s) | Purpose |
|---------|----------|---------|
| `signing` | types, client, server | Agent card signing (JWS/ES256) |
| `tls-rustls` | client | HTTPS via rustls (default, no OpenSSL) |
| `tracing` | client, server | Structured logging (zero-cost when off) |
| `sqlite` | server | SQLite-backed task and push config stores |
| `postgres` | server | PostgreSQL-backed stores |
| `websocket` | client, server | WebSocket transport |
| `grpc` | client, server | gRPC transport via tonic |
| `otel` | server | OpenTelemetry OTLP metrics export |
| `axum` | server | Axum framework integration |

## Design Decisions

See [ADR-0001: Workspace Crate Structure](../docs/adr/0001-workspace-crate-structure.md)
for the rationale behind splitting into four crates instead of a monolith.

## License

Apache-2.0
