# ADR 0006: Mutation Testing as a Required Quality Gate

**Date:** 2026-03-17
**Status:** Accepted
**Author:** Tom F.

---

## Context

The a2a-rust test suite includes unit tests, integration tests, property-based
tests (proptest), fuzz tests, and a 95-test E2E dogfood harness. Together these
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
   `a2a-protocol-sdk`), with the single documented exception in
   [Equivalent mutants](#equivalent-mutants) below.

   The target is unconditional; the *enforcement* is scoped. A PR must add no
   survivors to the lines it changes, and that is blocking. The workspace
   figure is a tracked standing target — **94%, 125 surviving, as of
   2026-08-10** — burned down over time rather than waived. There is
   deliberately no baseline or allowlist file: the incremental gate already
   prevents the count from growing, so a mechanism whose only purpose is to
   turn a red result green would buy nothing and cost the signal.

3. **Scope**: All source files in `crates/*/src/**/*.rs`, excluding:
   - ~~Thin `mod.rs` re-export files (false positives)~~ — *amended
     2026-06-10: the blanket `**/mod.rs` exclusion was removed. cargo-mutants
     only mutates function bodies, so pure re-export files generate no
     mutants anyway, while 14 `mod.rs` files carrying real logic (~2,800
     lines) were silently exempt. Only generated protobuf code is excluded
     now.*
   - Generated protobuf code (`proto/`)
   - Tracing/logging instrumentation
   - Note: `Display`/`Debug` impls are NOT excluded — we have tests for them

4. **CI integration** (`.github/workflows/mutants.yml`):
   - **Incremental, per PR** — mutates only the lines the PR changed
     (`--in-diff`). Blocking. This is the gate that holds the line.
   - **Full workspace sweep** — weekly on a schedule, and on demand via
     `workflow_dispatch`. Reports; does not block a merge.

   *Amended 2026-08-07.* Both gates were, until that date, structurally
   incapable of failing: cargo-mutants wrote its report to
   `mutants.out/mutants.out/` while every reader looked in `mutants.out/`, so
   a missing file counted as zero survivors and an empty denominator was
   scored as 100%. The 2026-07-27 sweep reported `100%, Missed: 0` over
   artifacts containing 200 surviving mutants. An ADR that declares a
   mandatory gate should record when that gate was not, in fact, mandatory —
   see [`book/src/reference/mutation-history.md`](../../book/src/reference/mutation-history.md)
   for the full diagnosis and the score history since.

5. **Configuration**: Centralized in `mutants.toml` at the workspace root.

### Equivalent mutants

Some mutants cannot be killed by any test, because the mutation is
*semantically equivalent* to the original — the program's observable behaviour
is unchanged. `x * 1` versus `x`, an unreachable defensive branch, a match arm
the wildcard already covers. These are a known, unavoidable property of
mutation testing, not a defect in the suite, and no amount of test-writing
removes them.

They are therefore the one accepted exception to the target above, subject to
three conditions:

1. **Prove it, don't assume it.** "I could not think of a test" is not
   equivalence. The claim is that *no* test can distinguish the mutant, which
   means being able to say why the mutated expression is observationally
   identical. A surviving mutant is a test gap until demonstrated otherwise;
   in this project's own experience that default has been correct far more
   often than not — of the 18 Postgres-file survivors in the 2026-07-27 sweep,
   17 were killable and 15 died to a CI fix rather than any test at all.

2. **Mark it in the source, next to the code.** The mechanism is
   `#[mutants::skip]`, with a comment giving the reason, so the exemption is
   visible to anyone reading the function and shows up in the diff when the
   surrounding code changes:

   ```rust
   // The wildcard arm already covers this case; splitting it out is for
   // readability, so deleting it changes no behaviour.
   #[mutants::skip]
   fn example() { /* … */ }
   ```

   **This carries a prerequisite, and it is not free.** The attribute resolves
   through the [`mutants`](https://crates.io/crates/mutants) crate; without it
   the build fails outright with `error[E0433]: failed to resolve: use of
   unresolved module or unlinked crate 'mutants'` (verified against
   cargo-mutants 27.1.0). It also cannot be a dev-dependency: cargo-mutants
   builds the library normally, so the attribute must resolve in a non-test
   build.

   This workspace does **not** currently depend on `mutants`, and adding it to
   a crate published on crates.io puts it in every downstream user's
   dependency tree — a supply-chain decision for a project that maintains a
   `deny.toml`, an SBOM and SLSA provenance. So the first genuinely equivalent
   mutant is also the trigger for that decision, and should be raised as one
   rather than settled inside an unrelated PR.

   Until then the target has been met the ordinary way, by writing tests. No
   exemption has yet been needed.

3. **Do not use `mutants.toml` for this.** Config-level `exclude_globs` and
   `exclude_re` are for whole categories that are never worth mutating —
   generated protobuf code, tracing macros. They are invisible at the call
   site and they rot silently: a blanket `**/mod.rs` exclusion once exempted
   ~2,800 lines of real logic in this repository for months, and nothing in
   the source said so. Per-case, in-source, reviewable in the diff, or not at
   all.

An exemption is a claim about the code, and like any other claim in this
project it is reviewed on the evidence given for it.

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
- **Negative**: Developers must address surviving mutants **on the lines their
  PR changes** before merge. This is intentional friction — the same class of
  friction as "fix clippy warnings."

  The qualifier is load-bearing and was missing until 2026-08-11. Read without
  it, this bullet said a contributor must clear the whole workspace's
  survivors — 125 of them, in code no recent PR has touched — which is not what
  any gate enforces and not a bar this project holds anyone to. `CONTRIBUTING.md`
  was corrected on this point earlier; this ADR was not, which left the
  governing document contradicting the enforced rule. That is exactly the class
  of defect this repository keeps finding, so it is fixed here rather than
  noted.
