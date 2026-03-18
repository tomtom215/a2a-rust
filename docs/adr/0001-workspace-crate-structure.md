# ADR 0001: Workspace Crate Structure

**Date:** 2026-03-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

The A2A protocol has three distinct concerns with different dep trees:

1. **Types** — pure data structures for the protocol; no I/O.
2. **Client** — sending A2A requests over HTTP; needs async + HTTP stack.
3. **Server** — receiving and dispatching A2A requests; needs async + HTTP stack.

A single-crate approach forces every user to compile the full dep tree regardless of need. An agent server implementor must not pay for the client dep tree and vice versa.

A2A also has a known future extension (gRPC transport, Phase 8+) that requires `prost` and `tonic` — heavyweight codegen deps that no pure-HTTP user should be forced to include.

## Decision

The workspace is divided into four crates:

```
a2a-protocol-types   (serde only)
a2a-protocol-client  (a2a-protocol-types + hyper + tokio)
a2a-protocol-server  (a2a-protocol-types + hyper + tokio)
a2a-protocol-sdk     (re-exports all three)
```

`a2a-protocol-client` and `a2a-protocol-server` are siblings; neither depends on the other. This mirrors the Go SDK's package separation between `a2aclient` and `a2asrv`.

## Consequences

### Positive

- An agent implementor adds `a2a-protocol-server` only; they do not compile client code.
- An orchestrator adds `a2a-protocol-client` only; they do not compile server code.
- Type-only users (downstream SDKs, protocol validators) add `a2a-protocol-types` only.
- Optional transports (gRPC, WebSocket, Axum) are feature-gated within existing crates rather than separate crates.
- `a2a-protocol-sdk` gives quick-start users a single dep.

### Negative

- Four `Cargo.toml` files to maintain.
- Cross-crate refactors require coordinated version bumps.
- The umbrella crate `a2a-protocol-sdk` must be re-published when any constituent crate changes.

## Alternatives Considered

### Single Crate with Feature Flags

`a2a-rs` with `features = ["client", "server", "grpc"]`. Rejected because:
- Feature unification in workspaces causes dep pollution across crates.
- Compile-time savings are smaller than crate-level splits in practice.
- API surface becomes harder to document cleanly.

### Two Crates (types + sdk)

Merge client and server into one `a2a-protocol-sdk`. Rejected because:
- Forces agent implementors to compile client code they will never use.
- Forces orchestrators to compile server code they will never use.
- Asymmetric dep trees (server needs `uuid` for ID gen; client does not in the same way).
