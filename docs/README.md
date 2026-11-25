<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Documentation

An index of the project's design records, audits and evidence. The user guide
is the book (`book/`, published at <https://a2a-rust.com>); the API reference
is on docs.rs.

## Start here

- **Asking whether it is ready for production?** [readiness-bar.md](readiness-bar.md) — what that would have to mean, written before measuring, and what has been measured against it
- **About to quote a claim this project makes?** [claims-ledger-2026-09-24.md](claims-ledger-2026-09-24.md) — every overstated claim found from 0.6.0 on, checked against the published artifacts, and what backs the ones that stand

## Conformance and compliance

| Document | What it is |
|---|---|
| [compliance/spec-ledger.md](compliance/spec-ledger.md) | Every keyword requirement of A2A v1.0.1, its status and the evidence for it; `scripts/check_spec_ledger.py` keeps it complete. The live record of spec compliance |
| [compliance/rfc-ledger.md](compliance/rfc-ledger.md) | Every MUST, SHOULD and MAY of RFC 7515 (JWS), RFC 7519 (JWT) and RFC 8725 (JWT best practice), what the code does about it and which test says so; `scripts/check_rfc_ledger.py` keeps it complete. The open and decided rows are listed there; the full table is `rfc-ledger.json` |
| [compliance/audit-register.md](compliance/audit-register.md) | Each audit finding against the spec or the code, the recommendation, and its status |
| [official-tck-findings.md](official-tck-findings.md) | Results against the A2A project's own conformance suite, requirement by requirement, with each dated measurement and every baselined failure's evidence |
| [compliance/control-map.md](compliance/control-map.md) | EU AI Act, Cyber Resilience Act, GDPR, OWASP agentic Top 10 and ISO/IEC 42001 provisions: what the SDK provides toward each, the test that proves it, and what is left to you |
| [openssf-best-practices.md](openssf-best-practices.md) | Every passing-level criterion of the OpenSSF Best Practices badge, with the file or measurement that answers it |
| [provenance-manifest.md](provenance-manifest.md) | The release's provenance measurements; `release.yml` checks it against the tagged commit |
| [implementation/a2a-v1.0.1-specification.md](implementation/a2a-v1.0.1-specification.md) | The specification text the ledger cites, vendored with its upstream hash |

## Architecture Decision Records

The complete index, one line per record, is [`book/src/reference/adrs.md`](../book/src/reference/adrs.md)
(published at <https://a2a-rust.com/reference/adrs.html>); the records themselves are in [`adr/`](adr/).

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
| [sdk-comparison-2026-11-11.md](sdk-comparison-2026-11-11.md) | One echo workload on one shared host: a tree from before 0.16.0 against a2a-rs's crates.io releases of that day. A dated snapshot: not re-measured since, and not a statement about either SDK today |
| [sdk-comparison-2026-10-02.md](sdk-comparison-2026-10-02.md) | a2a-rust 0.14.1 against a2a-rs's releases of that day: conformance suites, throughput, memory, probes. A dated snapshot, two releases old |
| [rust-sdk-assessment.md](rust-sdk-assessment.md) | Technical and governance assessment of the Rust A2A SDKs, 2026-08-26 |
| [v0.9.0-post-release-review.md](v0.9.0-post-release-review.md) | A critical read after the 0.9.0 release |
| [lf-readiness-review.md](lf-readiness-review.md) | What a critical external reviewer would find |

## When to Read What

- **New contributor?** Start with ADR-0001 (crate structure) and ADR-0004 (transport abstraction)
- **Debugging streaming?** Read ADR-0005 (SSE design) and ADR-0012 (event log)
- **Understanding the test strategy?** Read ADR-0006 (mutation testing) and ADR-0007 (TCK)

## Maintainer working files

State kept between working sessions. They are candid and unpolished by design, and are not documentation of the SDK.

- **Planning or picking up the work after 0.16.0?** [program/README.md](program/README.md) — the plan of record, approved 2026-11-25: the findings, the decisions made, the work packages and their status, the targets, and the audits behind them. A session starts at [`CLAUDE.md`](../CLAUDE.md)
- **Picking up work mid-stream?** [handoff.md](handoff.md) — the current state, what to do next and what waits on the maintainer; [handoff-history.md](handoff-history.md) keeps each earlier session's record

## License

Apache-2.0
