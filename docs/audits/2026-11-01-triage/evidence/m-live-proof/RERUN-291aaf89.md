<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Re-run on the fix (`291aaf89`)

This file continues `README.md` (the run on `main`); its findings 1 and 2 are the ones fixed here.

Findings 1 and 2 of `README.md` were fixed by the maintainer in `291aaf89` (`renewal.rs`/`options.rs`: a `reattach_with` with a lease makes its first renewal at once; `book/src/client/delegation.md`: the recovery loop reattaches every record, then waits on all together). This section runs that library, live, with the same harness. `crates/` and the book are read-only here. The measurements in `README.md` stay as the "before".

What changed in the harness: `a2a-live-supervisor recover-all` is the book's new loop (I added log lines, a record map and the `LEASE_SECS` option to the book's code; `recover` is unchanged and is now the control, the old serial loop); `delegation_live.py --restart-delays` sets the delay from the kill to the **launch** of `recover` (the `restart` of the earlier tables was measured to a snapshot taken just before the launch, so the two differ by about 0.2 s); `timeline.py` merges a run's results with the worker's stop and completion times in the run's own clock. Library under test, binaries, environment: `logs/fix/environment.txt`. Two passes were made: the first with a supervisor without the failure handling (`LINGER_SECS`, added for `afail`), every run of the main scenarios passing (`logs/fix/first-pass.txt`); the tables below are the second pass, on the final binaries, which also ran `afail` and `afailold`.

Commands (from the repository root, after the build in `README.md`; seed 20261104, 50 children x 60 s, 15 s leases, kill -9 at 20 s, a fresh database per run; `D` is `python3 scripts/live/delegation/delegation_live.py --worker $B/a2a-live-worker --supervisor $B/a2a-live-supervisor --children 50 --hold-secs 60 --kill-at 20 --lease-secs 15 --seed 20261104`):

    $D --scenarios alold   --runs 3 --restart-delays 3,7,7.3
    $D --scenarios alfix   --runs 3 --restart-delays 3,7,7.3
    $D --scenarios allate  --runs 3 --restart-delays 12
    $D --scenarios afix    --runs 1 --restart-delays 5.4
    $D --scenarios aledge  --runs 3 --restart-delays 9.5
    $D --scenarios aledge  --runs 3 --restart-delays 10.0,10.3,10.6
    $D --scenarios afail   --runs 3 --restart-delays 5.4
    $D --scenarios afailold --runs 1 --restart-delays 5.4

### Expectations, written before

## Expectations for the re-run on the fix (291aaf89), written before it was run

Recorded 2026-11-04, after the harness had been extended (`recover-all`,
`--restart-delays`, five new scenarios) and smoke-run at 5 children, 30 s
holds and 12 s leases (all of them behaved as below; those numbers are not
used anywhere), and before any 50-child run on the fixed library. The
maintainer's expectations are taken as given and challenged where I think
they are loose. The library under test is `crates/` at 291aaf89: a
`reattach_with` with a lease makes its first renewal at once
(`renewal.rs`, `options.rs`), and the book's loop is the new one.

Timeline from the previous run (`logs/results.jsonl`, `al` runs), t in seconds
from the supervisor's launch: the leases are granted at t = 0.3 to 0.7 and
renewed every 5 s, the kill is at t = 20.0 (the dead supervisor's last
renewal was its tick at about t = 15.3), the leases lapse at about t = 30.3
and the worker stopped the children at t = 30.1 to 31.4. `recover` starts
about 0.1 s after the delay given, so at t = 23.1, 27.1 and 27.4 for delays of
3, 7 and 7.3 s, and at 32.1 for 12 s.

Setup as the previous `al`: 50 children of 60 s, 15 s leases, kill -9 at 20 s,
the worker granting leases, a fresh database per run, seed 20261104, 3 runs
per scenario, the restart delay set explicitly from the kill to the launch of
`recover`.

**(alold) the old serial loop (`recover`) on the fixed library, delays 3, 7 and
7.3 s.** In all three runs exactly 1 child completes and 49 are canceled
(journal `completed` 1, `canceled` 49, none open; tasks the same; 49
`token-fired` lines at the worker). The first child is reattached at t = 23 to
27.5, before its lease lapses at t = 30.3, and now renewed at once, so the 7
and 7.3 s runs no longer lose it as the 7.3 s run did before the fix. The other
49 have no handle until the first child ends at about t = 60.4, and are stopped
at t = 30 to 31.5. `recover` exits about 60.5 s after the launch of the
supervisor, so 37, 33.5 and 33.2 s after its own launch.

**(alfix) `recover-all` on the fixed library, delays 3, 7 and 7.3 s.** In all
three runs all 50 complete: journal `completed` 50, 0 `canceled`, none open;
tasks `completed` 50; 0 `token-fired` and 0 `cancel-hook` lines. Reattaching
all 50 takes about 0.5 s (the smoke run: 53 to 57 ms for 5, so about 11 ms
each, an extrapolation), so every child is renewed by t = 28 at the latest,
2.3 s before the lapse in the worst run (7.3 s). `recover-all` exits within a
second of the last child's completion line.

*Challenge:* the margin in the 7.3 s run is about 2 to 3 s, and it is the only
run that tests the fix near its limit; 3 and 7 s do not. If reattaching 50
live subscriptions takes more than that, the children at the end of the list
are lost and the run fails, which would be a fact about the cost of a
reattach and not a contradiction of the fix.

**(allate) `recover-all`, delay 12 s (starts at t = 32.1).** The leases have
lapsed and the children were stopped at t = 30.1 to 31.4 in the previous run,
so before `recover-all` starts: the snapshot taken just before it shows the 50
tasks `canceled`, and `recover-all` reattaches 50 canceled children. Expected:
journal `canceled` 50, none open, 0 completed. It documents the limit and is
not a failure. *Caveat:* a child whose lapse the worker has not yet noticed
when the renewal arrives would be saved (the worker checks a lease every 3.75
s), so a few completions are possible; I do not expect any, because the
previous stops all came before t = 31.5.

**(afix) scenario `a` with `recover-all` and no leases, one run, delay 5.4 s**
(the delay of the previous `a` run 1). 50 children complete, journal
`completed` 50, 0 `canceled`, none open, tasks `completed` 50, 0 cancel lines;
`recover-all` exits within a second of the last completion (the serial loop
took 0.28 to 2.03 s after it).

**(aledge) extra, not asked for: `recover-all` at 9.5 s (starts at t = 29.6),
3 runs.** Half a second to a second before the lapse. I predict no outcome:
anything from all 50 completing to most being lost, depending on how the
reattach of 50 compares with the 0.5 to 1 s left and on the worker's polling
phase. The scenario only checks that no record is left open; the numbers are
reported.

**What would make me wrong:** the renewal made at once being refused or
dropped by the worker; a reattach of a working task costing far more than 11
ms at 50 children; `recover-all` holding 50 open subscriptions slowing the
worker or the pool of 10 so that completions are late; the worker's lease
watcher phase (3.75 s) letting some children be saved in (alold) or lost in
(alfix) at the margin.

(sha256 of this section's text, printed into the log when the runs started: `f30824c8ef939c2e3f36c8cffd6b3fc36afb9bda964205a3e2b9bb5b18637135`)

## Prediction for the edge probe (`aledge` at 10.0, 10.3 and 10.6 s), written after the main fix runs and before this probe

Recorded 2026-11-04 after the main re-run (all 13 runs of `alold`, `alfix`,
`allate` and `afix` and the first three `aledge` runs at 9.5 s had matched
their expectations: 50 complete at 9.5 s, all 50 stopped at 12 s) and before
any run at 10.0, 10.3 or 10.6 s. Extra, not asked for. It looks for where the
limit lies between 9.5 s (all saved) and 12 s (all lost).

Reasoning from `crates/a2a-protocol-server/src/store/lease/mod.rs` (the
in-memory `renew` sets the deadline to now + ttl for any lease still in the
map, even a lapsed one) and `handler/lease.rs:141-178` (a watcher polls each
lease every ttl / 4 = 3.75 s from the grant and stops the task only when it
finds the lease lapsed): a renewal saves a child if it reaches the worker
before that child's poll that follows the deadline, and loses it otherwise.
In the previous runs the stops were at t = 30.1 to 31.4 s, which I read as the
8th poll of each watcher (grant + 30.0 s, grants at t = 0.3 to 0.6 s), just
after the deadline (last renewal at about grant + 15 s, plus 15 s). `recover-all`
renews child k about 0.2 s after its launch plus 4 ms per child, in the order of
the records, which is the order of the grants.

Prediction (a CONJECTURE about a sharp edge, not a measurement):

1. 10.0 s (launch at t = 30.1, renewals at about t = 30.3 to 30.5): most or all
   of the 50 complete, because most renewals precede the polls at t = 30.35 to
   30.65. Anything from 40 to 50 completing would match.
2. 10.3 s (launch at t = 30.4, renewals at about 30.6 to 30.8): the edge itself;
   I expect a split, neither all 50 nor none completing.
3. 10.6 s (launch at t = 30.7, renewals at about 30.9 to 31.1): most or all
   lost, 0 to 10 completing.

A run that comes out otherwise (all 50 at 10.6 s, say) would mean the watcher
phase is not what I think. The scenario only checks that no record is left open.

(sha256 of this section's text, printed into the log when the probe started: `2918095f28818feb59c8d16a65e003df74dba536b3daeebf5a119b6061c43528`)

Correction of that text: its first sentence counts "all 13 runs of `alold`, `alfix`, `allate` and `afix`"; the right count is 10 runs of those four scenarios plus the 3 `aledge` runs at 9.5 s, 13 in all. The hash is of the text as written.

## Prediction for one bad record (`afail`, `afailold`), written after the main re-run and before these runs

Recorded 2026-11-04, after the first pass of the main re-run had matched its
expectations and after a 6-child smoke run of `afail` and `afailold` (4
children canceled by `recover-all`, none by the serial loop; those numbers are
not used below). Extra, not asked for. It tests a hazard I see in the new loop
of the book, not a claim of the maintainer's.

The new loop (`book/src/client/delegation.md`) collects the reattached handles in
a `Vec` and ends each reattach with `?`. A `Delegation` that is dropped before
its child settled cancels the child in the background (`delegation/drop.rs`:
"Cancels an unsettled child in the background"). So if `reattach_with` fails
for one record, say one whose task the agent has purged, `?` returns, the
`Vec` is dropped, and every child already reattached is sent `CancelTask`. In
the old serial loop the same failure only stops the loop, and the children it had
already waited on are finished.

Setup: scenario `a` without leases (50 children, kill -9 at 20 s, restart 5.4 s
after the kill), plus one extra open record in the journal for the same owner,
naming a task id the worker does not have (`no-such-task`), inserted by the driver
so that `journal.open` lists it straight after the 26th of the 50 real records
(24 real records come after it). `LINGER_SECS=3` keeps a failed recovery alive for 3 s
before it exits 1, standing in for the rest of a supervisor's life: without it the
process exits at once and the background cancels race with the exit.

**(afail) `recover-all`.** It reattaches 26 children, fails on the 27th record
(`ERROR`), and exits 1. The 26 handles are dropped: 26 tasks end `canceled`
(26 `cancel-hook` lines at the worker), and their 26 journal records are settled
`canceled` (the drop's cancel settles the record). The other 24 are never
reattached: they run on and complete at 60 s, their 24 records stay `open` and
the bogus record stays `open`. Final state: tasks 26 canceled and 24 completed;
journal 26 canceled, 25 open.

**(afailold) `recover` (serial).** It reattaches and waits for the 26 children one
after another, all completing at 60 s, fails on the 27th record and exits 1. Tasks
all 50 completed, 0 cancel lines; journal 26 completed, 25 open (24 real, 1 bogus).

If `afail` shows no canceled task, the exit raced the cancels (it would then need
a longer `LINGER_SECS`) or I have misread `drop.rs`.

(sha256 of this section's text, printed into the log when the runs started: `e142076d5d2317e7ab178e8b2b9a7d94098e3753fd5e5d631e7a91978bea60b5`)


### Results (second pass; `logs/fix/results.jsonl`, `logs/fix/driver-stdout.log`)

All times are seconds from the launch of the first supervisor (its `BEGIN` line), the stops and completions are the worker's `token-fired` and `completed` lines, `timeline.py`. Every run was made with the same expectations as above; "PASS" is the driver's verdict against them.

**alold: the old serial loop on the fixed library, 15 s leases**

| run | delay (s) | recover starts (s in run) | before: open / working / cancel lines | stops at the worker (n, first to last s) | recover exit (s) | exit after last child (s) | journal completed / canceled / open | tasks completed / canceled | rejected | result |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 3.0 | 23.14 | 50 / 50 / 0 | 49, 30.5 to 31.5 | 38.0 | 0.54 | 1 / 49 / 0 | 1 / 49 | 0 | PASS |
| 2 | 7.0 | 27.09 | 50 / 50 / 0 | 49, 30.1 to 31.5 | 34.1 | 0.59 | 1 / 49 / 0 | 1 / 49 | 0 | PASS |
| 3 | 7.3 | 27.41 | 50 / 50 / 0 | 49, 30.1 to 31.5 | 33.4 | 0.54 | 1 / 49 / 0 | 1 / 49 | 0 | PASS |

Min / median / max of `recover` launch to exit: 33.4 / 34.1 / 38 s; exit after the last child's completion line: 0.54 / 0.54 / 0.59 s. The one child that completed is the first record reattached (`logs/fix/alold-1/recover.log`, `alold-3`). Before the fix, the `al` run at 7.3 s lost all 50.

**alfix: `recover-all` on the fixed library, 15 s leases**

| run | delay (s) | recover-all starts (s in run) | reattach all 50 (ms) | before: open / working / cancel lines | recover exit (s) | exit after last child (s) | journal completed / canceled / open | tasks completed / canceled | worker token-fired / cancel-hook / completed lines | rejected | result |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 3.0 | 23.09 | 202 | 50 / 50 / 0 | 37.5 | 0.04 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 2 | 7.0 | 27.08 | 203 | 50 / 50 / 0 | 33.7 | 0.06 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |
| 3 | 7.3 | 27.4 | 266 | 50 / 50 / 0 | 33.2 | 0.05 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |

Min / median / max of `recover` launch to exit: 33.2 / 33.7 / 37.5 s; exit after the last child's completion line: 0.04 / 0.05 / 0.06 s. Reattaching all 50 took 179 / 203 / 266 ms (min / median / max over every `recover-all` run of this section that reattached 50 live or stopped children).

**allate: `recover-all` after the leases lapsed (delay 12 s)**

| run | delay (s) | recover-all starts (s in run) | stops at the worker (n, first to last s) | before: open / tasks canceled / worker cancel lines | reattach all 50 (ms) | recover exit (s) | journal completed / canceled / open | result |
|---|---|---|---|---|---|---|---|---|
| 1 | 12.0 | 32.08 | 50, 30.5 to 31.4 | 50 / 50 / 100 | 240 | 0.5 | 0 / 50 / 0 | PASS |
| 2 | 12.0 | 32.11 | 50, 30.1 to 31.4 | 50 / 13 / 100 | 260 | 0.5 | 0 / 50 / 0 | PASS |
| 3 | 12.0 | 32.09 | 50, 30.5 to 31.5 | 50 / 50 / 100 | 262 | 0.5 | 0 / 50 / 0 | PASS |

**afix: scenario `a` (no leases) with `recover-all`, one run**

| run | delay (s) | recover-all starts (s in run) | reattach all 50 (ms) | before: open / working / cancel lines | recover exit (s) | exit after last child (s) | journal completed / canceled / open | tasks completed / canceled | worker token-fired / cancel-hook / completed lines | rejected | result |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 5.4 | 25.49 | 185 | 50 / 50 / 0 | 35.3 | 0.04 | 50 / 0 / 0 | 50 / 0 | 0 / 0 / 50 | 0 | PASS |

For comparison, the serial loop in `a` run 1 (same delay 5.4 s, `README.md`): `recover` launch to exit 37.9 s, exit after the last child 2.03 s; here 35.1 s and 0.03 s (`recover_s`, `recover_exit_after_last_child_s`).

**aledge: `recover-all` close to the lapse (extra; 9.5 s, then 10.0, 10.3 and 10.6 s)**

| delay (s) | recover-all starts (s in run) | stops at the worker (n, first to last s) | reattach all 50 (ms) | journal completed / canceled | recover exit (s) | result |
|---|---|---|---|---|---|---|
| 9.5 | 29.59 | 0 | 191 | 50 / 0 | 30.9 | PASS |
| 9.5 | 29.57 | 0 | 242 | 50 / 0 | 31.0 | PASS |
| 9.5 | 29.58 | 0 | 180 | 50 / 0 | 31.0 | PASS |
| 10.0 | 30.09 | 0 | 183 | 50 / 0 | 30.4 | PASS |
| 10.3 | 30.38 | 1, 30.1 to 30.1 | 179 | 49 / 1 | 30.0 | PASS |
| 10.6 | 30.69 | 21, 30.3 to 30.3 | 204 | 29 / 21 | 29.7 | PASS |

The 12 s row of `allate` above is the next point. Nothing was run between 10.6 and 12 s. The driver's PASS for `aledge` only means that no record was left open; it predicts no outcome.

**afail and afailold: one bad record (extra)**

| scenario | run | real records before the bogus one | REATTACH lines (incl. the bogus) | recover exit | canceled right after the exit | final: tasks completed / canceled | final: journal completed / canceled / open | worker cancel-hook lines | result |
|---|---|---|---|---|---|---|---|---|---|
| afail | 1 | 26 | 27 | 1 | 26 | 24 / 26 | 0 / 26 / 25 | 26 | PASS |
| afail | 2 | 26 | 27 | 1 | 26 | 24 / 26 | 0 / 26 / 25 | 26 | PASS |
| afail | 3 | 26 | 27 | 1 | 26 | 24 / 26 | 0 / 26 / 25 | 26 | PASS |
| afailold | 1 | 26 | 27 | 1 | 0 | 50 / 0 | 26 / 0 / 25 | 0 | PASS |

The same `afail` with `--linger-secs 0` (the process exits at once after the error; three runs, `logs/fix/linger0-results.jsonl`; the driver's verdict is FAIL because its prediction is the 3 s one):

| run | real records before the bogus one | REATTACH lines | recover exit | canceled right after the exit | final: tasks completed / canceled | final: journal canceled / open | worker cancel-hook lines |
|---|---|---|---|---|---|---|---|
| 1 | 26 | 27 | 1 | 1 | 49 / 1 | 0 / 51 | 1 |
| 2 | 26 | 27 | 1 | 1 | 49 / 1 | 0 / 51 | 1 |
| 3 | 26 | 27 | 1 | 2 | 48 / 2 | 0 / 51 | 2 |

### What contradicted the expectations

Nothing contradicted the fix: no child was lost where the maintainer expected it to live, in either pass. What contradicted the expectations written before the runs:

1. **The edge probe's prediction was wrong, and the edge is soft and noisy.** I predicted most children saved at 10.0 s, a split at 10.3 s and most lost at 10.6 s. Measured, saved of 50 (journal `completed`), first pass then second pass: 10.0 s 49 and 50; 10.3 s 49 and 49; 10.6 s 46 and 29. 9.5 s saved 50 in 3 of 3 runs of each pass and 12 s saved 0 in 3 of 3. So there is no sharp edge at 10.3 s: the loss grows from nothing at 9.5 s to everything by 12 s, and two passes of the same delay differ (10.6 s: 4 lost, then 21). In each run the stops at the worker came in a group before `recover-all` started (second pass, 10.6 s: 21 stops at +30.3 s, `recover-all` launched at +30.69 s); the children that were saved were renewed after the deadline their last renewal implied (+30.3 s) and before their watcher's next poll. That reading is INFERRED from `InMemoryLeaseStore::renew` (`store/lease/mod.rs`: sets `deadline = now + ttl` for any lease still in the map) and `handler/lease.rs`, not shown by a log of the watcher.
2. **Reattaching 50 is faster than I predicted.** I predicted about 0.5 s (from the 5-child smoke run); the 13 `recover-all` runs took 179 to 266 ms, median 203 (the `reattach all 50` column, `REATTACHED took_ms`), about 4 ms per record. The margin in the 7.3 s run was therefore wider than I wrote: `recover-all` launched at +27.40 s and had reattached all 50 by about +27.67 s, 2.4 s before the earliest stop seen in any scenario (+30.1 s). My model of the cost was off by a factor of two to three; it did not change a verdict.
3. **The book sentence is stricter than what happened.** The new book text says a reattach with a lease "renews at once, but only if the lease has not already lapsed". Against the in-memory lease store, children whose lease had passed its deadline were still saved when the renewal reached the worker before the watcher stopped them (item 1 above, INFERRED). The sentence is safe advice; it understates the window by a few tenths of a second, for this store. `PostgresLeaseStore` was not run.
4. **Small timing differences from the maintainer's text:** `recover-all` started at +23.1, +27.1 and +27.4 s for delays of 3, 7 and 7.3 s (the text said "about 24-28 s") and at +32.1 s for 12 s ("about 33 s"). The leases lapsed, and the worker stopped the children, at +30.1 to +31.5 s ("about 30 s"). The serial loop's exit after the last completion was 0.54 to 0.59 s in the second pass (0.30 to 0.38 s in the first); `recover-all`'s was 0.04 to 0.06 s.
5. **The prediction that held exactly, and the one finding it produced:** `afail` (3 of 3: 26 cancelled, 24 completed, journal 26 canceled and 25 open) and `afailold` (26 completed, 25 open, none cancelled) came out as written. It is a hazard of the book's new loop that the maintainer did not state an expectation for: finding 5 below.
6. **`allate` run 2: the pre-recover snapshot did not show 50 canceled tasks.** I wrote that the snapshot taken just before `recover-all` would show the 50 tasks `canceled`; it did in runs 1 and 3 and in all three first-pass runs, and in run 2 it showed 13, because the worker had logged all 50 stops (100 cancel and token lines) but the task rows were still being written (a pool of 10 connections). The outcome was the same: `recover-all` ended with 50 canceled and 0 open in all three runs.


### Status of the findings

- **Finding 1 (the book's loop loses leased children): fixed in `291aaf89`, VALIDATED live.** The same 15 s leases and restarts that lost 49 or 50 of 50 under the serial loop (`README.md`, `al`, 3 runs) now complete 50 of 50 under `recover-all` at 3, 7 and 7.3 s (`alfix`, 3 of 3), and the serial loop on the fixed library still loses 49 (`alold`, 3 of 3, as the book now says). Side effect, finding 5 below.
- **Finding 2 (the first renewal waits `ttl / 3`): fixed in `291aaf89`, VALIDATED live.** At 7.3 s the serial loop kept its first child (`alold` run 3: 1 completed), where `al` run 2 had lost all 50 at the same delay. What is left is a limit, not a defect: a restart after the worker has noticed the lapse loses the child (`allate`, 12 s: 50 of 50 stopped before `recover-all` started), and the restart may be late by a few tenths of a second (`aledge`, above).
- **Finding 5 (new, from the book's new loop; PROVEN in 3 of 3 runs): one bad record cancels every child reattached before it.** With a record that names a task the worker does not have, in the middle of 50, `recover-all` reattached 26 children, failed on the 27th record, and the 26 dropped handles each sent `CancelTask`: 26 of 26 tasks `canceled` and their records settled `canceled`, 24 never-reattached children left running with their records open. The serial loop on the same journal cancelled none and completed the 26 it reached. The library does what `drop.rs` says (a handle dropped before its child settled cancels the child); the hazard is the shape of the book's loop: it collects handles, and its `?` drops them. Where the process exits at once instead of living on (`--linger-secs 0`), the cancels race the exit: 1, 1 and 2 of 26 reached the worker, the records stayed open, and the children ran on unfollowed. Not fixed; a loop that reattaches with `match` and keeps (or detaches) the handles it has when one record fails would not cancel them.

### Not run in the re-run

- The delays are exactly the set the maintainer asked for (3, 7, 7.3 s for `alold` and `alfix`; 12 s for `allate`; 5.4 s for `afix`), with 3 runs each (`afix`: 1). Added by me: `aledge` at 9.5, 10.0, 10.3 and 10.6 s, `afail`, `afailold`.
- `c`, `c2`, `cx` and `b` were not re-run on `291aaf89`: they do not use a lease, so the diff does not touch them, but that is reasoning, not a run. `a` with the serial `recover` was not re-run either (`README.md` is its evidence; `afix` is `a` with `recover-all`).
- `PostgresLeaseStore`: the worker's lease store is the in-memory one. Its `renew` (`store/lease/postgres.rs:129`) takes the `now_ms` it is given and does not use it (it runs `RENEW_SQL`, which I did not read); whether a late renewal revives a lapsed lease there was not run.
- The unit tests added in `291aaf89` (`renewal_tests.rs`, `delegation_lease_tests.rs`) were not run; neither was any `cargo test`.
- The book's code block as printed was not executed (the book's examples are compiled by `book-tests`, not run). `recover-all` is my transcription of it.
- Other TTLs, other kill phases (the kill was about 4.7 s after the dead supervisor's last renewal, inferred), more than 50 children, a restart between 10.6 s and 12 s, a worker that is restarted too.
- Release builds, more than one worker replica.

### Strongest case against the re-run

1. **One TTL, one kill phase, one worker.** 15 s leases, the kill about 4.7 s after the dead supervisor's last renewal (inferred: the leases lapsed from +30.1 s, so the last renewal was at about +15.1 s; not logged), an in-memory lease store in a worker that stayed up. "Restart within the lease" is measured at one point; the formula behind it (the lapse is `ttl` after the last renewal, and a late renewal still saves a lease the watcher has not yet found) is INFERRED from the stops and from `store/lease/mod.rs` (`renew` sets `deadline = now + ttl` for any lease still in the map), not from a second TTL.
2. **The edge is soft and I mis-predicted it.** I expected a sharp edge near 10.3 s; 10.0, 10.3 and 10.6 s lost 1, 1 and 4 children in the first pass and 0, 1 and 21 in the second, and 12 s lost 50. A reader should not take 10 s as a safe bound: with 15 s leases and this kill phase, 9.5 s lost none in 3 runs of each pass, and the same delay lost different numbers in two passes.
3. **`alfix` passes with a 2 to 3 s margin at 7.3 s and ~0.2 s of reattach.** Reattaching is still one `await` per record: 50 took 179 / 203 / 266 ms here, about 4 ms each. At about 4 ms a record, a 15 s lease is as long as about 3,700 reattaches here, or 300 over a link that costs 50 ms each; the new book text does not warn that the first phase has to fit inside the lease (arithmetic on these numbers, not run).
4. **`recover-all` is my transcription of the book's loop**, with log lines, a record map and a lease option that the printed code does not have (the printed loop gives `reattach_with` no lease). The `afail` result is for that loop's shape (collect, then `?`), which the transcription keeps, not for a loop the book prints with a lease.
5. **`afail` is one fault in one place.** A record naming a task the worker never had stands for a purged task; the real faults (a task purged by retention, a transient connection error in `reattach_with`) were not injected, and the bogus record's position (after 26 of 50) was chosen by me. `LINGER_SECS=3` stands for the rest of a supervisor's life; the number of children cancelled is 26 with it and 1 to 2 without it, so "26" is the loop's reach, not a measure of how a real supervisor fails.
6. **Expectations by the author of the harness, sharpened by earlier runs.** The main expectations were written before the first pass and the second pass reproduced it. The `afail` prediction was written after the first pass and after a 6-child smoke of the scenario, which is why it was right (3 of 3); the edge prediction was written after the first pass too and was wrong.
7. **Three runs per scenario, one for `afix` and `afailold`.** The tables are the whole distribution.
