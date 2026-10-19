<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Documentation

An index of the project's design records, audits and evidence. The user guide
is the book (`book/`, published at <https://a2a-rust.com>); the API reference
is on docs.rs.

## Start here

- **Asking whether it is ready for production?** [readiness-bar.md](readiness-bar.md) — what that would have to mean, written before measuring, and what has been measured against it
- **About to quote a claim this project makes?** [claims-ledger-2026-09-24.md](claims-ledger-2026-09-24.md) — every overstated claim found from 0.6.0 on, checked against the published artifacts, and what backs the ones that stand
- **Comparing this SDK with the official one?** [sdk-comparison-2026-10-02.md](sdk-comparison-2026-10-02.md) — both crates.io releases measured on one machine: official conformance and interop suites, throughput, memory, a live model, black-box tenant/auth/persistence probes; where each one is better, and the harness to re-run it
- **Picking up work mid-stream?** [handoff.md](handoff.md) — the current state, what to do next and what waits on the maintainer; [handoff-history.md](handoff-history.md) keeps each earlier session's record

## Conformance and compliance

| Document | What it is |
|---|---|
| [compliance/spec-ledger.md](compliance/spec-ledger.md) | Every keyword requirement of A2A v1.0.1, its status and the evidence for it; `scripts/check_spec_ledger.py` keeps it complete. The live record of spec compliance |
| [compliance/audit-register.md](compliance/audit-register.md) | Each audit finding against the spec or the code, the recommendation, and its status |
| [official-tck-findings.md](official-tck-findings.md) | Results against the A2A project's own conformance suite, requirement by requirement, with each dated measurement and every baselined failure's evidence |
| [compliance/control-map.md](compliance/control-map.md) | EU AI Act, Cyber Resilience Act, GDPR, OWASP agentic Top 10 and ISO/IEC 42001 provisions: what the SDK provides toward each, the test that proves it, and what is left to you |
| [openssf-best-practices.md](openssf-best-practices.md) | Every passing-level criterion of the OpenSSF Best Practices badge, with the file or measurement that answers it |
| [provenance-manifest.md](provenance-manifest.md) | The release's provenance measurements; `release.yml` checks it against the tagged commit |
| [implementation/a2a-v1.0.1-specification.md](implementation/a2a-v1.0.1-specification.md) | The specification text the ledger cites, vendored with its upstream hash |

## Architecture Decision Records

| ADR | Title | Summary |
|-----|-------|---------|
| [0001](adr/0001-workspace-crate-structure.md) | Workspace Crate Structure | Why 4 crates instead of a monolith |
| [0002](adr/0002-dependency-philosophy.md) | Dependency Philosophy | Minimal, auditable dependency tree |
| [0003](adr/0003-async-runtime-strategy.md) | Async Runtime Strategy | Tokio as the primary async runtime |
| [0004](adr/0004-transport-abstraction.md) | Transport Abstraction | Pluggable dispatchers for JSON-RPC, REST, WebSocket, gRPC |
| [0005](adr/0005-sse-streaming-design.md) | SSE Streaming Design | In-tree SSE with zero additional dependencies |
| [0006](adr/0006-mutation-testing.md) | Mutation Testing | Systematic mutation testing strategy |
| [0007](adr/0007-axum-integration-and-tck.md) | Axum Integration & TCK | Axum adapter and wire-format conformance testing |
| [0008](adr/0008-agent-executor-trait-shape.md) | Object-Safe `AgentExecutor` Trait Shape | A manual boxed future, with helpers to pay the ergonomic tax |
| [0009](adr/0009-protobuf-native-grpc.md) | Protobuf-Native gRPC Binding | Generated types on the wire, not JSON in a gRPC envelope |
| [0010](adr/0010-auth-integration.md) | First-party authentication helpers | Which auth schemes ship in-tree, and where they sit |
| [0011](adr/0011-task-retention.md) | Task Retention | Explicit, opt-in purging; the store schedules nothing |
| [0012](adr/0012-event-log-and-resumption.md) | Event Log and Resumption | An append-only log beside the snapshot; SSE `id:` and `Last-Event-ID` |
| [0013](adr/0013-observability.md) | Observability | `tracing` spans exported through `tracing-opentelemetry`; one RPC span shape on every binding; semconv metrics beside the deprecated catalogue; one entry point, `otel::Telemetry`. The ADR still reads "Proposed", though its decision has shipped |
| [0014](adr/0014-auth-rejection-status.md) | Auth Rejection Status | A refused credential answers `401`/`403` and gRPC `UNAUTHENTICATED`/`PERMISSION_DENIED`; supersedes ADR 0010's `400` mapping |
| [0015](adr/0015-audit-trail.md) | Audit Trail | Every call, run, task event and cancel request in a hash chain per tenant, content by digest, signed checkpoints, six-month retention floor, legal holds |
| [0016](adr/0016-types-generated-from-the-proto.md) | Types Generated from the Proto | The public types are generated from `a2a.proto`, and CI fails on drift |
| [0017](adr/0017-context-lookup-and-stored-task.md) | Context Lookup and Stored Task | The send's context lookup lists without history and reads the chosen task whole; `RequestContext::stored_task` keeps its meaning |
| [0018](adr/0018-mutual-tls-caller-identity.md) | Mutual-TLS Caller Identity | Every listener passes the client certificate to the interceptors; `MutualTlsAuthInterceptor` names the caller by fingerprint or a deployment's mapping, with no X.509 parser in the SDK |
| [0019](adr/0019-shared-halt-store.md) | Shared Halt Store | Halts written through to a store every replica polls and reads before its first send; a halt the store refuses holds locally and is retried |
| [0020](adr/0020-delegation-lease.md) | Delegation Lease | A delegated child carries a lease renewed by a `GetTask` that activates the extension; the deadline lives in a shared store, and a lapsed child is stopped |
| [0021](adr/0021-originating-principal.md) | Originating Principal | Delegated calls carry the principal by RFC 8693 token exchange; the server reads the `act` chain into `CallContext::actors` and the audit record |

## Audits, measurements and design notes

- [adopter-audit-2026-09-22.md](adopter-audit-2026-09-22.md) — using this SDK as a coordinator: seventy findings across every crate, which are fixed, and what is still open
- [swarm-scale-findings.md](swarm-scale-findings.md) — measured behaviour at a thousand agents sharing one task, and why the shard key has to be the context
- [swarm-orchestration.md](swarm-orchestration.md) — what a swarm needs from this layer, what `examples/swarm` measured (cancellation does not cascade), and the order to close the gaps
- [upstream/](upstream/) — reports and drafts for projects this one depends on or interoperates with; its README gives each one's status

## Historical

Dated records, kept as written; the documents above supersede them.

| Document | What it was |
|----------|---------|
| [implementation/plan.md](implementation/plan.md) | Implementation history (Phases 0–9, complete) |
| [implementation/spec-compliance-gaps.md](implementation/spec-compliance-gaps.md) | Gap tracking against v1.0.0, March 2026, all resolved; current status is [compliance/spec-ledger.md](compliance/spec-ledger.md) |
| [implementation/type-mapping.md](implementation/type-mapping.md) | A2A spec to Rust type mapping |
| [implementation/v1-upgrade-plan.md](implementation/v1-upgrade-plan.md) | Migration plan to v1.0.0 |
| [rust-sdk-assessment.md](rust-sdk-assessment.md) | Technical and governance assessment of the Rust A2A SDKs, 2026-08-26 |
| [v0.9.0-post-release-review.md](v0.9.0-post-release-review.md) | A critical read after the 0.9.0 release |
| [lf-readiness-review.md](lf-readiness-review.md) | What a critical external reviewer would find |

## When to Read What

- **New contributor?** Start with ADR-0001 (crate structure) and ADR-0004 (transport abstraction)
- **Debugging streaming?** Read ADR-0005 (SSE design) and ADR-0012 (event log)
- **Understanding the test strategy?** Read ADR-0006 (mutation testing) and ADR-0007 (TCK)

## License

Apache-2.0
