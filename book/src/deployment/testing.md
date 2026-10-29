# Testing Your Agent

a2a-rust lets you test an agent at several levels: grade the executor
against the protocol's invariants with no server at all, unit-test it
against a real event queue, test it end to end over HTTP, and fuzz the
parsers that read peer input. Each page below does one of those.

| You want to | Read |
|---|---|
| Check your executor handles cancellation, parking and terminal states as the protocol requires, with no server | [Grading an executor](./testing/conformance.md) |
| Grade a halt store you wrote | [Kill-switch conformance](./testing/conformance.md#kill-switch-conformance) |
| Unit-test an executor, or test it end to end over HTTP, on both transports and streaming | [Unit and integration tests](./testing/unit-and-integration.md) |
| Check the JSON on the wire, or fuzz the parsers | [Wire format and fuzz tests](./testing/wire-and-fuzz.md) |
| Measure what the SDK itself costs | [Benchmark results](../reference/benchmarks.md#reproducing-locally) |
| Work on a2a-rust itself: mutation testing and the workspace suite | [Testing a2a-rust itself](./testing/contributors.md) |

## Why no single test type is enough

A key lesson from a2a-rust is that **no single testing technique — not even all
of them together minus one — is sufficient.** Each layer catches a different
class of bug, and the gaps between layers are where production incidents hide:

| Test type | What it proves | What it cannot prove |
|---|---|---|
| **Unit tests** | Individual functions return correct values | That calling code uses those values correctly |
| **Integration tests** | Components work together pairwise | Multi-hop and emergent system behavior |
| **Property tests** | Invariants hold for all generated inputs | That real-world inputs exercise those invariants |
| **Fuzz tests** | Parser doesn't crash on malformed input | Semantic correctness of valid input handling |
| **E2E dogfooding** | The full stack works under realistic conditions | That your *assertions* actually detect regressions |
| **Mutation tests** | Your assertions detect real code changes | Protocol-level emergent behavior |

**The a2a-rust experience:** At v0.3.0 the suite held about 1,630
unit/integration/property/fuzz tests (about 1,850 with all feature flags; it has more than
doubled since) and a 94-test E2E dogfood suite whose 13 documented passes found 68 real bugs,
and CI was fully green — **mutation testing, adopted in that release, still found gaps.** Tests
that looked comprehensive were silently missing assertions on return values,
boundary conditions, and delegation correctness. The suite was green, but mutants
survived because no test *verified* the specific behavior being mutated.

This is why mutation testing is an important quality signal: it is the only
technique that measures test *effectiveness* rather than test *existence*. Every
other technique answers "does the code work?" — mutation testing answers "would
the tests catch it if the code broke?" In this repo the full sweep runs weekly
(Mondays 03:00 UTC) plus on-demand via the `workflow_dispatch` trigger on
`.github/workflows/mutants.yml`, rather than as a blocking PR gate, because a
full sweep can take 100+ minutes per crate; a separate incremental sweep
(`cargo-mutants --in-diff`) *is* a blocking PR gate, scoped to just the lines
a PR changes (it runs once the PR is labelled `mutation-test`; the required
check fails until then). See [CI/CD](./cicd.md#mutation-testing-workflow) for
the full policy.
How to run a sweep, and how to read and fix what it reports, is on
[Testing a2a-rust itself](./testing/contributors.md#mutation-testing).

## Next steps

- **[Production Hardening](./production.md)** — Preparing for deployment
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common testing mistakes
