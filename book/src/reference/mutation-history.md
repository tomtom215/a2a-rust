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

### A seventh gate defect: the PR gate was blind to 41 of 141 source files

Found 2026-08-12 while re-measuring at `6ebf821`. This one is not about the
weekly sweep — it is the **incremental, per-PR** gate in
`mutants.yml`'s `Build PR source diff` step, and it had been silently scoping
out roughly a third of the library.

The step read:

```sh
git diff -M "${BASE}...HEAD" -- 'crates/*/src/**/*.rs' > pr-src.diff
ADDED=$(grep -cE '^\+[^+]' pr-src.diff || true)
if [ "${ADDED:-0}" -eq 0 ]; then echo "skip=true" ...
```

A git pathspec is matched by `fnmatch` **without** `FNM_PATHNAME` unless it
carries `:(glob)` magic, so `*` crosses `/` freely and the `**/` in the middle
collapses to "one or more directories" — the literal slash after it still has to
match something. `crates/*/src/**/*.rs` therefore matched only files in a
*subdirectory* of `src/`, and was blind to every file sitting directly in
`crates/<crate>/src/`.

Measured at `6ebf821` with `git ls-files`, not deduced:

| Pathspec | Files matched | Matches `types/src/method.rs`? |
|---|---:|---|
| `crates/*/src/**/*.rs` (old) | **100** | no |
| `:(glob)crates/*/src/**/*.rs` (new) | **141** | yes |

**41 of 141 tracked sources — 29% — were invisible**, among them
`server/src/rate_limit.rs`, `server/src/serve.rs`, `server/src/builder.rs`,
`server/src/executor.rs`, `types/src/method.rs`, `types/src/signing.rs`,
`client/src/client.rs`, `client/src/retry.rs`, and all four `lib.rs`.
`rate_limit.rs` is one of the eight largest survivor clusters in the table
below, and `serve.rs` is one of the three weakest-covered files in `ROADMAP.md`.

When `ADDED` came out 0 the mutation step was skipped by its own `if:`, the job
went green, and the summary printed **"No Rust source files changed in
`crates/*/src/` — nothing to mutate"**, which was false. That is this page's
oldest theme in a new place: not a gate that failed to fail, but a gate that
declared there was nothing to measure and was believed.

**Proven against real history rather than a synthetic case.** Of the last 120
commits, **nine** changed `crates/*/src` sources *exclusively* in the invisible
set, so each would have produced `ADDED=0` as a PR. Replaying the CI step on
three of them:

| Commit | What it did | `ADDED` old | `ADDED` fixed |
|---|---|---:|---:|
| `a9e1235` | `fix(server): extract the slow path, not the fast one` | 0 | **57** |
| `a116bc5` | `fix(server): collapse the duplicated rate-limit predicate` | 0 | **24** |
| `e6aa9e1` | `feat(types): derive the A2A method set from the spec` | 0 | **376** |

Two behavioural fixes to `rate_limit.rs` and a *feature* commit carrying 376
added source lines, none of it mutated, all three reported as nothing to mutate.

The fix is the `:(glob)` prefix, which gives `**` the "zero or more directories"
meaning readers already assume. It means **"the incremental gate was green" has
been a weaker statement than it looked for every PR touching only top-level
modules.**

**A guard now exists, and it is itself proven able to fail.** Fixing the
pathspec fixes today's tree and nothing else; the next person to edit that line
has the same trap waiting. So `scripts/check_mutation_scope.sh` reads the
pathspec **out of `mutants.yml`** — not a copy, or it could drift from the thing
that runs — and asserts it matches exactly the tracked `.rs` files under
`crates/*/src/`, failing in both directions (a missed file hides code from the
gate; an over-matched one can balloon a PR run past its timeout). It is gate
**40** in `ci.yml`'s `fmt` job, costs two `git ls-files` calls, and prints
`141 of 141 tracked sources … are reachable by the PR gate`.

It is registered in `scripts/prove_gates_fail.sh` as the `mutation_scope`
injection, whose defect is the historical bug restored verbatim — drop the
`:(glob)` prefix — and whose marker is `MUTATION SCOPE GAP`. Verified: with the
prefix removed the check exits 1 and names all 41 files; with it, exits 0.

Worth being precise about why this is a `ci.yml` gate and not a probe in
`scripts/prove_workflow_gates_fail.py`, since that harness owns the other
workflows. That harness proves a step can fail on bad **input**. This step's
input was fine — it read its git range correctly, asked for the wrong files, and
reported success. It also never exits non-zero (it writes `skip=true` to
`$GITHUB_OUTPUT`), so it matched neither `discover()`'s `EXPLICIT_FAIL` regex nor
the curated registry, and **fell through the drift guard in both directions**.
"Can this gate fail?" and "is this gate pointed at everything it claims to
cover?" are different questions. Only the second one catches a pathspec, and
nothing in the repository had been asking it.

**The weekly full sweep is not affected, and that is checked rather than
assumed.** `mutants.toml`'s `examine_globs` carry the same
`crates/…/src/**/*.rs` shape, which is the obvious next place for this bug to
live — but those are consumed by cargo-mutants' glob crate, not by git's
pathspec matcher, and there `**/` does mean "zero or more directories". The
evidence is in the table below rather than in a reading of the semantics: run
`31352927429` reported **6 survivors in `rate_limit.rs`**, a file sitting
directly in `src/`. A sweep blind to top-level files could not have found them.
The full sweep also never passes `--in-diff`, so it takes no scoping from the
PR diff at all. Both halves had to hold, and both do.

## Known equivalent mutants

Mutants that no test can kill, because the mutation does not change observable
behaviour. Each is listed with the argument for its equivalence, so the claim
can be checked rather than taken on trust. Per
[ADR 0006](../../../docs/adr/0006-mutation-testing.md#equivalent-mutants) the
burden is "no test can distinguish it", not "no test occurred to me".

> **Superseded 2026-08-09 — this list is now empty, and nothing is excluded.**
> The two `evict` mutants below were real equivalents, and they were briefly
> excluded by an `--exclude-re` pattern. Both the exclusion and the mutants are
> gone: the equivalence turned out to be a property of the *operator*, not of
> the logic, and rewriting the guard removed it at the source. See
> "How the last equivalents were retired" below. The analysis is kept because
> the reasoning is what makes the retirement checkable.

Neither was marked with `#[mutants::skip]`: that attribute resolves through the
`mutants` crate, which this workspace does not depend on, and adding a regular
dependency to a published crate is a decision to take deliberately rather than
in passing. With nothing left to skip, nothing now rides on that decision.

### How the last equivalents were retired

`evict` computed `store.len() - max` under an `if store.len() > max` guard, at
two sites. Weakening `>` to `>=` differed only at `len == max`, where the mutant
entered with `overflow` of 0, collected, sorted, and then `take(0)` removed
nothing — unkillable by construction.

The earlier note here argued this guard could not simply be deleted the way
`messaging.rs`'s was, because it skips an O(n log n) collect-and-sort and
deleting it would pay that sort on every write sitting exactly at capacity.
That argument was correct, and the fix respects it: the guard is **kept**, and
only its spelling changes.

```rust
let overflow = store.len().saturating_sub(max);
if overflow != 0 { /* collect, sort, evict */ }
```

`saturating_sub` is zero exactly when `len <= max`, so this is the same
predicate and the same short-circuit — the sort is still skipped at or below
capacity. But `!=` mutates only to `==`, which inverts the guard and is caught,
where `>` mutated to an equivalent `>=`.

Measured rather than asserted:

| Check | Before | After |
|---|---:|---:|
| Mutants in `eviction.rs` (`--list`) | 25 | 17 |
| Matches for the old exclusion pattern, crate-wide (2097 mutants) | 2 | 0 |
| Full sweep of `eviction.rs` | — | 17 tested, **17 caught, exit 0** |

The generalisable part: an equivalent mutant is usually a branch guarding a
no-op, or an operator whose weakened form reaches the same state. Both are
often fixable in the source, and deleting the mutant beats teaching the gate to
ignore it.

While it existed it was passed on the command line, not as a `mutants.toml`
`exclude_re` entry. That detail outlives the exclusion and still governs any
future one, because **cargo-mutants 27.1.0 silently ignores that config key** — verified
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

## Per-file measurements

A full workspace sweep needs 21 CI runners; a single 4-core machine projects to
well over a day for `a2a-server` alone (measured: 37 of 2113 mutants in ~40
minutes). Per-file sweeps are the practical unit for burning down a cluster,
and they answer the question a stale survivor list cannot: *is this still true
of `HEAD`?*

All rows below were run on 2026-08-09 against this branch, with a live Postgres
and `--run-ignored all`. Exit codes are quoted because they are the only
reliable signal — `0` all caught, `2` survivors, `4` baseline failed.

| File | Mutants | Caught | Missed | Unviable | Exit | Was |
|---|---:|---:|---:|---:|---:|---:|
| `dispatch/axum_adapter.rs` | 49 | 49 | **0** | 0 | **0** | 7 |
| `dispatch/grpc/service.rs` | 44 | 44 | **0** | 0 | **0** | 9 |
| `streaming/event_queue/in_memory.rs` | 50 | 19 | **0** | 31 | **0** | 7 |
| `agent_card/caching.rs` | 112 | 110 | **0** | 2 | **0** | — |
| `store/task_store/in_memory/eviction.rs` | 17 | 17 | **0** | 0 | **0** | 2 (excluded) |
| `dispatch/websocket.rs` | 33 | 33 | **0** | 0 | **0** | 7 |
| `push/sender.rs` | 99 | 64 | **0** | 35 | **0** | 7 |
| `rate_limit.rs` | 56 | 32 | **0** | 24 | **0** | 5 |
| `handler/event_processing/sync_collector.rs` | 27 | 14 | **0** | 13 | **0** | 9 |
| `a2a-protocol-types` (whole crate) | 674 | 605 | 8 | 61 | 2 | 8 |
| `a2a-protocol-client` (whole crate) | 804 | 357 | 10 | 437 | 2 | 10 |

Across these `a2a-server` files: **57 survivors addressed, none remaining.**
Every one of the nine reports exit 0, and nothing is excluded anywhere in the
project — no `--exclude-re`, no `#[mutants::skip]`, no baselined exception.

**What that sentence does not cover.** Nine files is not the crate.
`a2a-server` has 2113 mutants; these nine account for 487 of them. Treat "zero
survivors" as a claim about the nine rows, not the crate.

**The weekly sweep ran on 2026-08-10** (scheduled 03:33 UTC, `041c3666` on
`main`) and is the current whole-repo figure:

| | Caught | Missed | Timeout | Unviable | Score |
|---|---:|---:|---:|---:|---:|
| All crates, 2026-08-10 | 2187 | 125 | 2 | 1277 | **94%** |

It exited 1, which is the workflow working: 125 survivors, reported rather
than rounded away. Ten of the twelve `a2a-server` shards carried survivors;
shards 3 and 8 came back clean.

**The per-crate split, recovered 2026-08-10** by re-counting run
[31352927429](https://github.com/tomtom215/a2a-rust/actions/runs/31352927429)'s
21 shard artifacts with the workflow's own counting rule (non-empty lines).
This closes the gap the paragraph here previously described — the ledger now
states a whole-crate `a2a-server` score:

| Crate | Caught | Missed | Timeout | Unviable | Score |
|---|---:|---:|---:|---:|---:|
| `a2a-server` | 1225 | 107 | 2 | 779 | **91%** |
| `a2a-client` | 357 | 10 | 0 | 437 | 97% |
| `a2a-types` | 605 | 8 | 0 | 61 | 98% |
| `a2a-sdk` | 0 | 0 | 0 | 0 | n/a — pure re-export facade |
| **combined** | **2187** | **125** | **2** | **1277** | **94%** |

The combined row reproduces the CI figure exactly, which is what makes the
split trustworthy: the same arithmetic that yields 94% yields these four rows.
`a2a-server`'s 2113 total (1225 + 107 + 2 + 779) also matches the count quoted
in "What that sentence does not cover" above.

### The 125 is measured on a commit that predates PR #103

Read the headline with this attached. `041c3666` is **44 commits behind**
`af7a1f8`; the scheduled sweep ran at 03:33 UTC and PR #103 merged later the
same day. So the 125 describes a tree that no longer exists.

Quantified rather than asserted: **61 of the 125 survivors (48%) sit in files
that changed between `041c3666` and `af7a1f8`.**

| Survivors | File (`a2a-server`) | In the per-file table above? |
|---:|---|---|
| 9 | `dispatch/grpc/service.rs` | yes — driven to 0 |
| 9 | `handler/event_processing/sync_collector.rs` | yes — driven to 0 |
| 8 | `handler/event_processing/background/state_machine.rs` | **no** |
| 8 | `streaming/event_queue/in_memory.rs` | yes — driven to 0 |
| 7 | `dispatch/axum_adapter.rs` | yes — driven to 0 |
| 7 | `dispatch/websocket.rs` | yes — driven to 0 |
| 7 | `push/sender.rs` | yes — driven to 0 |
| 6 | `rate_limit.rs` | yes — driven to 0 |

Seven of those eight are files the per-file sweeps above already took to zero
survivors on 2026-08-09 — against the branch that became PR #103, which the
weekly sweep had not yet seen. The counts line up closely with that table's
`Was` column but not exactly (`in_memory.rs` 8 vs 7, `rate_limit.rs` 6 vs 5),
so the two were not measured at the same commit and the overlap must not be
treated as an identity.

**What this does and does not license.** It does not license subtracting 61
and claiming 64: the per-file runs and the sweep are different commits, and
rewritten code generates new mutants as readily as it retires old ones. The
only honest statement is that the current-`main` survivor count is **not
measured**, that 125 is an upper bound carried from a superseded tree, and
that the next scheduled sweep is what settles it. That sweep, not this
paragraph, is the thing to check.

The remaining **64 survivors are in files unchanged since `041c3666`**, so
those are valid against current `main` and are the ones worth triaging today.

#### Re-checked 2026-08-12 at `6ebf821` — and the sweep was NOT re-run

Stated plainly, because the distinction is the whole point of this page: the
mutation sweep was **NOT RUN** in the 2026-08-12 session. Not "unchanged", not
"still 125" — **not measured**. The machine had 4 cores (`nproc`), and a sweep
that needs 21 CI shards does not fit there in a session; running a partial one
and reporting it beside a full one is how denominators move silently. So the
headline row below is still `041c366`'s, and `git rev-list --count
041c366..6ebf821` puts it **91 commits behind** `main` — up from the 44 recorded
above against `af7a1f8`.

What *was* re-measured is the staleness itself, which is cheap and does not need
a sweep:

* All **eight** named survivor-cluster files still changed between `041c366` and
  `6ebf821`, so the "61 of 125 sit in changed files" half holds unchanged.
* The set of changed `crates/*/src` sources grew from **9** files at `af7a1f8`
  to **13** at `6ebf821` — four more files entered it. Since the 64 "still
  valid" survivors were never enumerated per file, it cannot be said which of
  them those four touch. **64 is therefore an upper bound at `6ebf821`, not a
  current count**, and the same refusal to subtract applies as above.

The honest position is unchanged and now further from its evidence: the
current-`main` survivor count is not measured, 125 is an upper bound carried
from a tree 91 commits back, and the next full sweep settles it.

### Triage of the 64 still-valid survivors (2026-08-10)

| Bucket | Count | Disposition |
|---|---:|---|
| Body-size limit boundaries (`> max` at exactly `max`) | 6 | real — one boundary test per call site kills two each |
| Whole-function replacements with no asserted return | ~18 | real — names an untested layer, per the section above |
| `> / >= / ==` and `+ / -` off-by-one arithmetic | ~14 | real, mostly cheap |
| `Debug`/`Display` `fmt` impls replaced | 3 | low value — needs assertions on formatted output |
| `authenticates -> bool` on interceptor impls | 3 | real — no test asserts the flag |
| Logging-only (`warn_unrecognized_params`) | 2 | **equivalent unless log output is asserted**; the function has no effect but a trace event |
| `serve` / process-lifecycle replaced with `Ok(())` | 4 | hard — the servers block; no test asserts they actually serve |
| `days_from_civil` negative-year branch | 2 | **equivalent** — see below |
| remainder | 12 | not individually classified |

**Eight of these were killed on 2026-08-10** (see the commit adding the tests):
three in `signing.rs` canonicalization depth, four in
`handler/capability.rs::activated_extensions`, one in
`parse_iso8601_to_unix_millis`. Each was verified by re-applying the mutant by
hand and confirming the new test goes red — a test written to kill a mutant,
without checking that it does, is the unverified green this page is about.

**Two are classified equivalent rather than fixed.** Both `days_from_civil`
mutants sit on the `y < 0` branch of
`let era = if y >= 0 { y } else { y - 399 } / 400;`. That branch is reachable
only for year 0 with a January/February date, and every input reaching it is
pre-epoch — which `parse_iso8601_to_unix_millis` rejects afterwards via
`(total >= 0).then_some(total)`, under the mutant as much as under the
original. No observable difference through the public API, so there is nothing
to test. Checked, not assumed: both mutants were reasoned through to a
returned `None` either way.

The remaining caution on the combined row stands: it was measured on `main`,
not on any branch in progress.

An earlier revision of this section said the honest statement was "not
measured since 2026-08-03". That was true when written and false within
hours — the Monday sweep it did not account for had already run. A ledger
whose freshness claims are hand-maintained will keep going stale like this;
the run date above is the thing to check, not the prose.

The two whole-crate rows are current and were produced on this branch:
`a2a-protocol-types` on 2026-08-09 (674 mutants, exit 2) and
`a2a-protocol-client` on 2026-08-10 (804 mutants, 37 minutes, exit 2). Both
still carry survivors; they are listed below rather than left implicit.

Both reproduce the 2026-08-03 recovered counts exactly — `a2a-client` 357/10
and `a2a-types` 605/8, matching the table above to the mutant. Two things
follow. The recovered figures were not an artefact of the archaeology: an
independent sweep a week later, on a different machine, agrees. And these
survivors are long-standing rather than newly introduced — neither crate's
tested behaviour has moved in the interval.

### Standing survivors outside `a2a-server`

Recorded so the count is visible rather than inferred from a missing table.
Neither crate has been burned down; these are the live lists as of the sweeps
above.

`a2a-protocol-client` — 10:

| Location | Mutation |
|---|---|
| `error.rs:126` | `parse_retry_after` → `None` |
| `error.rs:126` | `parse_retry_after` → `Some(Default::default())` |
| `token_provider.rs:86` | `*` → `+` |
| `token_provider.rs:136` | `Debug for StaticTokenProvider::fmt` → `Ok(())` |
| `token_provider.rs:174` | `Debug for BearerAuthInterceptor::fmt` → `Ok(())` |
| `token_provider.rs:403` | `<` → `<=` in `OAuth2ClientCredentials::cached` |
| `transport/mod.rs:50` | `*` → `+` |
| `transport/mod.rs:76` | `>` → `==` in `collect_response_limited` |
| `transport/mod.rs:76` | `>` → `>=` in `collect_response_limited` |
| `transport/jsonrpc.rs:303` | `delete !` in `JsonRpcTransport::execute_request` |

`a2a-protocol-types` — 8:

| Location | Mutation |
|---|---|
| `lib.rs:200` | `+` → `-` in `parse_iso8601_to_unix_millis` |
| `lib.rs:254` | `-` → `+` in `days_from_civil` |
| `lib.rs:254` | `-` → `/` in `days_from_civil` |
| `signing.rs:87` | `>` → `==` in `write_canonical` |
| `signing.rs:87` | `>` → `>=` in `write_canonical` |
| `signing.rs:142` | `+` → `*` in `write_canonical` |
| `proto/convert/mod.rs:67` | `Display for ConvertError::fmt` → `Ok(())` |
| `proto/convert/mod.rs:165` | `<` → `<=` in `timestamp_to_rfc3339` |

The last three fell to the same move rather than to cleverer tests: each was a
decision buried behind machinery a test cannot drive, so the decision was moved
somewhere it could be reached.

| Was standing | How it died |
|---|---|
| `push/sender.rs` — scheme-to-port default | Sat behind a DNS lookup whose only successful outcome needs a hostname resolving to a *public* address, so a hermetic test always errored before the port was observable. Extracted as `webhook_port`, a pure mapping. The inverted form pins an https webhook to port 80 and delivers it in cleartext, so it was worth reaching. |
| `rate_limit.rs` — window comparison in the write-lock double-check | Reachable only through a genuine race between callers. Extracted as `admit_or_roll_window`, taking the bucket directly, it is an ordinary state transition; two tests pin both arms. |
| `websocket.rs` — sign of the back-pressure `-32000` | Not the timing test it looks like: the permit is taken before the handler task spawns and released only when it finishes, so an executor that never returns holds its permit for the life of the connection. 65 requests exhaust `Semaphore::new(64)` by construction. |

### Equivalent is not the same as unkillable

The last one to fall, `sync_collector.rs:240`, had been recorded here as a
provably equivalent mutant: the append revert runs only when a store save
fails, both callers of `process_event` propagate with `?`, and the
`CollectState` the revert repairs is dropped without being read. Correct code
and code that reverted *the wrong artifact* were observationally identical.

The proof was sound and the conclusion was too narrow. The revert was
untestable because it was welded to an error path, not because its logic is
unobservable. As a free function — `revert_artifact_append(task, id, len,
meta)` — it is an ordinary transformation with an ordinary assertion, and the
mutant dies.

Two artifacts are what make that test bite, which is the same lesson as the
axum routing tests above: with one artifact `!=` finds nothing and the revert
is a no-op, indistinguishable from correctly reverting an untouched artifact.
With two it truncates the bystander and leaves the intended artifact holding
the failed append.

The revert was **not** deleted, and the contrast with the websocket size guard
is the point. That guard was redundant with a cap enforced by the same constant
one layer up, so deleting it lost nothing. This one is the only thing that
would repair in-memory state if a caller ever handled a `process_event` error
instead of propagating it. Deleting it would have traded a survivor for a
latent correctness hole. "Delete the dead branch" and "extract the buried
decision" are both available; which one applies depends on whether the code
would still be wrong if it ever ran.

### A refactor can manufacture a survivor

Worth recording, because it cost a round trip. Splitting `check` for clippy's
`too_many_lines` originally extracted the read-lock **fast path**. That handed
mutation testing an unkillable target: the fast path is a pure optimization, so
replacing the whole function with `None` still reaches the same decisions
through the slow path, and only lock contention differs. The sweep duly
reported a survivor that had not existed before the refactor.

Extracting the **slow path** instead fixes it — stub that and no bucket is ever
created, so every caller is admitted forever, which the enforcement tests catch
at once. The rule generalises: when splitting a function for length, extract
the half whose absence changes behaviour.

Two of these settle open questions:

* **`a2a-protocol-types` 605/8** matches the 2026-08-03 artifact recount and
  the 2026-08-07 CI figure exactly. Three independent measurements agree, so
  that crate's contribution to the survivor total is confirmed at 8.
* **`agent_card/caching.rs` is at zero**, which the 2026-08-03 artifacts
  (7 survivors) contradict. The artifacts are simply older than the three
  commits that fixed it. This is the trap the survivor list sets: a cluster
  list is only meaningful against the commit it was measured on.

Five of the ten files above now report **exit 0** — every mutant caught, and
no exclusions anywhere. `state_machine.rs` was verified by `--list` only
(12 mutants → 7) without a full sweep.

Three things this burndown is worth remembering for:

* **The same bug wore four hats.** `if len > CAP { len - CAP }` appeared in
  `messaging.rs`, `sync_collector.rs`, `background/state_machine.rs` and
  `eviction.rs`. Found by grepping for the *shape* rather than by waiting for
  a sweep to rediscover it, and fixed with `saturating_sub` — which deletes
  the operators rather than excluding their mutants. `rate_limit.rs` was the
  same defect in a different dress: one predicate written twice, where only
  the copy on the reachable path was ever tested.

* **A passing test is not a killing test.** The first `axum_adapter` routing
  tests asserted 404 for an unknown id — green, and worthless, because every
  mutant routes the request somewhere that also 404s. Seven survivors became
  five, not zero. The fix was to seed the task so a correct parse answers 200
  and each wrong parse answers 404. 404 is the most common answer that router
  gives, which made it the worst possible thing to assert against a routing
  bug.

* **A survivor can mean the code is wrong to exist.** Three `websocket.rs`
  survivors lived in an oversized-message guard that cannot execute:
  tungstenite is configured with the same constant and rejects during the
  read, which `ws_oversized_message_rejected` already proves. It was deleted,
  not tested. Conversely `push/sender.rs:730` *looked* like a latent
  underflow — `max_attempts - 1` on a public unvalidated `usize` — and is not
  one, because the enclosing loop is `for attempt in 0..max_attempts`. Checked
  rather than reported as a bug; its five survivors were a missing
  retry-boundary test, which now measures the backoff on the clock.

## History

~~**Every sweep in this table ran without a database.**~~ **True until the
2026-08-10 row was added on 2026-08-11; that row is the first sweep with one.**
Established 2026-08-09: the `services:` block and `A2A_TEST_POSTGRES_URL` were
added to `mutants.yml` by
[`4b68ac4`](https://github.com/tomtom215/a2a-rust/commit/4b68ac4)
(2026-08-09), which takes the file from 0 to 19 mentions of `postgres`, and
that commit is an ancestor of none of `b416c1a`, `a3c8c0f`, or `803a139`. The
whole Postgres suite is `#[ignore]`d behind that variable, so in each of those
runs it did not execute and its mutants survived for want of a server rather
than for want of a test.

`4b68ac4` **is** an ancestor of `041c366`, and `mutants.yml` at that commit
carries 20 mentions of `postgres`, so the 2026-08-10 sweep ran against a live
server. The effect is visible in its survivors: the Postgres-file cluster is
down to **3**, all in `tenant_postgres_store.rs`. The 12 in `pg_migration.rs`
and 3 in `postgres_store.rs` that every earlier row carries are gone — they
were never test gaps, only missing infrastructure, which is what the paragraph
below predicted and this row confirms.

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
| 2026-08-13 | [`7469fd5`](https://github.com/tomtom215/a2a-rust/commit/7469fd5) | **97%** | 2210 | 57 | 2 | Run [31742334862](https://github.com/tomtom215/a2a-rust/actions/runs/31742334862), `workflow_dispatch` on the 0.8 release branch — **the first sweep of this table measured on something other than `main`**, and the first covering the code 0.8 actually ships. Full CI configuration: `--all-features --run-ignored all`, live PostgreSQL, 21 shards. All 21 completed; 21/21 `COMPLETED` markers and a 57-line `missed.txt` total both verified against the artifacts rather than taken from the summary. 1289 unviable. Per crate: `a2a-server` 40, `a2a-types` 9, `a2a-client` 8. `dispatch/grpc/service.rs` is absent (deleted in 0.8) and `dispatch/grpc/native.rs` stays 0. `streaming/event_queue/manager.rs` and `dispatch/grpc/dispatcher.rs` are **0**, confirming the five killed in `288d703` stay dead under the full feature set — the narrower `--features grpc` run that first verified them was not misleading. **Of the 63 → 57 drop, five are this branch's; the other is variance** — `rest/mod.rs:255` (cancel match arm) and `sse.rs:102` (`send_event`, previously a `TIMEOUT`) both flipped to caught in files the branch does not touch, on async paths where the harness is timing-sensitive. Superseded for `a2a-protocol-types`: the 9 it reports there are addressed in `df6f023` and `05272fa`, which post-date this commit. |
| 2026-08-13 | [`6ebf821`](https://github.com/tomtom215/a2a-rust/commit/6ebf821) | **97%** | 2254 | 63 | 3 | Run [31681284244](https://github.com/tomtom215/a2a-rust/actions/runs/31681284244), `workflow_dispatch` on `main`. All 21 shards completed: 21/21 `COMPLETED` markers present across the uploaded artifacts, and the concatenated `missed.txt` files hold exactly 63 lines — the aggregate matches the shard reports rather than being trusted on the summary's word. 1291 unviable. Wall-clock 103m. **Survivors halved against 2026-08-10, 125 → 63**, and all three clusters the earlier sweeps led with are now zero: `handler/messaging.rs` 17 → 0, `store/task_store/in_memory/eviction.rs` 13 → 0, `dispatch/grpc/native.rs` 11 → 0. Largest remaining are `types/error.rs`, `dispatch/rest/mod.rs` and `dispatch/jsonrpc/response.rs` at 5 each. `dispatch/grpc/service.rs` is **0** here too — see the ROADMAP note: the 9 that 0.8's deletion was expected to retire had already been killed by tests landed since `041c366`, so the deletion retires none. |
| 2026-08-10 | [`041c366`](https://github.com/tomtom215/a2a-rust/commit/041c366) | **94%** | 2187 | 125 | 2 | Run [31352927429](https://github.com/tomtom215/a2a-rust/actions/runs/31352927429), scheduled, on `main`. **The first sweep with a live database** — see the note above; Postgres-file survivors fall 18 → 3. All 21 shards completed and all 21 carry a `COMPLETED` marker. Exit 1, correctly: 125 survivors reported rather than rounded away. 1277 unviable. Per crate: `a2a-server` 1225/107 (91%), `a2a-client` 357/10 (97%), `a2a-types` 605/8 (98%), `a2a-sdk` 0/0. Largest survivor clusters: `handler/event_processing/sync_collector.rs` 9, `dispatch/grpc/service.rs` 9, `streaming/event_queue/in_memory.rs` 8, `handler/event_processing/background/state_machine.rs` 8. **Row added 2026-08-11**, three weeks' worth of readers late — the sweep was described under "Per-file measurements" from the day it ran, but never given a row here, so the canonical dated table still showed 92%/183 as the latest figure. Independently re-derived that day from all 21 shard artifacts with a second implementation of the workflow's counting rule: 2187/125/2/1277 and all four per-crate rows reproduce exactly. |
| 2026-08-07 | [`f54f33e`](https://github.com/tomtom215/a2a-rust/commit/f54f33e) | **92%** | 2168 | 183 | 3 | Run [31209868659](https://github.com/tomtom215/a2a-rust/actions/runs/31209868659) — first sweep aggregated by CI itself rather than by hand, and the first on the 21-shard matrix (a2a-types and a2a-client split 4 ways each). All 21 shards completed; `Require every shard to have completed` passed on the COMPLETED markers while the matrix result was `failure`, which is the case the previous run could not handle. **Identical 183 survivors to the 15-shard run below**, from a completely different partitioning of the mutant set — the one-mutant difference in caught/timeout is timing flake. 1276 unviable. Wall-clock 117m, still set by `a2a-server` shard 2/12 at 116m. |
| 2026-08-07 | [`803a139`](https://github.com/tomtom215/a2a-rust/commit/803a139664f7b9326dc8b90bd91d382ea187f481) | **92%** | 2169 | 183 | 2 | First complete sweep. Run [31193107921](https://github.com/tomtom215/a2a-rust/actions/runs/31193107921), all 15 shards finished. Per crate: `a2a-server` 1207/165 (87%), `a2a-client` 357/10 (97%), `a2a-types` 605/8 (98%), `a2a-sdk` 0/0 (a pure re-export facade — it generates no mutants). 1276 unviable. Largest survivor clusters: `handler/messaging.rs` 17, `store/task_store/in_memory/eviction.rs` 13, `dispatch/grpc/native.rs` 11. Postgres-file survivors fell 18 → 3 once the sweep got a live database. |
| 2026-08-03 | [`a3c8c0f`](https://github.com/tomtom215/a2a-rust/commit/a3c8c0f08f1ba636e4992ea3489bdbae82be271a) | **91%** | 2156 | 196 | 2 | Run [30783745696](https://github.com/tomtom215/a2a-rust/actions/runs/30783745696), scheduled, on `main`. **Recorded retroactively on 2026-08-09** by re-counting the run's own artifacts: CI reported `100%` over `Caught 0 / Missed 0` and concluded `success`, while all 15 shards had in fact completed and their reports were intact. See "A second scheduled run told the same lie" above. Per crate: `a2a-server` 1194/178, `a2a-client` 357/10, `a2a-types` 605/8, `a2a-sdk` 0/0. 1276 unviable — the same count as both 2026-08-07 sweeps. Denominator is sound (no cancelled shards), so unlike run 30236603180 this figure is a measurement rather than forensics. **But it was measured without a database:** `mutants.yml` at `a3c8c0f` contains no `services:` block and no `A2A_TEST_POSTGRES_URL`, so the whole `#[ignore]`d Postgres suite never ran. Exactly 18 of the 196 are Postgres-file survivors that a live database would have killed — `pg_migration.rs` 12, `postgres_store.rs` 3, `tenant_postgres_store.rs` 3. Read the comparable figure as **178 survivors + 18 unmeasured**, not as 196 test gaps. |
