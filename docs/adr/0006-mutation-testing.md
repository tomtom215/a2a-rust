# ADR 0006: Mutation Testing as a Required Quality Gate

**Date:** 2026-03-17
**Status:** Accepted
**Author:** Tom F.

---

## Context

The a2a-rust test suite includes unit tests, integration tests, property-based
tests (proptest), fuzz tests, and a 72-test E2E dogfood harness. Together these
provide strong coverage of correctness and edge cases.

However, none of these techniques answer the question: **do the tests actually
detect real bugs?** A test suite can achieve 100% line coverage while containing
only trivial assertions (`assert!(true)`) or missing critical boundary checks.
Traditional coverage metrics measure test *existence*, not test *effectiveness*.

At multi-data-center deployment scales, the class of bugs that escape traditional
testing are precisely the ones with the highest blast radius:

- Off-by-one errors in retry/timeout/pagination logic
- Swapped boolean conditions in state machine transitions
- Silently returning default values instead of computed results
- Dead branches that compile but are never exercised by tests

These bugs are difficult to reproduce in staging, often only manifesting under
specific concurrency patterns, network partition scenarios, or multi-hop agent
orchestration flows.

## Decision

### Adopt `cargo-mutants` as a mandatory quality gate

1. **Tool**: [`cargo-mutants`](https://mutants.rs/) — a mature, well-maintained
   Rust mutation testing tool that integrates with Cargo's test framework.

2. **Target**: Zero surviving mutants across all four library crates
   (`a2a-protocol-types`, `a2a-protocol-client`, `a2a-protocol-server`,
   `a2a-protocol-sdk`).

3. **Scope**: All source files in `crates/*/src/**/*.rs`, excluding:
   - Thin `mod.rs` re-export files (false positives)
   - Generated protobuf code (`proto/`)
   - Tracing/logging instrumentation
   - Note: `Display`/`Debug` impls are NOT excluded — we have tests for them

4. **CI integration**:
   - **On-demand full sweep**: Triggered via `workflow_dispatch` in
     `.github/workflows/mutants.yml`. Any surviving mutant fails the build.
   - Nightly schedule and PR-gate triggers are currently disabled to save CI time.
     Re-enable when iteration stabilises.

5. **Configuration**: Centralized in `mutants.toml` at the workspace root.

### Alternatives Considered

| Alternative | Why Not |
|---|---|
| **Manual code review only** | Subjective, does not scale, misses subtle semantic issues |
| **Coverage-only metrics (llvm-cov)** | Measures execution, not assertion quality — high coverage ≠ effective tests |
| **`mutagen` (Rust)** | Requires nightly, less actively maintained, fewer mutation operators |
| **`mutation-testing-elements`** | HTML reporting framework, not a mutation engine |

## Rationale

Mutation testing is the only technique that directly measures the *fault
detection capability* of a test suite. It provides an objective, automated answer
to "would this test suite catch a real bug at this location?" — something that
code review, coverage metrics, and even property-based testing cannot guarantee.

The cost is compute time (mutation testing is inherently O(mutants × test-time)),
which is managed through:

- On-demand sweeps via `workflow_dispatch`
- Exclusion of unproductive mutation targets
- Timeout tuning in `mutants.toml`

For a production-grade, enterprise-deployed SDK, this cost is trivial compared to
the cost of a semantic bug escaping to multi-data-center production.

## Consequences

- **Positive**: Every future code change is backed by tests proven to detect
  regressions. Test suite quality becomes measurable and enforceable.
- **Positive**: Surviving mutants surface test gaps that would otherwise be
  invisible, guiding targeted test improvements.
- **Negative**: Nightly CI compute increases (~30-120 min depending on crate
  size). Mitigated by caching and parallelism.
- **Negative**: Developers must address surviving mutants before merge. This is
  intentional friction — the same class of friction as "fix clippy warnings."
