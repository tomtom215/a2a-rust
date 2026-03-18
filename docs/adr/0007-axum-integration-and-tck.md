# ADR 0007: Axum Framework Integration and TCK Wire Format Tests

**Date:** 2026-03-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

Two gaps were identified during the SDK audit:

1. **No Technology Compatibility Kit (TCK).** The SDK had extensive unit and
   integration tests but lacked formal conformance tests that validate wire
   format interoperability with other A2A v1.0 implementations. ProtoJSON
   naming conventions (e.g. `TASK_STATE_WORKING` vs `"working"`),
   `SecurityRequirement`/`StringList` wrapper format, and discriminated union
   serialization are specific areas where a mismatch would silently break
   cross-SDK communication.

2. **No web framework integration.** The SDK required users to wire raw hyper
   for HTTP serving. Every official A2A SDK integrates with its ecosystem's
   dominant web framework (FastAPI for Python, Express for JS, net/http for Go).
   Rust's dominant async web framework is Axum.

## Decision

### TCK Wire Format Conformance Tests

Add a dedicated test file (`crates/a2a-types/tests/tck_wire_format.rs`) with
golden JSON fixtures representing the canonical A2A v1.0 wire format. Tests
validate:

- ProtoJSON `SCREAMING_SNAKE_CASE` for `TaskState` and `MessageRole`
- `SecurityRequirement` / `StringList` proto wrapper format (rejects OpenAPI-style flat scopes)
- `Part` type discriminator (`{"type": "text", ...}`)
- All 5 `SecurityScheme` variants
- Cross-SDK interop fixtures (Python, JS, Go payload shapes)
- JSON-RPC 2.0 envelope and all 14 error codes
- Full round-trip of complex Task and AgentCard objects

### Axum Framework Integration

Add a feature-gated `axum` module (`crates/a2a-server/src/dispatch/axum_adapter.rs`)
that provides `A2aRouter` — a thin adapter that builds an `axum::Router` wrapping
the existing `RequestHandler`. Design principles:

- **Zero business logic duplication** — delegates entirely to `RequestHandler`
- **Feature-gated** — `axum` feature in `a2a-protocol-server` and `a2a-protocol-sdk`
- **Composable** — the returned `Router` can be merged with other Axum routes
- **Idiomatic** — uses Axum extractors (`State`, `Path`, `Query`, `Bytes`)
- **Catch-all for colon routes** — Axum doesn't support `{id}:cancel` path patterns,
  so `/tasks/{*rest}` is used with manual segment parsing

## Alternatives Considered

### TCK

- **JSON Schema validation**: Rejected — JSON Schema validates structure but not
  semantic correctness (e.g. it can't verify that `TaskState::Working` serializes
  as `"TASK_STATE_WORKING"` rather than `"working"`).
- **a2a-inspector tool**: Not yet available. When it is, the golden fixtures can
  be validated against it.

### Framework Integration

- **Actix-Web**: Considered. Axum was chosen because it shares the tokio ecosystem
  (hyper, tower) that the SDK already uses, reducing dependency surface.
- **Tower service adapter**: Considered. Too low-level for most users — they want
  a `Router`, not a raw `Service`.
- **Framework-agnostic middleware**: The `Dispatcher` trait already serves this role
  for raw hyper. Axum integration is additive, not replacing.

## Consequences

### Positive

- Wire format regressions are caught before release (44 TCK tests)
- Axum users get a 3-line server setup instead of ~25 lines of hyper boilerplate
- The `Router` is composable with Axum's middleware ecosystem (CORS, tracing, auth)
- Zero impact on users who don't enable the `axum` feature

### Negative

- One additional optional dependency (`axum 0.8`) when the feature is enabled
- The catch-all `/tasks/{*rest}` pattern is less declarative than explicit routes
- TCK tests must be manually updated when the A2A spec changes

### Neutral

- The `Dispatcher` trait and raw hyper `serve()` remain the primary API
- Axum integration does not replace JSON-RPC or REST dispatchers — it is an alternative
