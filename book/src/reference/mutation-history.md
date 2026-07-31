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

## Why there is still no row (verified 2026-07-31)

The ledger below is empty, and that is not an oversight — **no full sweep has
ever completed.** Checked against the Actions API rather than assumed:

* `mutants.yml` has exactly **one** scheduled run in its history:
  [30236603180](https://github.com/tomtom215/a2a-rust/actions/runs/30236603180),
  2026-07-27, against `b416c1a`. Run conclusion: `cancelled`.
* Of its 11 sweep jobs, nine succeeded. `a2a-server` shards **3/8 and 4/8**
  were cancelled at the 120-minute job timeout — the shards that did finish
  took 86-113 minutes, so those two were only ever a few minutes from the
  limit.
* **`Mutants Summary` nonetheless concluded `success`.** A cancelled shard
  still runs its upload step, so it contributes an empty `mutants.out`; the
  aggregator's `count()` reads zero missed mutants from it, and the job's only
  failure condition was `TOTAL_MISSED > 0`. Two elevenths of the workspace
  went unmutated and the gate reported everything caught.

That is the failure mode this project refuses everywhere else: a green check
over work that did not happen. The same hazard was already understood for the
*incremental* PR job — its header says a job that overruns "gets cancelled, so
the gate silently stops enforcing" — but the full sweep had no equivalent
guard.

Both halves are now fixed in `mutants.yml`:

1. **A shard-completeness gate** runs before aggregation and fails unless the
   matrix result is `success` *and* the expected number of per-shard reports
   arrived. A scoped `workflow_dispatch` (one package) is exempt from the
   count, since it skips the other shards by design.
2. **`a2a-server` is sharded 12 ways instead of 8**, cutting per-shard load by
   a third so the slowest shard finishes with headroom rather than four
   minutes to spare.

So the first real row still has to come from a real, complete sweep — the next
Monday run, or an on-demand `workflow_dispatch`. **Do not backfill a number
from run 30236603180**: its score was computed from partial data and is not a
measurement of the workspace.

## History

| Date | Commit | Overall Score | Caught | Missed | Timeout | Notes |
|------|--------|---------------|-------:|-------:|--------:|-------|
| _(none recorded yet)_ | | | | | | No full sweep has completed. The only scheduled run (2026-07-27, `b416c1a`) lost two `a2a-server` shards to the 120-minute timeout while its summary job still reported success — see the section above. The completeness gate and 12-way sharding that fix this landed 2026-07-31; the first complete sweep after that date is the first eligible row. |
