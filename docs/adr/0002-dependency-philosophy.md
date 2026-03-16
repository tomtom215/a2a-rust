# ADR 0002: Dependency Philosophy and Minimal Footprint

**Date:** 2026-03-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

Rust SDK quality is inversely correlated with transitive dep count. Each dependency:
- Adds compile time.
- Adds supply chain attack surface.
- Constrains downstream version choices.
- Can introduce incompatible license terms.

A production-grade A2A SDK must be usable in embedded or resource-constrained environments, in corporate environments with strict dep audit requirements, and without forcing users into specific TLS or logging frameworks.

## Decision

### Mandatory Runtime Dependencies

Only dependencies with no viable in-tree alternative:

| Dep | Rationale |
|---|---|
| `serde` + `serde_json` | A2A is a JSON-RPC protocol. There is no reasonable alternative to serde in the Rust ecosystem for correct, zero-copy JSON at this level. |
| `tokio` (minimal features) | Async I/O is essential; tokio is the de-facto standard. Features pinned to: `rt, net, io-util, sync, time`. Not `full`. |
| `hyper` 1.x | Lowest-level HTTP library in common Rust use. Brings no cookie jar, no redirect policy, no multipart support. The API is explicit about what it does. |
| `http-body-util` | Required adapter for hyper 1.x body types. No alternative within hyper 1.x. |
| `hyper-util` | Connection pooling for the client; client-legacy for compatibility. |
| `uuid` | Task, Message, Artifact, and Context IDs are UUIDs per spec. Feature: `v4` only (6 source files). |

### Explicitly Excluded (and why)

| Dep | Reason for exclusion |
|---|---|
| `reqwest` | 30+ transitive deps; includes native-tls, cookie jar, redirect, multipart — none needed here. |
| `anyhow` | Erases error types; downstream crates lose the ability to pattern-match on specific errors. We define `A2aError` with typed variants. |
| `thiserror` | Macro sugar that adds a proc-macro dep. `std::fmt::Display` + `std::error::Error` manual impls are 10 lines and fully explicit. |
| `tokio-util` | Use `http-body-util` for hyper body adapters; `tokio-util` codec is not needed. |
| `futures` | We need only `std::future::Future` and `tokio` combinators. The `futures` crate adds 12+ sub-crates. |
| `openssl-sys` | System dep; builds fail on musl/alpine without extra packages. `rustls` is pure Rust with no system dep. |
| `log` | Older logging API; superseded by `tracing`. Made optional via feature flag. |
| `base64` | Used only for `FileWithBytes.bytes` field. Implemented inline (~40 lines) using `std` primitives rather than adding a dep. |

### Feature Flags for Optional Deps

```toml
[features]
default = []
tracing = ["dep:tracing"]
tls-rustls = ["dep:rustls", "dep:tokio-rustls", "dep:webpki-roots"]
```

### Version Bounding

All versions use bounded ranges (no open-ended `>=`):
```toml
serde      = { version = ">=1.0.200, <2", features = ["derive"] }
tokio      = { version = ">=1.38, <2",    features = ["rt", "net", "io-util", "sync", "time"] }
hyper      = { version = ">=1.4, <2",     features = ["client", "server", "http1", "http2"] }
```

Upper bounds prevent silent adoption of breaking changes between SemVer majors that have historically been incompatible within the Rust ecosystem.

## `deny.toml` Enforcement

```toml
[licenses]
allow = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016"]

[bans]
multiple-versions = "warn"
wildcards = "warn"
deny = [
    { name = "openssl-sys" },
    { name = "reqwest" },
]

[sources]
unknown-git = "deny"
unknown-registry = "deny"
```

## Consequences

### Positive

- Compile time for `a2a-protocol-types` is dominated by serde proc-macros only (~8s cold).
- No system library requirements; cross-compilation to `musl` and `wasm32` is possible.
- License audit is simple: everything is MIT/Apache-2.0.
- Downstream crates with strict dep policies can audit a short list.

### Negative

- Base64 encoding must be maintained in-tree (~40 lines).
- TLS requires a feature flag; users must opt in for HTTPS.
- No built-in retry logic (users bring their own or use the interceptor API).

## Alternatives Considered

### Use `reqwest` for HTTP

Rejected. `reqwest` is excellent for application code but wrong for SDK infrastructure:
- Its cookie and redirect handling conflicts with A2A's requirement for precise HTTP control.
- 30+ transitive deps violate our minimal footprint goal.
- Cannot serve HTTP (server-side) — requires a separate dep anyway.

### Use `axum` for Server

Rejected. Framework lock-in. Users who have chosen `actix-web`, `warp`, or raw `hyper` would be forced to add `axum` as a dep. The server is implemented directly on `hyper` and exposes a transport-agnostic `RequestHandler` trait that any framework can wrap.
