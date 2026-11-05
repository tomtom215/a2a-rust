# Architecture Decision Records

The design decisions behind a2a-rust, each recorded as an ADR: the context, the
decision, and why. Read the one behind a part of the SDK before changing how
that part works, or to learn why it is not done the obvious way.

## All records

The full documents are in
[`docs/adr/`](https://github.com/tomtom215/a2a-rust/tree/main/docs/adr); each
number links to its file.

| ADR | Decision | Status |
|-----|----------|--------|
| [0001](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0001-workspace-crate-structure.md) | Four-crate workspace (types, client, server, sdk) | Accepted |
| [0002](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0002-dependency-philosophy.md) | Minimal mandatory dependencies, optional framework integration | Accepted |
| [0003](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0003-async-runtime-strategy.md) | Tokio as mandatory runtime | Accepted |
| [0004](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0004-transport-abstraction.md) | Three-layer architecture (dispatcher → handler → executor) | Accepted; superseded in part by 0009 |
| [0005](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0005-sse-streaming-design.md) | In-tree SSE parser/emitter, zero additional deps | Accepted |
| [0006](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0006-mutation-testing.md) | `cargo-mutants` as a test-effectiveness gate: weekly full sweep, per-PR diff gate | Accepted |
| [0007](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0007-axum-integration-and-tck.md) | Axum integration + TCK wire format conformance tests | Accepted |
| [0008](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0008-agent-executor-trait-shape.md) | `AgentExecutor` kept object-safe so `RequestHandler` stays non-generic | Accepted |
| [0009](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0009-protobuf-native-grpc.md) | Protobuf-native gRPC on the canonical `lf.a2a.v1.A2AService`, wire-compatible with the official SDKs; the pre-0.7 JSON tunnel was deprecated behind `grpc-legacy-json` in 0.7 and removed in 0.8 | Accepted |
| [0010](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0010-auth-integration.md) | First-party auth helpers (client OAuth2 client-credentials + token providers; server API-key/bearer/JWT interceptors) built on `ring`/`hyper`, not the OAuth-ecosystem crates; its "rejections map to `InvalidRequest`" consequence is superseded by ADR 0014 | Accepted; one consequence superseded by 0014 |
| [0011](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0011-task-retention.md) | Task retention is explicit, and the store does not schedule it | Accepted |
| [0012](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0012-event-log-and-resumption.md) | An append-only event log, with the snapshot kept as the record | Accepted |
| [0013](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0013-observability.md) | Spans, metrics and one telemetry entry point, implemented as `otel::Telemetry` (its open question on making `tracing` a default feature records the maintainer's decision of 2026-09-23: yes) | Accepted |
| [0014](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0014-auth-rejection-status.md) | A refused credential answers each binding's own status: HTTP `401`/`403`, gRPC `UNAUTHENTICATED`/`PERMISSION_DENIED` (supersedes ADR 0010's `InvalidRequest` mapping) | Accepted |
| [0015](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0015-audit-trail.md) | A tamper-evident audit trail: calls, runs, task events and cancel requests in a SHA-256 hash chain per tenant, content by digest, signed checkpoints and anchors, a six-month retention floor and legal holds | Accepted |
| [0016](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0016-types-generated-from-the-proto.md) | The public types are generated from `a2a.proto` by `tools/a2a-codegen`, the output is checked in, and CI fails on drift | Accepted |
| [0017](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0017-context-lookup-and-stored-task.md) | The send's context lookup lists without history and reads only the chosen task whole; `RequestContext::stored_task` stays the whole stored task | Accepted |
| [0018](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0018-mutual-tls-caller-identity.md) | A mutual-TLS client certificate reaches the interceptors on every binding, and `MutualTlsAuthInterceptor` names the caller by its SHA-256 fingerprint or a deployment's mapping; no X.509 parser in the mutual-TLS path (the `signing` feature parses `x5c` certificates for key validity) | Accepted |
| [0019](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0019-shared-halt-store.md) | Halts are written through to a `HaltStore` that every replica reads before its first send and polls; a halt the store refuses holds where it was given and is retried | Accepted |
| [0020](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0020-delegation-lease.md) | A delegated child carries a lease, renewed by a `GetTask` that activates the lease extension; the deadline lives in a store every replica shares, and a child whose lease lapses is stopped | Accepted |
| [0021](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0021-originating-principal.md) | The originating principal travels as an RFC 8693 delegated token: the server reads its `act` chain into `CallContext::actors` and the audit record, and the client exchanges tokens at the identity provider | Accepted |
| [0022](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0022-tool-calling-loop.md) | A tool-calling loop in the SDK: `ToolModel`, `Tool`, `ToolSet` and `ToolLoop` behind the off-by-default `tool-loop` feature, with an OpenAI-compatible model client | Accepted |
| [0023](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0023-delegation-budget.md) | A budget travels down the delegation chain: a relative deadline and named meters on the message, held by the receiving server, reserved from the parent at grant time and reported on each status update | Accepted |
| [0024](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0024-pushed-halts-and-cancels.md) | Halts and cancels are pushed to every replica: the PostgreSQL halt store notifies with each write and a listener per replica forwards it, a cancel reaches the replica running the task, and the poll stays behind the push | Accepted |
| [0025](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0025-production-profile.md) | A production profile that refuses to start: `Profile::Production` requires an authenticating interceptor, a halt store and a lease store (and, since ADR 0027, a content signer), and `ServeConfig::production()` refuses plaintext beyond loopback and an undeclared binding; the defaults are unchanged. Amended 2026-11-01: over a task store several processes share, the profile also refuses an in-memory halt, lease, push-config or audit store unless `allow_process_local_stores()` says otherwise | Accepted |
| [0026](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0026-tool-call-idempotency.md) | Tool calls carry idempotency keys, HMAC-SHA256 under the loop's secret of the task and message, the tool and its canonical arguments; a repeated call to a tool not declared read-only is answered from the first; `Delegation` keys its sends to agents that honour keys | Accepted |
| [0027](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0027-served-content-provenance.md) | Content is signed when served and verified when received: `with_content_signer` signs every artifact, status message and direct reply a server sends (not a task's history), over exactly what is sent; `with_content_keys` on the server and the client refuses altered content and marks unsigned content unverified; the production profile requires a signer | Accepted |
| [0028](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0028-webassembly-and-cloudflare-workers.md) | WebAssembly and Cloudflare Workers: the real `RequestHandler` was run inside `workerd` behind a runtime seam (`crate::rt`) and a `Send` timer; the server builds for `wasm32-unknown-unknown` with that patch, the client does not; proposes pursuing the seam in four phases and qualifying ADR 0003 for wasm32; nothing landed | Proposed |
| [0029](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0029-local-network-discovery.md) | Agents on the local network are found with mDNS/DNS-SD (`_a2a._tcp.local.`) in a crate of their own, `a2a-protocol-discovery`; one link only, no registry, and the crate is not published | Proposed; built and tested |
| [0030](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0030-delegation-resume-and-journal.md) | A delegation resumes a lost stream before it cancels the child, and a `DelegationJournal` (in-memory, PostgreSQL and SQLite) lets its record outlive the supervisor so a restarted one can reattach | Proposed; resume, reattach and the in-memory and PostgreSQL journals built; the SQLite journal built 2026-11-04 |
| [0031](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0031-discovery-beyond-one-link.md) | Agents are found across networks through sources behind one `AgentSource` interface: a static list, unicast DNS-SD, Kubernetes DNS SRV and, behind a feature, the Kubernetes API; every source requires signed cards and a host allow-list | Proposed; not built |
| [0032](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0032-http-pool.md) | A shared `HttpPool` is opt-in, fixed at construction (one TLS identity and proxy route), and owns per-origin state for an in-flight cap, circuit breaker, retry budget and ejection; client-side load balancing is recorded as a decision for the maintainer | Proposed; not built |
| [0033](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0033-group-delegation.md) | Fan-out is a `DelegationGroup`: a required in-flight cap, one join rule (`All`, `FirstOk`, `Quorum`, `FailFast`), budget shares taken at start so a loop cannot starve later children, and one cancel | Proposed; not built |
| [0034](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0034-task-lineage.md) | Task lineage is an extension (`root`, `depth`; the root defaults to the originating context), and an operator halts a job with `HaltScope::Root`; a claim used to scope a halt, never to authorize | Accepted, amended 2026-11-04 (root length in bytes, depth cap 16, `parentTaskId`, key encoding); built in 0.16.0 |
| [0035](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0035-agent-skills.md) | Agent Skills are adopted in the tool loop, as a feature-gated module (0.17.0): lenient discovery, a catalog in the prompt, an `activate_skill` tool whose name is an enum, path-confined resource reads, and no script execution | Proposed; not built |
| [0036](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0036-steering-a-running-task.md) | A message to a running task is an extension (`.../extensions/steering/v1`) delivered through a bounded per-task inbox, across replicas through the claim holder; peers without it keep today's refusal | Proposed; not built |
| [0037](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0037-push-outbox.md) | Push notifications get a durable outbox in the task database, one row per event and webhook, claimed per webhook so order holds across replicas, retried with backoff up to a maximum age, opt-in and SQL stores only; built in 0.16.0 with the row written after the event is stored, not in its transaction | Accepted; built (amended) |
| [0039](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0039-observability-stack-alerts-and-deployment.md) | The shipped alerts count the server's failures (caller mistakes and abandoned calls are not), alert on bucket bounds rather than interpolated quantiles, reach an Alertmanager that notifies nobody until given a receiver; the stack watches its own Collector, converts delta temporality, stores exemplars from Tempo's span metrics, and ships generated Kubernetes objects rather than a chart | Accepted; built |
| [0040](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0040-first-run-configuration-and-scope.md) | First-run configuration: deploy-agent reads `A2A_DATABASE_URL` before the generic `DATABASE_URL`, a schema of its own is set through `search_path` (made correct), configuration names are documented in one table and none renamed, and the published image is the ready-to-run server (no gateway binary) | Accepted; built in 0.16.0 |
| [0041](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0041-server-certificate-reload-and-crypto-policy.md) | Server certificates reload from their PEM files (`ReloadingCertResolver`, polled, keeping the old one when a pair does not load) on every listener, the gRPC one through its own rustls handshake (`with_rustls`); a client certificate a trusted TLS-terminating proxy forwards can name the caller; the crypto provider, FIPS and CRLs are the caller's `rustls` configuration, with no `fips` feature | Accepted |
| [0042](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0042-listener-liveness-defaults.md) | The gRPC and WebSocket listeners probe idle peers by default (HTTP/2 keep-alive 60 s / 20 s; WebSocket idle bound 75 s, pinging at half), with opt-outs; no default connection cap | Accepted |
| [0043](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0043-push-egress-fairness-and-proxies.md) | Push egress is shared fairly between partitions, bounded by the address it reaches as well as the name, a wait for the gate does not spend a delivery's timeout, and an `http://` webhook is pinned in the resolver and decided by name behind a proxy | Accepted; built |
| [0044](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0044-lease-renewal-and-control-plane-cost.md) | A lease renewal can ask for the lease alone (`.../lease/v1/renewal-only`) and is answered with the status alone; one read per interval checks every lease a handler watches (50 turns: 150 reads in three intervals before, 3 after) | Accepted and built for 0.16.0 (audits NEW-h-designs-3, H5, NEW-h-s0-8) |
| [0045](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0045-admission-queue.md) | An opt-in admission gate with reservations, a bounded wait, priority classes and weighted fairness (`with_admission`); off by default | Accepted; built |
| [0046](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0046-readiness-and-load-signal.md) | Readiness stays a dependency check; load is a separate signal (`GET /load`, the `endpoint-load-metrics` header); saturation drives readiness only when asked, with hysteresis; `/ready` shows a halt | Accepted; built |
| [0047](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0047-event-byte-budget-and-payload-scaling.md) | A process-wide byte budget over buffered events (`HandlerLimits::with_max_buffered_event_bytes`); blob offload stays with the executor; live-log compaction and SQL history splicing are decided and not built (live-log compaction is being built) | Accepted; byte budget built |
| [0048](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0048-tool-loop-inference-limit-and-backoff.md) | The tool loop gets a shared inference limit and a Retry-After-aware retry of a 429 inside the budget, off by default (amends ADR 0022) | Accepted; built |
| [0049](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0049-polyglot-swarm-extensions.md) | The swarm controls are published as a wire contract another language can implement: a reference page, a JSON Schema and conformance vectors for every extension, integers read from a whole double, the interop shims included from the files that run, and a machine-readable `HALTED` refusal; middleware for other SDKs and bindings are not built, upstream proposals are drafted, not sent | Accepted, amended 2026-11-05; built in 0.16.0 |
| [0054](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0054-turn-hand-off-on-shutdown.md) | On a graceful shutdown, a turn whose executor says it can be run again is handed back to its caller (`working`, marked `a2a-rust.com/hand-off`, claim released) instead of ending `Failed`; the caller continues it on another replica (this crate's `Delegation` does so by itself, at most three times by default), and the reaper ends it only after `hand_off_window` (ten minutes). Crashes and handlers without claims are out of scope | Accepted and built for 0.16.0 (audit D-21) |
| [0055](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0055-websocket-on-the-http-port.md) | The WebSocket binding is served on the HTTP port by an upgrade route on `CombinedDispatcher` and the axum router: the interceptor chain admits the upgrade before the `101`, a foreign `Origin` is refused unless listed, the connection keeps the server's connection slot and closes with a Close frame in the drain | Accepted |

## The first eight in brief

ADRs 0001 to 0008 set the shape of the workspace; each has a summary here.
Later records are in the table above only.

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

## ADR 0002: Dependency Philosophy and Minimal Footprint

**Status:** Accepted

**Context:** Rust SDK quality is inversely correlated with transitive dependency count. Each dependency adds compile time, supply chain attack surface, version conflicts, and potential license issues.

**Decision:** Minimal dependency footprint:
- No web frameworks required (optional Axum integration via `axum` feature)
- TLS through pure-Rust rustls, no OpenSSL (`tls-rustls`, a default feature of the client and SDK; `default-features = false` removes it)
- Logging through `tracing`, a default feature since 0.14 (ADR 0013); `default-features = false` compiles it out
- In-tree SSE parser instead of third-party crate
- `serde`, `tokio` and `hyper` as the only mandatory heavyweight deps

**Rationale:** A production-grade SDK must work in corporate environments with strict dependency audits, embedded/constrained environments, and without forcing users into specific TLS or logging frameworks.

## ADR 0003: Async Runtime Strategy

**Status:** Accepted

**Context:** Rust async code is runtime-agnostic at the language level, but `hyper` 1.x uses tokio internally. Making the SDK runtime-agnostic would require wrapping every I/O call behind an abstraction layer.

**Decision:** Tokio is the mandatory async runtime. It is a required dependency (not optional, not feature-gated) for the I/O crates. `a2a-protocol-types` has no async runtime dependency.

**Rationale:** >95% of Rust async production code runs on tokio. The complexity cost of runtime abstraction is not justified by the ~5% of users on other runtimes.

## ADR 0004: Transport Abstraction Design

**Status:** Accepted (superseded in part by ADR 0009 — the gRPC transport is protobuf-native as of 0.7, no longer JSON-over-gRPC)

**Context:** A2A defines three transport bindings (JSON-RPC, REST, gRPC). Both client and server share protocol logic that must not be duplicated across transport implementations.

**Decision:** Three-layer architecture:

```text
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
- Client parser: `sse_parser`, handles partial TCP frames, enforces an event size cap (16 MiB by default)
- Server emitter: `streaming/sse.rs`, formats SSE `data:` lines with proper `\n\n` terminators
- Zero additional dependencies beyond hyper

**Rationale:** Third-party SSE crates either use older hyper versions, carry unnecessary dependencies, or lack proper backpressure. The in-tree implementation is small enough to audit, test, and maintain.

## ADR 0006: Mutation Testing as a Required Quality Gate

**Status:** Accepted

**Context:** The test suite includes unit, integration, property, fuzz, and E2E dogfood tests — but none of these measure whether the tests actually *detect* real bugs. A test suite can achieve 100% line coverage with trivial assertions. At multi-data-center deployment scales, the bugs that escape traditional testing have the highest blast radius.

The full sweep runs weekly and on demand (`workflow_dispatch`); every pull
request labelled `mutation-test` runs an incremental `--in-diff` mutation gate
on the changed lines, and the required check fails until the label is present.

**Rationale:** Mutation testing is the only technique that directly measures *fault detection capability*. It provides an objective, automated answer to "would this test suite catch a real bug at this location?" The tradeoff is CI time: running the full sweep on every commit is punitive given current compute budgets, so the full sweep is scheduled, and pull requests are gated on the mutants in their own diff.

## ADR 0007: Axum Framework Integration and TCK Wire Format Tests

**Status:** Accepted

**Context:** The SDK lacked formal wire format conformance tests and required raw hyper for HTTP serving. Other SDKs integrate with their ecosystem's dominant web framework.

**Decision:** Two additions:
1. **TCK conformance tests** — 48 golden-fixture tests in `crates/a2a-protocol-types/tests/tck_wire_format.rs` validating ProtoJSON serialization, SecurityRequirement/StringList format, Part discriminators, cross-SDK interop fixtures, and full round-trip.
2. **Axum integration** — Feature-gated `A2aRouter` (`axum` feature) wrapping `RequestHandler` as an idiomatic `axum::Router`. Zero business logic duplication.

**Rationale:** TCK tests catch interop regressions before release. Axum integration reduces server setup from ~25 lines to 3 while remaining optional (the raw hyper `serve()` API is unchanged).

## ADR 0008: Object-Safe `AgentExecutor` Trait Shape

**Status:** Accepted

**Context:** `AgentExecutor` is the primary extension point users implement. The obvious question a reader asks when opening `executor.rs` is: *why not `async fn execute(...)`?* — every method returns `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>`, and every example in the repo wraps bodies with `Box::pin(async move { ... })` or uses the `boxed_future` / `agent_executor!` helpers.

**Decision:** Keep `AgentExecutor` as a manual `Pin<Box<dyn Future + Send + 'a>>` trait (object-safe), and ship two ergonomic helpers (`boxed_future` and the `agent_executor!` macro) to compensate for the call-site noise. `RequestHandler` stores the executor as `Arc<dyn AgentExecutor>`, which keeps `RequestHandler`, every dispatcher, the Axum integration, and the builder API **non-generic**.

**Rationale:** Async-fn-in-trait is stable since Rust 1.75 but produces per-impl anonymous future types that are **not object-safe on stable**. Using `async fn execute` would force `RequestHandler<E>` to become generic, and that generic parameter would then leak through every dispatcher, the Axum layer, `A2aRouter`, and the builder API. The `async-trait` crate produces exactly the same `Pin<Box<dyn Future>>` shape we have now but hides the heap allocation and forces a proc-macro dep on every downstream user — a non-starter given ADR 0002. The manual shape is honest about the cost (one `Box::pin` allocation per call, deep in the noise relative to network RTT) and keeps the public API free of trait-erasure machinery. The `agent_executor!` macro gives trivial executors the same ergonomics as `async fn` without introducing a second trait. See [ADR 0008 full document](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0008-agent-executor-trait-shape.md) for the full alternatives analysis and the revisit trigger.

## Next Steps

- **[Configuration Reference](./configuration.md)** — All tunable parameters
- **[Pitfalls & Lessons Learned](./pitfalls.md)** — Practical issues and solutions
