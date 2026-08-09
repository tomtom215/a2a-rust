<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Mutation Testing History

A dated record of full-workspace mutation sweep results, so the
surviving-mutant count has a durable history rather than only living in
GitHub Actions artifacts (retained 90 days — see
[CI/CD](./../deployment/cicd.md#mutation-testing-workflow)) or transient CI
logs.

## Why this page exists

`mutants.yml` runs a full sweep weekly and fails the workflow on any
surviving mutant, but a failing scheduled workflow is a notification, not a
record: it says *this week* had a survivor, not *when the score last
changed* or *what the trend looks like*. Nothing before this page captured
that — a real gap this project should not let recur, in the same spirit as
`docs/official-tck-findings.md`'s conformance history.

## How to add an entry

After a full sweep completes (`mutants-summary` job in `mutants.yml`), copy
the aggregated table from that run's `GITHUB_STEP_SUMMARY` output into a new
row below, dated to the run, with the commit it ran against. A clean sweep
(zero missed) is still worth recording — "still zero" is signal too, not a
no-op.

## How the ledger came to be empty for so long

The first row below was recorded on 2026-08-07. Every sweep before it produced
no number at all — not "an incomplete number", none — and the reasons are worth
keeping, because each was a gate reporting success over work it had not done.

An earlier revision of this page blamed two cancelled shards for the false
green. That diagnosis was wrong, and the correction matters more than the
original finding, so it is recorded here rather than quietly edited away.

### What the 2026-07-27 run actually did

[Run 30236603180](https://github.com/tomtom215/a2a-rust/actions/runs/30236603180)
(2026-07-27, against `b416c1a`) is the only scheduled run in `mutants.yml`'s
history. Its `Mutants Summary` job concluded `success` and printed:

```text
COMBINED MUTATION SCORE: 100%
Caught: 0  Missed: 0  Timeout: 0  Unviable: 0
```

Its own uploaded artifacts — still retrievable, and re-counted directly on
2026-08-06 — contain **200 surviving mutants** out of 2,286 examined. A real
score of **91%**, reported as 100%.

| Shard | Caught | Missed |
|---|---:|---:|
| `a2a-types` | 592 | 8 |
| `a2a-client` | 357 | 10 |
| `a2a-sdk` | 0 | 0 |
| `a2a-server` 1/8 … 8/8 | 1137 | 182 |
| **Total** | **2086** | **200** |

Nine of the eleven jobs ran to completion and every one of them reported
`Missed: 0` while holding a `missed.txt` with up to 40 lines in it. The two
cancelled shards were real, but they were not the cause — the nine that
finished contributed zero as well.

### The actual mechanism

Two independent path defects, both verified against the artifacts:

1. **`--output mutants.out` does not write to `mutants.out/`.** cargo-mutants
   creates a directory *named* `mutants.out` inside whatever `--output` names
   ("Create mutants.out within this directory"), so reports landed in
   `mutants.out/mutants.out/` while every `count` read `mutants.out/`. A
   missing file counts zero lines, and zero missed mutants reads exactly like
   a clean sweep.
2. **The artifact upload absolutised every path.** `upload-artifact` was given
   `mutants.out/` *and* `/tmp/mutants-run.log`; with two paths it uses their
   least common ancestor, which is `/`. That run's log says so verbatim:
   "The least common ancestor is /. This will be the root directory of the
   artifact". Every file was stored under `home/runner/work/…`, where the
   summary job did not look.

Then one line converted "measured nothing" into a passing grade: with an
empty denominator the aggregator set `SCORE=100`.

This affected the **incremental PR gate** identically — same `--output`, same
reader — so the required check that is supposed to block a PR on a surviving
mutant could not fail either.

### What is fixed

* `--output .`, so the report is where the readers look. Verified empirically
  against cargo-mutants 27.1.0 in both forms, not inferred from the docs.
* A single upload path, so artifact entries sit at the root of `mutants.out/`.
* **A missing or malformed report is a hard error, not a zero.**
* **The aggregate refuses to publish a score over an empty denominator** — a
  whole-workspace sweep that examined no mutants has not passed, it has not
  run.
* `set -o pipefail`, so `tee` stops masking cargo-mutants' exit status.
* `count()` no longer emits `"0\n0"` for an empty-but-present file, which
  would have broken the arithmetic the moment the paths were corrected.
* The **shard-completeness gate** and **12-way `a2a-server` sharding** from
  2026-07-31 remain: they address a real second hazard (shards cancelled at
  the 120-minute timeout), they were simply not what made that run green.

**Do not backfill a number from run 30236603180.** The 91% above is recorded
as forensics, not as a measurement of the workspace: two of its shards were
cancelled, so its denominator is short by roughly two-elevenths of
`a2a-server`.

### One more gate defect, found by the first working sweep

Run [31193107921](https://github.com/tomtom215/a2a-rust/actions/runs/31193107921)
was the first sweep in which all 15 shards ran to completion and the gates
functioned. It still could not score itself: `Mutants Summary` failed at
`Require every shard to have completed` and skipped aggregation entirely,
because that check read the matrix `result`, which was `failure` — 13 shards
had *correctly* failed on surviving mutants.

The check conflated "a shard did not finish" (score is not a measurement) with
"a shard finished and did its job" (score is exactly what we want). Each shard
now writes a `COMPLETED` marker inline at the end of its run step, so it exists
if and only if cargo-mutants returned; the summary requires 15 of them and
ignores the matrix conclusion. The marker cannot live in an `always()` step —
those still run on cancellation, as run 30236603180 proved by uploading
artifacts from cancelled shards.

The score in the first row below was produced by running that fixed
aggregation over run 31193107921's 15 complete reports.

## History

| Date | Commit | Overall Score | Caught | Missed | Timeout | Notes |
|------|--------|---------------|-------:|-------:|--------:|-------|
| 2026-08-07 | [`803a139`](https://github.com/tomtom215/a2a-rust/commit/803a139664f7b9326dc8b90bd91d382ea187f481) | **92%** | 2169 | 183 | 2 | First complete sweep. Run [31193107921](https://github.com/tomtom215/a2a-rust/actions/runs/31193107921), all 15 shards finished. Per crate: `a2a-server` 1207/165 (87%), `a2a-client` 357/10 (97%), `a2a-types` 605/8 (98%), `a2a-sdk` 0/0 (a pure re-export facade — it generates no mutants). 1276 unviable. Largest survivor clusters: `handler/messaging.rs` 17, `store/task_store/in_memory/eviction.rs` 13, `dispatch/grpc/native.rs` 11. Postgres-file survivors fell 18 → 3 once the sweep got a live database. |
