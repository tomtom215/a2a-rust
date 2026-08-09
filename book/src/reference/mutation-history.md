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
(2026-07-27, against `b416c1a`) is the *first* scheduled run in `mutants.yml`'s
history. An earlier revision of this page called it "the only scheduled run",
which was wrong when it was written — a second scheduled run on 2026-08-03 had
already happened and had already reported the same false green. It is analysed
below under "A second scheduled run told the same lie". Its `Mutants Summary`
job concluded `success` and printed:

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

### A second scheduled run told the same lie

*Added 2026-08-09.* The section above missed one, and the omission mattered:
it made a recurring defect look like a single incident.

[Run 30783745696](https://github.com/tomtom215/a2a-rust/actions/runs/30783745696)
(2026-08-03, scheduled, against `a3c8c0f0` on `main`) concluded **`success`** —
workflow and `Mutants Summary` job alike — and printed the identical signature:

```text
COMBINED MUTATION SCORE: 100%
Caught: 0  Missed: 0  Timeout: 0  Unviable: 0
```

All fifteen crate shards concluded `success` too, after doing real work: the
`a2a-types` job alone ran for 1h49m. So this was not a run that failed to
start. It was a run that measured 3,430 mutants and reported none of them.

Its artifacts had not expired, and were re-counted directly on 2026-08-09:

| Crate | Caught | Missed |
|---|---:|---:|
| `a2a-server` (12 shards) | 1194 | 178 |
| `a2a-client` | 357 | 10 |
| `a2a-types` | 605 | 8 |
| `a2a-sdk` | 0 | 0 |
| **Total** | **2156** | **196** |

Plus 2 timeouts and 1276 unviable. A real score of **91%**, reported as 100%.

The mechanism was the same pair of path defects, still visibly present in the
downloaded archives: every report sits at
`home/runner/work/a2a-rust/a2a-rust/mutants.out/mutants.out/…` — the
`--output` double-nesting *and* the least-common-ancestor absolutisation, both
in one path. The fixes listed under "What is fixed" landed between 2026-08-03
and 2026-08-07, which is why the 2026-08-07 sweeps could score themselves.

Two things are worth keeping from this:

* **The 2026-07-27 postmortem was correct but under-scoped.** It diagnosed the
  mechanism precisely and then asserted a fact about frequency — "the only
  scheduled run" — that nobody checked. A correct diagnosis attached to an
  unchecked count is still a claim, and this one was false at the time.
* **Unlike 2026-07-27, this run's denominator is sound.** No shard was
  cancelled and all fifteen reports are complete, so unlike the 91% above,
  this 91% *is* a measurement of the workspace at `a3c8c0f0` and is recorded
  in the History table as such.

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
if and only if cargo-mutants returned; the summary requires one per matrix
entry (21 since a2a-types and a2a-client were sharded) and ignores the matrix
conclusion. The marker cannot live in an `always()` step —
those still run on cancellation, as run 30236603180 proved by uploading
artifacts from cancelled shards.

The score in the older of the two rows below was produced by running that
fixed aggregation by hand over run 31193107921's 15 complete reports. The
newer row is CI's own output on the 21-shard matrix — same 183 survivors from
a different partitioning, which is what makes the figure trustworthy rather
than merely produced.

## Known equivalent mutants

Mutants that no test can kill, because the mutation does not change observable
behaviour. Each is listed with the argument for its equivalence, so the claim
can be checked rather than taken on trust. Per
[ADR 0006](../../../docs/adr/0006-mutation-testing.md#equivalent-mutants) the
burden is "no test can distinguish it", not "no test occurred to me".

Neither is marked with `#[mutants::skip]`: that attribute resolves through the
`mutants` crate, which this workspace does not depend on, and adding a regular
dependency to a published crate is a decision to take deliberately rather than
in passing. That decision is still open — see `ROADMAP.md`.

Since 2026-08-09 they are **excluded from both sweeps** rather than counted as
survivors, because the blocking per-PR gate began failing on one of them: the
`evict` refactor brought its line into a PR diff, and a required check cannot
sit permanently red on a mutant that is unkillable by construction.

The exclusion is a single `--exclude-re` pattern, defined once in
`.github/workflows/mutants.yml` and passed to both the sweep and the
incremental gate. It matches these two mutants and nothing else — measured on
`eviction.rs`, 25 mutants without it and 23 with, so the `==` and `<` mutations
of the very same comparisons stay under test.

It is passed on the command line, not as a `mutants.toml` `exclude_re` entry,
because **cargo-mutants 27.1.0 silently ignores that config key** — verified
with `--list` on 2026-08-09, after a precise pattern added there still produced
the mutant it named. The same version ignores `test_tool` and `profile`
identically. The pre-existing `^tracing::` and `^log::` entries in that file
are therefore inert, which nothing had noticed because neither ever matched a
real mutant. If you add an exclusion, confirm it took effect with
`cargo mutants --file <path> --list | wc -l` before and after; an exclusion you
believe in but that does nothing is the same class of defect as a gate that
cannot fail.

| Location | Mutation | Why no test can kill it |
|---|---|---|
| `store/task_store/in_memory/eviction.rs:100` in `evict` | `store.len() > max` → `>=` | They differ only at `len == max`, where the mutant enters the capacity branch with `overflow == 0`. It collects and sorts the terminal tasks, then `take(0)` removes none, and the fallback's own `len > max` is false. It burns a sort and changes nothing observable. |
| `store/task_store/in_memory/eviction.rs:123` in `evict` | `store.len() > max` → `>=` | The same at the fallback guard: `len == max` gives `remaining == 0` and `take(0)`. |

Both are the same shape: **a branch whose only purpose is to skip work, whose
two arms coincide at the boundary the comparison tests.** That pattern is worth
recognising, because it will recur wherever a hot path avoids work, and because
it is genuinely different from an untested branch.

### Two of these were retired by deleting the branch, not by testing harder

This table held four rows until 2026-08-08. The two that left were both in
`handler/messaging.rs`, and neither was killed — the code that generated them
was rewritten so the mutation no longer exists:

| Was | Now |
|---|---|
| `shape_response_history` — `msgs.len() > n` → `>=` | `helpers::truncate_history`, branchless via `saturating_sub` |
| `send_message_inner` — `history.len() > MAX` → `>=` | the guard deleted; `drain(..excess)` runs unconditionally |

The reasoning is the same in both cases and generalises. A guard of the form
`if len > n { trim_by(len - n) }` is unkillable at `len == n` *because the body
does nothing there*. Write the amount first — `let excess =
len.saturating_sub(n)` — and the branch has nothing left to decide. `drain(..0)`
is genuinely free: `Drain::drop` skips its memmove when the tail does not move,
so this is O(1) rather than an O(n) shift.

**This does not generalise to every guard, and the two survivors above are
where it stops.** `evict`'s guard skips an O(n log n) collect-and-sort, not a
no-op. Deleting it to retire a mutation-testing artefact would put a sort on
every write that is exactly at capacity — paying real cycles to improve a
score, which is the wrong trade and the reason those two rows remain.

The rule worth carrying forward: **when the guarded work is a no-op at the
boundary, delete the guard; when it is expensive, keep it and record the
equivalence.** The mutant is a symptom either way — sometimes of redundant
code, sometimes of a deliberate optimisation.

It is not a reason to relax the target, and the equivalence rate is nowhere
near uniform. `messaging.rs` went from 17 survivors to **0**, `eviction.rs`
from 13 to 2, `dispatch/grpc/native.rs` from 11 to **0**, and
`handler/lifecycle/list_tasks.rs` from 10 to **0**. `native.rs` produced no
equivalents at all because its survivors were not boundary conditions: ten of
its eleven were whole-method replacements, and a method survives being replaced
by `Ok(Response::new(Default::default()))` only when nothing calls it.

Of the 51 survivors across those four files, 49 were ordinary test gaps or
removable branches. Two remain, both in `eviction.rs`, both for the deliberate
reason given below.

### Whole-method survivors name an untested layer, not a missing edge case

`dispatch/grpc/native.rs` is worth separating from the other two files. Its
survivors were not `>` versus `>=`; they were ten of the eleven methods of the
`A2aService` trait impl, each replaceable in its entirety by an empty `Ok`.

`grpc_dispatch_tests.rs` did exist and did pass. It covers `GrpcConfig`'s
builder, `into_service`, and binding a listener — it never issues an RPC. The
gRPC binding had test files, test names, and no test that called it, which is
exactly the state a line-coverage number is worst at revealing and a mutation
score is best at.

Two things made the difference when killing them:

* **Assert on content, not on `is_ok()`.** The mutation returns a *successful*
  empty response, so `assert!(result.is_ok())` passes against a method whose
  body has been deleted. Every test asserts a field that a default cannot
  carry — a task id, the configured card's name, the registered URL.
* **Some methods can only be caught by their side effect.** Both mutations of
  `delete_task_push_notification_config` return exactly the `Ok(Response::new(()))`
  the real method returns on success; the response cannot distinguish them at
  all. Only deleting a config and then listing it can.

### Redundant code can be what makes a mutant killable

Worth recording because it is counter-intuitive and it cost a wrong claim.

`eviction.rs` originally guarded its fallback with
`if removed < overflow && store.len() > max`, where `removed` counted the
terminal tasks actually evicted. Four of the file's thirteen survivors were on
that counter, and they survive for a provable reason: every evicted id comes
from `store.entries`, so every removal lands, and `len` is `max + overflow` on
entry — which makes `removed < overflow` and `store.len() > max` the *same
predicate*. The counter was dead weight, so it was deleted.

Deleting it was behaviour-preserving, and it did remove those four mutants. It
also turned a *caught* mutant into a missed one:
`overflow = store.len() - max` → `len / max` had been killed by a test that
only asserted the store's final size. The `&&` short-circuit was what made that
work — with a too-small overflow, `removed == overflow` switched the fallback
off and the store was left above the cap. Without the counter the fallback
re-checks the size and quietly mops up the difference, so the store lands at
the cap either way and a size assertion cannot see the bug.

The mutant was still killable, just not by that test: a too-small overflow
evicts one fewer terminal task and then lets the fallback take the oldest entry
*overall*, which can be an in-flight task that should have been spared. The
fix was a test that pins which tasks survive rather than how many
(`evict_prefers_terminal_tasks_over_an_older_in_flight_one`), and it covers a
policy — terminal-first eviction — that nothing had tested.

Two things generalise. **Redundant logic can carry mutation-detection strength
that the non-redundant version does not**, so simplifying can lower the score
without changing behaviour. And **an assertion on a quantity is weaker than an
assertion on identity** whenever a later stage can compensate for an earlier
stage's arithmetic. Neither is an argument against the simplification — the
file ended simpler *and* better tested — but both are arguments for re-running
the sweep after a refactor rather than assuming the score can only improve.

## History

**Every sweep in this table ran without a database.** Established 2026-08-09:
the `services:` block and `A2A_TEST_POSTGRES_URL` were added to `mutants.yml`
by [`4b68ac4`](https://github.com/tomtom215/a2a-rust/commit/4b68ac4)
(2026-08-09), which takes the file from 0 to 19 mentions of `postgres`, and
that commit is an ancestor of none of `b416c1a`, `a3c8c0f`, or `803a139`. The
whole Postgres suite is `#[ignore]`d behind that variable, so in each of those
runs it did not execute and its mutants survived for want of a server rather
than for want of a test.

Measured on the 2026-08-03 artifacts, that accounts for exactly **18**
survivors: `pg_migration.rs` 12, `postgres_store.rs` 3,
`tenant_postgres_store.rs` 3. `pg_migration.rs` in particular is not a test
gap — `migrations_apply_in_order_and_are_idempotent` has covered it since
2026-06-10 and pins the `pending_migrations` boundary explicitly.

One consequence for the row below: its note that "Postgres-file survivors fell
18 → 3 once the sweep got a live database" cannot describe run 31193107921,
because `803a139` has no database wiring at all, and its 18 is exactly the
no-database count. The `f54f33e` row could not be checked the same way — that
commit is not reachable in a fresh clone of `main`, so whether the second
2026-08-07 run had a server is **unverified here**, and the 3 is left
unattributed rather than reassigned on a guess.

| Date | Commit | Overall Score | Caught | Missed | Timeout | Notes |
|------|--------|---------------|-------:|-------:|--------:|-------|
| 2026-08-07 | [`f54f33e`](https://github.com/tomtom215/a2a-rust/commit/f54f33e) | **92%** | 2168 | 183 | 3 | Run [31209868659](https://github.com/tomtom215/a2a-rust/actions/runs/31209868659) — first sweep aggregated by CI itself rather than by hand, and the first on the 21-shard matrix (a2a-types and a2a-client split 4 ways each). All 21 shards completed; `Require every shard to have completed` passed on the COMPLETED markers while the matrix result was `failure`, which is the case the previous run could not handle. **Identical 183 survivors to the 15-shard run below**, from a completely different partitioning of the mutant set — the one-mutant difference in caught/timeout is timing flake. 1276 unviable. Wall-clock 117m, still set by `a2a-server` shard 2/12 at 116m. |
| 2026-08-07 | [`803a139`](https://github.com/tomtom215/a2a-rust/commit/803a139664f7b9326dc8b90bd91d382ea187f481) | **92%** | 2169 | 183 | 2 | First complete sweep. Run [31193107921](https://github.com/tomtom215/a2a-rust/actions/runs/31193107921), all 15 shards finished. Per crate: `a2a-server` 1207/165 (87%), `a2a-client` 357/10 (97%), `a2a-types` 605/8 (98%), `a2a-sdk` 0/0 (a pure re-export facade — it generates no mutants). 1276 unviable. Largest survivor clusters: `handler/messaging.rs` 17, `store/task_store/in_memory/eviction.rs` 13, `dispatch/grpc/native.rs` 11. Postgres-file survivors fell 18 → 3 once the sweep got a live database. |
| 2026-08-03 | [`a3c8c0f`](https://github.com/tomtom215/a2a-rust/commit/a3c8c0f08f1ba636e4992ea3489bdbae82be271a) | **91%** | 2156 | 196 | 2 | Run [30783745696](https://github.com/tomtom215/a2a-rust/actions/runs/30783745696), scheduled, on `main`. **Recorded retroactively on 2026-08-09** by re-counting the run's own artifacts: CI reported `100%` over `Caught 0 / Missed 0` and concluded `success`, while all 15 shards had in fact completed and their reports were intact. See "A second scheduled run told the same lie" above. Per crate: `a2a-server` 1194/178, `a2a-client` 357/10, `a2a-types` 605/8, `a2a-sdk` 0/0. 1276 unviable — the same count as both 2026-08-07 sweeps. Denominator is sound (no cancelled shards), so unlike run 30236603180 this figure is a measurement rather than forensics. **But it was measured without a database:** `mutants.yml` at `a3c8c0f` contains no `services:` block and no `A2A_TEST_POSTGRES_URL`, so the whole `#[ignore]`d Postgres suite never ran. Exactly 18 of the 196 are Postgres-file survivors that a live database would have killed — `pg_migration.rs` 12, `postgres_store.rs` 3, `tenant_postgres_store.rs` 3. Read the comparable figure as **178 survivors + 18 unmeasured**, not as 196 test gaps. |
