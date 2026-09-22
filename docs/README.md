<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Documentation

Architecture Decision Records (ADRs) and implementation planning documents for the a2a-rust project.

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

## Implementation Documents

| Document | Purpose |
|----------|---------|
| [plan.md](implementation/plan.md) | Development roadmap (all 9 phases complete) |
| [spec-compliance-gaps.md](implementation/spec-compliance-gaps.md) | Spec compliance tracking |
| [type-mapping.md](implementation/type-mapping.md) | A2A spec to Rust type mapping |
| [v1-upgrade-plan.md](implementation/v1-upgrade-plan.md) | Migration plan to v1.0.0 |

## When to Read What

- **New contributor?** Start with ADR-0001 (crate structure) and ADR-0004 (transport abstraction)
- **Debugging streaming?** Read ADR-0005 (SSE design)
- **Understanding the test strategy?** Read ADR-0006 (mutation testing) and ADR-0007 (TCK)
- **Planning a feature?** Check the implementation plan for architectural context
- **Wondering whether this scales to a swarm?** Read [swarm-scale-findings.md](swarm-scale-findings.md) — measured behaviour at a thousand agents sharing one task, and why the shard key has to be the context
- **Picking up work mid-stream?** Read [handoff.md](handoff.md) — branch state, what is in flight outside this repository, what to do first, and the ideas the project has not committed to

## License

Apache-2.0
