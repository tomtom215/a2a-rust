# Architecture Decision Records

Key design decisions for a2a-rust, documented as ADRs. Each record captures the context, decision, and rationale.

## ADR 0001: Workspace Crate Structure

**Status:** Accepted

**Context:** The A2A protocol has three distinct concerns with different dependency trees: types (pure data, no I/O), client (HTTP sending), and server (HTTP receiving). A single-crate approach forces every user to compile the full dependency tree regardless of need.

**Decision:** Four crates in a Cargo workspace:

| Crate | Purpose | Key Dependencies |
|-------|---------|-----------------|
| `a2a-protocol-types` | Wire types only | `serde`, `serde_json` |
| `a2a-protocol-client` | HTTP client | `hyper`, `tokio` |
| `a2a-protocol-server` | Server framework | `hyper`, `tokio` |
| `a2a-protocol-sdk` | Umbrella re-exports | All above |

**Rationale:** An agent server implementor doesn't pay for the client dependency tree and vice versa. The types crate is usable without any async runtime — useful for codegen, validation, or non-HTTP transports.

## ADR 0002: Dependency Philosophy

**Status:** Accepted

**Context:** Rust SDK quality is inversely correlated with transitive dependency count. Each dependency adds compile time, supply chain attack surface, version conflicts, and potential license issues.

**Decision:** Minimal dependency footprint:
- No web frameworks required (optional Axum integration via `axum` feature)
- No TLS bundled (bring your own or use a proxy)
- No logging framework forced (optional `tracing` feature)
- In-tree SSE parser instead of third-party crate
- `serde` + `hyper` as the only mandatory heavyweight deps

**Rationale:** A production-grade SDK must work in corporate environments with strict dependency audits, embedded/constrained environments, and without forcing users into specific TLS or logging frameworks.

## ADR 0003: Async Runtime Strategy

**Status:** Accepted

**Context:** Rust async code is runtime-agnostic at the language level, but `hyper` 1.x uses tokio internally. Making the SDK runtime-agnostic would require wrapping every I/O call behind an abstraction layer.

**Decision:** Tokio is the mandatory async runtime. It is a required dependency (not optional, not feature-gated) for the I/O crates. `a2a-protocol-types` has no async runtime dependency.

**Rationale:** >95% of Rust async production code runs on tokio. The complexity cost of runtime abstraction is not justified by the ~5% of users on other runtimes.

## ADR 0004: Transport Abstraction Design

**Status:** Accepted

**Context:** A2A defines three transport bindings (JSON-RPC, REST, gRPC). Both client and server share protocol logic that must not be duplicated across transport implementations.

**Decision:** Three-layer architecture:

```
Dispatcher (transport) → RequestHandler (protocol) → AgentExecutor (user logic)
```

- **Dispatchers** handle HTTP-level concerns (routing, content types, CORS)
- **RequestHandler** contains all protocol logic (task lifecycle, stores, streaming)
- **AgentExecutor** is the user's entry point

The `Transport` trait (client) and `Dispatcher` trait (server) make transports pluggable. Both share the same handler/client core.

**Rationale:** Adding gRPC support later requires only a new dispatcher/transport — zero changes to protocol logic or user code.

## ADR 0005: SSE Streaming Design

**Status:** Accepted

**Context:** A2A streaming uses Server-Sent Events (SSE). Rust lacks a battle-tested, zero-dep SSE library that is hyper 1.x native.

**Decision:** In-tree SSE implementation for both client parsing and server emission:
- Client parser: ~220 lines, handles partial TCP frames, enforces 16 MiB buffer cap
- Server emitter: ~200 lines, formats SSE `data:` lines with proper `\n\n` terminators
- Zero additional dependencies beyond hyper

**Rationale:** Third-party SSE crates either use older hyper versions, carry unnecessary dependencies, or lack proper backpressure. The in-tree implementation is small enough to audit, test, and maintain.

## ADR 0006: Mutation Testing as a Required Quality Gate

**Status:** Accepted

**Context:** The test suite includes unit, integration, property, fuzz, and E2E dogfood tests — but none of these measure whether the tests actually *detect* real bugs. A test suite can achieve 100% line coverage with trivial assertions. At multi-data-center deployment scales, the bugs that escape traditional testing have the highest blast radius.

The full sweep runs via `workflow_dispatch`; every pull request runs an
incremental `--in-diff` mutation gate on the changed lines.

**Rationale:** Mutation testing is the only technique that directly measures *fault detection capability*. It provides an objective, automated answer to "would this test suite catch a real bug at this location?" The tradeoff is CI time: making it a blocking PR gate is punitive given current compute budgets, so it is treated as an on-demand audit that *enforces* zero survivors when it runs, rather than a per-commit gate.

## ADR 0007: Axum Integration and TCK Wire Format Tests

**Status:** Accepted

**Context:** The SDK lacked formal wire format conformance tests and required raw hyper for HTTP serving. Other SDKs integrate with their ecosystem's dominant web framework.

**Decision:** Two additions:
1. **TCK conformance tests** — 44 golden-fixture tests in `crates/a2a-protocol-types/tests/tck_wire_format.rs` validating ProtoJSON serialization, SecurityRequirement/StringList format, Part discriminators, cross-SDK interop fixtures, and full round-trip.
2. **Axum integration** — Feature-gated `A2aRouter` (`axum` feature) wrapping `RequestHandler` as an idiomatic `axum::Router`. Zero business logic duplication.

**Rationale:** TCK tests catch interop regressions before release. Axum integration reduces server setup from ~25 lines to 3 while remaining optional (the raw hyper `serve()` API is unchanged).

## ADR 0008: Object-Safe `AgentExecutor` Trait Shape

**Status:** Accepted

**Context:** `AgentExecutor` is the primary extension point users implement. The obvious question a reader asks when opening `executor.rs` is: *why not `async fn execute(...)`?* — every method returns `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>`, and every example in the repo wraps bodies with `Box::pin(async move { ... })` or uses the `boxed_future` / `agent_executor!` helpers.

**Decision:** Keep `AgentExecutor` as a manual `Pin<Box<dyn Future + Send + 'a>>` trait (object-safe), and ship two ergonomic helpers (`boxed_future` and the `agent_executor!` macro) to compensate for the call-site noise. `RequestHandler` stores the executor as `Arc<dyn AgentExecutor>`, which keeps `RequestHandler`, every dispatcher, the Axum integration, and the builder API **non-generic**.

**Rationale:** Async-fn-in-trait is stable since Rust 1.75 but produces per-impl anonymous future types that are **not object-safe on stable**. Using `async fn execute` would force `RequestHandler<E>` to become generic, and that generic parameter would then leak through every dispatcher, the Axum layer, `A2aRouter`, and the builder API. The `async-trait` crate produces exactly the same `Pin<Box<dyn Future>>` shape we have now but hides the heap allocation and forces a proc-macro dep on every downstream user — a non-starter given ADR 0002. The manual shape is honest about the cost (one `Box::pin` allocation per call, deep in the noise relative to network RTT) and keeps the public API free of trait-erasure machinery. The `agent_executor!` macro gives trivial executors the same ergonomics as `async fn` without introducing a second trait. See [ADR 0008 full document](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0008-agent-executor-trait-shape.md) for the full alternatives analysis and the revisit trigger.

## Summary

| ADR | Key Decision |
|-----|-------------|
| 0001 | Four-crate workspace (types, client, server, sdk) |
| 0002 | Minimal mandatory dependencies, optional framework integration |
| 0003 | Tokio as mandatory runtime |
| 0004 | Three-layer architecture (dispatcher → handler → executor) |
| 0005 | In-tree SSE parser/emitter, zero additional deps |
| 0006 | `cargo-mutants` as on-demand test-effectiveness audit |
| 0007 | Axum integration + TCK wire format conformance tests |
| 0008 | `AgentExecutor` kept object-safe so `RequestHandler` stays non-generic |
| 0009 | Protobuf-native gRPC on the canonical `lf.a2a.v1.A2AService`, wire-compatible with the official SDKs; the pre-0.7 JSON tunnel is deprecated behind `grpc-legacy-json` |
| 0010 | First-party auth helpers (client OAuth2 client-credentials + token providers; server API-key/bearer/JWT interceptors) built on `ring`/`hyper`, not the OAuth-ecosystem crates |

The full ADR documents are in the [`docs/adr/`](https://github.com/tomtom215/a2a-rust/tree/main/docs/adr) directory.

## Next Steps

- **[Configuration Reference](./configuration.md)** — All tunable parameters
- **[Pitfalls & Lessons Learned](./pitfalls.md)** — Practical issues and solutions
