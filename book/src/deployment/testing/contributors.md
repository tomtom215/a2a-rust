<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Testing a2a-rust Itself

For people changing the SDK rather than building on it: how this repository
measures whether its tests would catch a bug (mutation testing), and how to
run the workspace's own suite. What CI runs, and when, is on
[GitHub Pages & CI/CD](../cicd.md).

## Mutation testing

Mutation testing is the final, critical layer of test quality assurance. While
unit tests verify correctness and fuzz tests find edge cases, mutation testing
answers a fundamentally different question: **do your tests actually detect real
bugs?**

A *mutant* is a small, deliberate code change — replacing `+` with `-`, flipping
`true` to `false`, returning a default value instead of a computed one. If the
test suite still passes after a mutation, there is a gap: a real bug in that
exact location would go undetected.

### Why this matters at scale

At multi-data-center deployment scales, the bugs that slip through traditional
testing are precisely the kind that mutation testing catches:

- **Off-by-one errors** in pagination, retry logic, and timeout calculations
- **Swapped operands** in status comparisons (e.g., `==` vs `!=` on task state)
- **Missing boundary checks** where a default return looks plausible
- **Dead code paths** where a branch is never exercised by any test

These are the subtle, semantic correctness issues that only manifest under load,
across network partitions, or during multi-hop agent orchestration — exactly the
conditions that are hardest to reproduce in staging.

### What mutation testing found in a2a-rust

Even with about 1,630 passing tests at v0.3.0 (about 1,850 with all feature flags), an E2E dogfood suite on `agent-team`, property tests, and fuzz targets —
all green — mutation testing surfaced gaps across every crate:

- **Delegation methods** returning `()` instead of forwarding calls (e.g.,
  `Arc<T>` metrics delegation, OTel instrument recording)
- **Hash function correctness** — replacing `^=` with `|=` in FNV-1a was
  undetected because no test verified specific hash values
- **Date arithmetic** — swapping `/` with `%` or `*` in HTTP date formatting
  was undetected because the only test used epoch (where all fields are 0)
- **Rate limiter logic** — replacing `>` with `>=` in window checks, `&&` with
  `||` in cleanup conditions, and `/` with `%` in window calculations
- **Builder patterns** — `builder()` returning `Default::default()` instead of
  a functional builder was undetected because existing tests used the built
  result without verifying builder-specific behavior
- **Debug formatting** — `fmt` returning `Ok(Default::default())` (empty string)
  instead of the real debug output
- **Cancellation token** — `is_cancelled()` returning a hardcoded `true` or
  `false` instead of delegating to the actual token

Every one of these mutations represents a real bug that could have been
introduced without any test catching it. The fix in each case was
straightforward: add a test that asserts the specific behavior.

### Running mutation tests

```bash
# Install the pinned, patched cargo-mutants and cargo-nextest CI runs
# (one-time setup). Stock cargo-mutants grades fewer mutants: it makes no
# viable body replacement for `*Result` aliases or boxed futures.
scripts/install_cargo_mutants.sh

# A live database. Without one the run does not produce weak results, it
# produces none: two `rate_limit::shared::postgres` tests fail in the
# UNMUTATED tree, cargo-mutants stops before testing a single mutant, and it
# says so as "cargo test failed in an unmutated tree", which reads like a
# broken checkout rather than a missing service.
export A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres

# What CI actually runs, per crate. Reproduce this exactly before concluding
# anything about a survivor — a narrower invocation measures a smaller
# feature set and reports survivors that a full one kills.
#
# The -E filter is part of "exactly". Both invocations in mutants.yml carry
# it, because --run-ignored all otherwise picks up the soak and swarm-scale
# binaries, whose tests run for 30 to 140 seconds EACH, once per mutant, and
# the audit cost measurement, which asserts nothing and, at the then 45s test kill,
# scored mutants as caught without the suite having run.
cargo mutants -p a2a-protocol-server --test-tool=nextest --profile=mutants --all-features \
  -- --run-ignored all \
     -E 'not (binary(soak) or binary(soak_multi_replica) or binary(swarm_scale) or test(=audit::privacy::tests::cost::what_protection_costs_an_append))'

# Test a specific file
cargo mutants --file crates/a2a-protocol-types/src/task.rs \
  --test-tool=nextest --all-features

# Dry-run: list all mutants without running tests
cargo mutants --list --workspace
```

### Configuration

**There is a `mutants.toml` at the workspace root and cargo-mutants does not
read it.** Measured 2026-08-14 against cargo-mutants 27.1.0, which discovers
`.cargo/mutants.toml` rather than a root-level file. Nothing in it — the
`examine_globs`, the `exclude_globs`, the timeout multiplier and cap — has ever
applied to a run. Two proofs and the consequences are recorded in
[Mutation Testing History](../../reference/mutation-history.md); the file's own
header carries them too, and `ROADMAP.md` carries the plan for activating it
(which changes both the scope and the timeout, so it needs a sweep of its own).

Read that as: **generated protobuf code under `crates/*/src/proto/` is mutated
like any other source**, and, in the full sweep, the per-mutant timeout is
cargo-mutants' default 5x the baseline with no cap (the incremental PR job
passes `--timeout 300`).

What genuinely configures a sweep is the command line in
`.github/workflows/mutants.yml`, plus one file that *is* read:

```toml
# .config/nextest.toml — nextest reads this normally.
[profile.default]
slow-timeout = { period = "20s", terminate-after = 3 }
```

That kills any single test at 60 seconds (45 until 2026-10-29). It is not a tuning knob: without it, a
mutant that makes a test *hang* rather than fail wedges the whole run until
cargo-mutants' own timeout, and is reported `TIMEOUT` — a result the workflow's
score counts in neither the numerator nor the denominator. The file's comments
name the mutant that established this and the measurement behind the numbers.

### CI integration

- **Weekly**: a full mutation sweep runs every Monday at 03:00 UTC, sharded
  across parallel runners (`.github/workflows/mutants.yml`). Any surviving
  mutant fails the build.
- **On-demand**: the same full sweep can also be triggered via
  `workflow_dispatch`, e.g. to re-run it against `main` outside the weekly
  schedule.
- **Every pull request** is gated on the incremental job (`--in-diff`). Once
  the PR carries the `mutation-test` label, only changed source lines are
  mutated and any missed mutant fails the PR. Without the label the required
  `Mutation Testing (incremental)` check fails as not run.

### Interpreting results

An illustrative summary, annotated for this page (cargo-mutants' own closing
line reads like `76 mutants tested in 22m: 26 caught, 49 unviable, 1 timeouts`,
as quoted in [Mutation Testing History](../../reference/mutation-history.md)):

```text
Found 250 mutants to test
 247 caught   ✓     # Test suite detected the mutation
   0 missed   ✗     # ALERT: test gap — add or strengthen tests
   0 timeout  ?     # No verdict — the suite hung rather than answering
   3 unviable ⊘     # Mutation caused compile error (not a gap)
```

- **Caught**: The test suite correctly detected the mutation. Good.
- **Missed**: A real bug in this location would go undetected. Add tests.
- **Unviable**: The mutation produced a compile error. Not a test gap.
- **Timeout**: the run was killed before it finished. Not a pass and not a
  failure — the mutation score is `caught / (caught + missed)`, so a timeout is
  in neither term and the score does not describe it at all. Treat a non-zero
  count as unfinished work, and note that `0 missed` and `0 timeout` are
  different statements: this project has read one as the other and published
  the mistake.

**Target: 100% mutation score** (zero missed mutants across all library crates).

### Fixing surviving mutants

When a mutant survives, `cargo mutants` prints the exact source location and
mutation. For example:

```text
MISSED: crates/a2a-protocol-types/src/task.rs:42: replace TaskState::is_terminal -> bool with false
```

This tells you that replacing the body of `is_terminal()` with `false` did not
cause any test to fail. The fix is to add a test that asserts `is_terminal()`
returns `true` for terminal states.


## Running the test suite

> **Current status:** the workspace carries several thousand tests (unit,
> integration, property and TCK conformance, across all crates) — run
> `cargo test --workspace --all-features` for the live count. The E2E dogfood
> suite is a separate binary, `cargo run -p agent-team`.

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p a2a-protocol-server

# With output
cargo test --workspace -- --nocapture

# Specific test
cargo test test_calculator_executor
```

## Benchmarks

The `benches/` directory holds the Criterion.rs suites that measure SDK
overhead independently of agent logic. Their results, what each suite covers
and how to run them and compare against a baseline are on
[Benchmark Results](../../reference/benchmarks.md#reproducing-locally); the
workflow that publishes them is described on
[GitHub Pages & CI/CD](../cicd.md).
