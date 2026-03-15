<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# a2a-rust

![CI](https://github.com/tomtom215/a2a-rust/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-1.93%2B-orange)

Pure Rust SDK for the [A2A (Agent-to-Agent) protocol](https://google.github.io/A2A/) v0.3.0.

## Status

> **Phase 0 — Project foundation.** Workspace scaffold, CI, and empty crate stubs are in place.
> Protocol types and HTTP transport are implemented in subsequent phases.

## Crate Structure

| Crate | Purpose | Use When |
|---|---|---|
| [`a2a-types`](crates/a2a-types) | All A2A protocol types — serde only, no I/O | You need types without the HTTP stack |
| [`a2a-client`](crates/a2a-client) | HTTP client for sending A2A requests | You are building an orchestrator or test harness |
| [`a2a-server`](crates/a2a-server) | Server framework for implementing agents | You are building an A2A agent |
| [`a2a-sdk`](crates/a2a-sdk) | Convenience re-export of all three | Quick-start / full-stack usage |

## Quick Start

```toml
# Cargo.toml
[dependencies]
a2a-sdk = "0.1"
```

```rust
// Coming in Phase 2 — HTTP client implementation
```

## Design Goals

- **Full spec compliance** with A2A 0.3.0.
- **Zero mandatory I/O deps** in `a2a-types` — only `serde` + `serde_json`.
- **Minimal footprint** — `reqwest`, `axum`, `anyhow`, `thiserror`, `openssl-sys` are all excluded by policy.
- **Modern Rust** — `async fn` in traits (stable since 1.75), Edition 2021, MSRV 1.93.
- **No `unwrap()`** in library code. All errors are typed.

## License

Apache-2.0 — see [LICENSE](LICENSE).
