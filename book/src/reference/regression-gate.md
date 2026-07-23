<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Pull-Request Benchmark Regression Gate

Every pull request against `main` runs the [`Regression Gate`][job] job in
the `Benchmarks` workflow. The job runs a focused subset of the criterion
benchmark suite twice — once on the PR's base branch, once on the PR — and
fails CI if any individual benchmark regresses beyond the configured
threshold.

[job]: ../../.github/workflows/benchmarks.yml

This page documents the **design**, the **statistical test**, and the
**known limitations** of the gate so that future contributors (and
reviewers) can evaluate its signals with informed context.

## What the gate runs

Only two of the ~14 criterion modules participate in the PR gate:

| Module | Why it's in the gate |
|---|---|
| `transport_throughput.rs` | Exercises the per-transport HTTP round-trip hot path (JSON-RPC and REST end-to-end through loopback). Most "quiet production slowdown" bugs land here. |
| `protocol_overhead.rs` | Exercises the serde hot loop (serialize/deserialize every A2A wire type plus JSON-RPC envelopes). Catches allocator thrash, missing `#[inline]` on hot helpers, and regressions from generic-explosion in derive macros. |

The full criterion suite (~14 modules, ~267 individual benchmarks) still
runs — but only on pushes to `main`, published to the [Benchmark
Dashboard][dashboard]. Running all of them twice inside a 60-minute PR job
is not realistic on a shared CI runner.

[dashboard]: ./dashboard.md

## The statistical test

Criterion's `change/estimates.json` file (produced when a bench is run
with `--baseline <name>`) records the median and mean change from the
baseline, each with a 95 % confidence interval:

```json
{
  "median": {
    "point_estimate": 0.042,
    "confidence_interval": {
      "confidence_level": 0.95,
      "lower_bound": 0.031,
      "upper_bound": 0.054
    }
  },
  "mean": { ... }
}
```

A benchmark is flagged as a regression **only when the 95 % CI lower
bound of the median change exceeds the threshold** — in other words, we
are 95 % confident that the regression is at least `threshold` slower
than the baseline. Gating on the point estimate alone was the original
implementation; it produced false positives on every PR because the
point estimate swings freely within the CI envelope on a noisy runner.

The check lives in [`benches/scripts/check_regression.py`][script].
Exit code `0` means no regression; `1` means at least one benchmark
regressed; `2` means a configuration error (no criterion output
found, malformed JSON, etc.). CI surfaces all three meaningfully.

[script]: ../../benches/scripts/check_regression.py

## The threshold — and why it's 50 %

A careful reader will notice the threshold is **50 %**, not the
more typical 10-20 %. This is deliberate, and documented here so it
isn't mistaken for carelessness.

On GitHub-hosted runners we have observed **tight-CI regressions of
~25-30 % appear on benchmarks whose production-code path did not
change at all** in the PR. Two plausible mechanisms:

1. **Runner heterogeneity.** GitHub rotates pool VMs with different
   CPU frequencies, cache sizes, and thermal budgets. Two consecutive
   benchmark runs on the "same" runner spec can differ by 20 %+ on
   small, fast benchmarks, and criterion's confidence interval
   correctly reports that the observed samples are internally
   consistent — even though the absolute numbers reflect the runner,
   not the code.

2. **Release-mode LTO inlining shifts.** `cargo bench` uses the
   release profile, which has `lto = true` and `codegen-units = 1`
   in this workspace. Under whole-program LTO, the optimizer
   considers *all* code in *all* workspace crates when making
   inlining decisions. Adding unrelated test code in a sibling crate
   can shift which functions the optimizer decides to inline,
   changing instruction-cache hit rates on the benchmarked hot path.
   This is real behaviour — not a bug in criterion — and it appears
   as a tight-CI regression on benches that touch the hot path.

A threshold of 25 % was therefore unreliable: it failed PRs whose
code demonstrably could not have caused a regression. Lifting the
threshold to 50 % still catches the regressions we want to block —
accidental O(n²) loops, allocator thrash, whole-function inlining
loss on a hot path — while staying honest about what a per-PR gate
on shared CI hardware can reliably detect.

If this project migrates to self-hosted runners with stable CPU
pinning, the threshold should come back down to 20 % or lower; the
comment in [`benchmarks.yml`][workflow] flags this for the future.

[workflow]: ../../.github/workflows/benchmarks.yml

## When the gate fails

The job's **step summary** on GitHub Actions shows:

- Every benchmark's median change and 95 % CI.
- Which benchmarks the script flagged as regressions, with their
  numbers.
- A pointer back to this page.

Before investigating as a real regression, check:

1. **Is the CI wide?** A wide CI means the samples were too noisy to
   conclude anything — this is a CI-flakiness signal, not a code
   signal.
2. **Did a sibling benchmark in the same module move by a similar
   amount in the opposite direction?** That's a strong hint of
   runner-systematic effects rather than a real regression.
3. **Does the regression reproduce on a clean local machine?** Run
   `./benches/scripts/run_benchmarks.sh --save` on `main`, then the
   same command again on the PR branch, then
   `--compare`. If the regression does not reproduce locally, it's
   CI-specific.

If after those checks the regression still looks real, a follow-up
PR should either (a) fix the regression, or (b) if it's a deliberate
trade-off, annotate the call site with a `// perf: ...` comment
explaining the trade-off and justifying the threshold hit.

## Per-benchmark overrides

A benchmark whose absolute runtime is tiny can be noisier than the 50 %
gate accommodates even when every neighbouring benchmark is stable —
allocator and cache-layout luck dominate at specific payload sizes.
`protocol/payload_scaling/from_str/16384` is the known case: it has
produced tight-CI swings past the global gate on provably identical
code, while its 4 KiB and 100 KiB neighbours sit still.

Rather than loosening the gate for every benchmark, the workflow passes
a targeted override to `check_regression.py`:

```
--override '*/from_str/16384=0.75'
```

The pattern is a glob over the benchmark name (first match wins;
repeatable flag). Overridden rows are labelled `[override N%]` in the
gate output so the raised tolerance is visible in every run, and a
regression past the raised threshold still fails the gate.

Add an override only with evidence (multiple false-positive runs on
unchanged code, stable neighbours) — an override on a benchmark that
regresses for real silently raises the bar for catching it.

## Why we run the base and PR benches sequentially

The workflow runs both sides on the same runner, in sequence, on the
same target directory. This is deliberate: consecutive runs on the
same physical machine share cache-warm state and runner-specific
noise, so the *comparison* is more stable than two independent runs
on separate runners would be — even if either individual absolute
number is noisier. Criterion's `--baseline` flag is designed for
exactly this shape of comparison.

The full criterion suite on `main` (the dashboard you see under
[Benchmark Results][benchmarks]) is a different artifact: those
numbers are the *absolute* latencies for the current `main`, not a
comparison.

[benchmarks]: ./benchmarks.md
