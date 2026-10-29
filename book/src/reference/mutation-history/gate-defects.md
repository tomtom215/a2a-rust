<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# How the ledger came to be empty

Part of [Mutation Testing History](../mutation-history.md).

The first row of the [ledger](./sweeps.md#the-ledger) was recorded on 2026-08-07. Every sweep before it produced
no number at all — not "an incomplete number", none — and the reasons are worth
keeping, because each was a gate reporting success over work it had not done.

An earlier revision of this page blamed two cancelled shards for the false
green. That diagnosis was wrong, and the correction matters more than the
original finding, so it is recorded here rather than quietly edited away.

## What the 2026-07-27 run actually did

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

## The actual mechanism

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

## What is fixed

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
* The **shard-completeness gate** remains, and `a2a-server` sharding began at
  12-way on 2026-07-31 (24 on 2026-10-12, 60 on 2026-10-20): they address a
  real second hazard (shards cancelled at the 120-minute timeout), they were
  simply not what made that run green.

**Do not backfill a number from run 30236603180.** The 91% above is recorded
as forensics, not as a measurement of the workspace: two of its shards were
cancelled, so its denominator is short by two of `a2a-server`'s eight shards
(a quarter of that crate, two-elevenths of the jobs).

## A second scheduled run told the same lie

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
start. It was a run that measured 3,630 mutants and reported none of them.

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

## One more gate defect, found by the first working sweep

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
entry (21 when a2a-types and a2a-client were sharded; 69 since 2026-10-20) and
ignores the matrix conclusion. The marker cannot live in an `always()` step —
those still run on cancellation, as run 30236603180 proved by uploading
artifacts from cancelled shards.

The score in the older of the two 2026-08-07 rows of the [ledger](./sweeps.md#the-ledger) was produced by running that
fixed aggregation by hand over run 31193107921's 15 complete reports. The
newer row is CI's own output on the 21-shard matrix — same 183 survivors from
a different partitioning, which is what makes the figure trustworthy rather
than merely produced.

## A seventh gate defect: the PR gate was blind to 41 of 141 source files

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
gate; an over-matched one can balloon a PR run past its timeout). It runs in
`ci.yml`'s `static-checks` job (formerly `fmt`), costs one `git ls-files` call
per crate in the incremental matrix plus one for the intended set, and prints
`N of N tracked sources … are reachable by the PR gate` (141 of 141 when it was
added, 568 of 568 at 0.15.0).

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

**The conclusion stands; half the reasoning did not.** See the next section:
`examine_globs` is not consumed by cargo-mutants' glob crate or by anything
else, because `mutants.toml` is never read. What actually keeps the sweep
pointed at the whole library is `-p <crate>` on the command line, and what
proves it is the `rate_limit.rs` evidence above — which is why that evidence
was worth gathering instead of resting on the semantics.

## An eighth gate defect: `mutants.toml` is not read at all

Found 2026-08-14 while chasing the `EventQueueManager::destroy` timeout in the
2026-08-14 row. cargo-mutants 27.1.0 discovers `.cargo/mutants.toml`; this
repository's file is at the root. Three keys had already been documented as
"silently ignored" — `test_tool`, `profile`, `exclude_re` — each with its own
note. The cause was never per-key.

Two independent proofs, both reproducible in a checkout:

```console
$ cargo mutants -p a2a-protocol-types --all-features --list | grep -c 'src/proto/'
155
$ cargo mutants -p a2a-protocol-types --all-features --list --no-config | grep -c 'src/proto/'
155
```

`exclude_globs` names `crates/*/src/proto/**` and those mutants are listed
anyway — and `--no-config`, which disables the file, changes nothing. Disabling
something that is not being read is a no-op, and that is the tell.

```console
$ cargo mutants --config mutants.toml --list
Error: parse toml from mutants.toml
Caused by: TOML parse error at line 1, column 1
           unknown field `cap_timeout`
```

Pointing at the file explicitly does not run. cargo-mutants 27.1.0 rejects
unknown fields outright, so a *discovered* `mutants.toml` would abort every
sweep rather than configure one. No sweep has ever aborted on it, which is the
second proof.

What this changes about reading the [ledger](./sweeps.md#the-ledger):

* **Generated protobuf code is mutated.** Every score in the ledger includes
  `src/proto/` mutants. They are not excluded and never have been.
* **The per-mutant budget is cargo-mutants' default 5.0x baseline with no cap**,
  not the 3.0x / 300s the config states. Measured the same day:
  `Unmutated baseline in 50s build + 65s test` → `Auto-set test timeout to
  328s`. 328 exceeds the 300s `cap_timeout`, which settles it without needing
  to agree on the multiplier — a cap that was applied could not be exceeded.
  Only the `mutants-incremental-shard` jobs pass `--timeout 300` on the command
  line, so the PR gate is capped per mutant and the full sweep is not (both
  cap the build phase at `--build-timeout 1800`).
* **What does govern a sweep** is the command line in `mutants.yml`. On
  2026-08-14 it was `-p <crate> --output . --shard K/N --jobs 4
  --test-tool=nextest --profile=mutants -- --all-features --run-ignored all`.
  It is now `-p <crate> --output . --build-timeout 1800 --shard K/N
  --sharding round-robin --jobs 4 --test-tool=nextest --profile=mutants
  --all-features -- --run-ignored all -E 'not (binary(soak) or …)'`:
  `--all-features` is given to cargo-mutants itself (since 2026-09-23), and
  `--build-timeout`, `--sharding` and the `-E` test filter were added.
  `.config/nextest.toml` also applies, which nextest reads normally and which
  now supplies the per-test kill described in the 2026-08-14 row.

Not fixed here, deliberately: activating the config changes what the sweep
measures, in both scope and budget, and that belongs on a sweep of its own
rather than at the tail of a release. `ROADMAP.md` carries the plan and the
numbers it would move.
