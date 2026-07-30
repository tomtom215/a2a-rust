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

2. **Whole-program-LTO inlining shifts.** The shipping profile has
   `lto = true` and `codegen-units = 1`, and by default `cargo bench`
   inherits it. Under whole-program LTO the optimizer considers *all*
   code in *all* workspace crates when making inlining decisions, so
   adding unrelated code in a sibling crate can shift which functions
   the optimizer inlines, changing the code layout and instruction-cache
   behaviour of a hot path whose own source did not change. This is real
   compiler behaviour — not a bug in criterion — and it appears as a
   *tight-CI* regression on benches that touch the affected hot path,
   large enough to blow past even a 50 % threshold.

   A concrete instance: a change that added a cold ISO-8601 helper to
   `a2a-protocol-types` produced +54..84 % tight-CI "regressions" on the
   `payload_scaling` **serialize** micro-benchmarks (which route through
   serde_json's string serializer), while every **deserialize** bench
   stayed flat, the serialize path's source was byte-identical to the
   base branch, and the delta vanished under a different compiler — the
   signature of code-layout luck rather than an algorithmic change.

   To stop this false-positive class at the source, **the gate builds its
   comparison benches without whole-program LTO** (`CARGO_PROFILE_BENCH_LTO=false`,
   `CARGO_PROFILE_BENCH_CODEGEN_UNITS=16` on the `bench-regression` job).
   Every dependency — including serde_json — is then compiled
   independently, so its hot loops are immune to unrelated changes in our
   crates, and the comparison measures the diff's algorithmic delta rather
   than LTO layout roulette. The full `main`-only suite that feeds the
   [dashboard][dashboard] still runs under the real fat-LTO shipping
   profile, so the published absolute numbers stay faithful to what ships.

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
4. **If it does reproduce locally, does it reproduce *again*, on the
   same commit pair, immediately afterward?** A single local match is
   not sufficient evidence for a tiny (low-microsecond) benchmark — see
   the `from_str/16384` case in "Per-benchmark exclusions" below, where
   the exact same commit pair, same build, same flags, measured +175 %
   and then -15 % back to back. Re-run the comparison at least once more
   before trusting a local reproduction on a small enough benchmark.

If after those checks the regression still looks real, a follow-up
PR should either (a) fix the regression, or (b) if it's a deliberate
trade-off, annotate the call site with a `// perf: ...` comment
explaining the trade-off and justifying the threshold hit.

## Per-benchmark overrides

A benchmark whose absolute runtime is tiny can be noisier than the 50 %
gate accommodates even when every neighbouring benchmark is stable —
allocator and cache-layout luck dominate at specific payload sizes.

Rather than loosening the gate for every benchmark, the workflow can
pass a targeted override to `check_regression.py`:

```
--override 'GLOB=THRESHOLD'
```

The pattern is a glob over the benchmark name (first match wins;
repeatable flag). Overridden rows are labelled `[override N%]` in the
gate output so the raised tolerance is visible in every run, and a
regression past the raised threshold still fails the gate.

Add an override only with evidence (multiple false-positive runs on
unchanged code, stable neighbours) — an override on a benchmark that
regresses for real silently raises the bar for catching it. And check
the next section before reaching for one: if a benchmark keeps
defeating a generous override, the percentage itself may not be a
meaningful signal for it, which calls for exclusion instead.

## Per-benchmark exclusions

Some benchmarks aren't just noisier than the 50 % gate — no fixed
percentage tolerance is a reliable signal for them at all. For those,
`check_regression.py` also accepts:

```
--exclude 'GLOB'
```

An excluded benchmark's numbers are still measured and printed every
run, labelled `[EXCLUDED]` — this is not `continue-on-error`, and it does
not hide the benchmark from the job summary. It just cannot fail the
gate on its own, at any magnitude. Everything else keeps gating
normally; excluding one benchmark does not widen tolerance for its
neighbours (verified: `check_regression.py`'s own test run confirms an
excluded regression is dropped from the failing set while an
unexcluded one in the same run still fails the job).

**`protocol/payload_scaling/from_str/16384` is the case that motivated
this.** It started as a `--override */from_str/16384=0.75` entry (see
the previous section's history) after repeatedly producing tight-CI
swings past the 50 % default on provably unchanged code. PR #99 showed
that 75 % isn't a ceiling either — the investigation, dated 2026-07-29:

1. Two separate CI runs on the **same commit** measured `from_str/16384`
   at +169 % and +175 %, both with confidence intervals about 1
   percentage point wide — internally tight, and reproducible across
   runs, which initially looked like a real, deterministic effect
   rather than noise.
2. A local reproduction using this job's *exact* build configuration
   (`CARGO_PROFILE_BENCH_LTO=false`, `CARGO_PROFILE_BENCH_CODEGEN_UNITS=16`,
   sequential `git checkout` in one target directory, the same
   `--warm-up-time 1 --measurement-time 3 --sample-size 30` flags)
   matched CI almost exactly on the first attempt: +175 %.
3. Measuring that **same commit pair again immediately afterward**, with
   nothing else changed, flipped the result to **-15 %** — on identical
   source, identical baseline, identical build flags, back to back.
4. A full bisection of every commit on that PR against `main`, using the
   same exact-CI-build method, showed every single commit as an
   *improvement* (-5 % to -21 %) on this benchmark — never a regression,
   at any point in the PR's history.
5. `protocol/payload_scaling/from_slice/16384` — same payload size, same
   `Message`/`Part` `Deserialize` implementation, entered via a
   different reader constructor — never moved. If the regression were
   in shared deserialization logic, both would show it.

Taken together: this benchmark's *within one process launch* variance
is small (hence the tight CIs — all 30 samples in a run share that
launch's memory layout), but its *launch-to-launch* variance is large
enough to exceed even a 75 % tolerance, in either direction, on
byte-identical code. That is a description of ASLR/cache-layout luck at
an unusually small scale (~2 µs per call, small enough that layout
effects are a large fraction of the total), not of a percentage that
was merely set too low. No single fixed override could have caught
real regressions here without also flagging this noise, so gating on
it at all was the wrong tool — hence exclusion rather than a larger
override.

If a future change to this benchmark's payload size or implementation
changes this calculus (e.g. batching multiple parses per sample to
dilute the per-launch layout cost), the exclusion should be
reconsidered rather than assumed permanent.

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
