<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Official TCK runs

Part of [Conformance History](../conformance-history.md). How to run the suite is in [Conformance Testing](../conformance-testing.md#the-official-a2aprojecta2a-tck).

Counts are pytest outcomes. `MUST graded` is the number of MUST-level
requirements that reached an actual verdict (`PASS`/`FAIL`/`ERROR`) — as
opposed to `SKIPPED` or `NOT TESTED`, which mean the requirement was never
exercised. That column is the one that distinguishes a clean run from a run
that measured nothing.

| Date | a2a-rust | a2a-tck | Profile | Passed | Failed | Skipped | Desel. | MUST graded | Exit |
|---|---|---|---|---:|---:|---:|---:|---:|---:|
| 2026-08-09 | `d6d28d8` | `5996b79` | full | 246 | 0 | 19 | 0 | 88 | 0 |
| 2026-08-09 | `d6d28d8` | `5996b79` | minimal | 181 | 0 | 83 | 1 | 66 | 0 |
| 2026-08-09 | `d6d28d8` | `5996b79` | extension (`-k`) | 2 | 0 | 0 | 263 | — | 0 |
| 2026-08-10 | `af7a1f8` | `5996b79` | full | 246 | 0 | 19 | 0 | 88 | 0 |
| 2026-08-10 | `af7a1f8` | `5996b79` | minimal | 181 | 0 | 83 | 1 | 66 | 0 |
| 2026-08-10 | `af7a1f8` | `5996b79` | extension (`-k`) | 2 | 0 | 0 | 263 | 1 | 0 |
| 2026-08-11 | `c008ab0` | `5996b79` | full | 246 | 0 | 19 | 0 | 88 | 0 |
| 2026-08-11 | `c008ab0` | `5996b79` | minimal | 181 | 0 | 83 | 1 | 66 | 0 |
| 2026-08-11 | `c008ab0` | `5996b79` | extension (`-k`) | 2 | 0 | 0 | 263 | 1 | 0 |
| 2026-08-12 | `6ebf821` | `5996b79` | full | 246 | 0 | 19 | 0 | 88 | 0 |
| 2026-08-12 | `6ebf821` | `5996b79` | minimal | 181 | 0 | 83 | 1 | 66 | 0 |
| 2026-08-12 | `6ebf821` | `5996b79` | extension (`-k`) | 2 | 0 | 0 | 263 | 1 | 0 |
| 2026-09-01 | `b6f3afb` | `de6af18` | full | — | — | — | — | 88 | — |
| 2026-09-24 | `e5ee151` | `263b9cf` | full | — | — | — | — | 88 | — |
| 2026-09-24 | `e5ee151` | `263b9cf` | minimal | — | — | — | — | 66 | — |
| 2026-09-26 | `c67accc` | `main` | full | 240 | 7 | 18 | 0 | 88 | — |
| 2026-09-26 | `c67accc` | `main` | minimal | 174 | 8 | 83 | 0 | 66 | — |
| 2026-09-26 | `c67accc` | `main` | extension (`-k`) | 2 | 0 | 0 | 263 | 1 | 0 |
| 2026-10-10 | `f824b03` | `263b9cf` | full | — | — | — | — | 88 | — |
| 2026-10-16 | `d05a7fc` | `263b9cf` | full | — | — | — | — | 88 | — |
| 2026-10-16 | `d05a7fc` | `263b9cf` | minimal | — | — | — | — | 66 | — |
| 2026-10-18 | — | `263b9cf` | full | — | — | — | — | 88 | — |
| 2026-10-18 | — | `263b9cf` | minimal | — | — | — | — | 66 | — |

The 2026-08-10 rows are the first against post-#103 `main`; the 2026-08-09 rows
predate it. Every count is identical, which is the point of recording an
unchanged run. Both gates' exit codes were captured separately from the
suites', so a green gate over a red suite would be visible here rather than
averaged into one column — all six were 0.

The 2026-08-11 rows are the first against post-#104 `main` (`c008ab0`), run
locally rather than in CI, mirroring `official-tck.yml` step for step. All six
exit codes were again captured separately and all six were 0:

```text
EXIT_SUITE_FULL=0       EXIT_GATE_FULL=0
EXIT_SUITE_MINIMAL=0    EXIT_GATE_MINIMAL=0
EXIT_SUITE_EXTENSION=0  EXIT_GATE_EXTENSION=0
```

Every count is unchanged from 2026-08-10 — the third consecutive identical run,
now across three different a2a-rust commits. The gate output is the load-bearing
part, since it is what the counts are being compared against:

```text
  MUST compatibility : 100.0%
  MUST graded        : 88 (floor 88)     full
  MUST graded        : 66 (floor 66)     minimal
  MUST graded        : 1                 extension, --require-pass CORE-CAP-004
```

The MUST tables below were recomputed from this run's own
`reports/compatibility.json` rather than carried forward, including the
per-transport split and the 92-of-114 arithmetic. Both reproduce exactly.

The extension row's `MUST graded` is 1, not the `—` recorded on 2026-08-09.
That is a measurement, not a change in behaviour: the scoped run grades exactly
`CORE-CAP-004`, which is what `--require-pass CORE-CAP-004` asserts. The
earlier `—` meant "not computed", and reading it as "not gradeable" would
understate what that profile exists to do.

`a2a-tck` was re-cloned from floating `main` (W1) and resolved to `5996b79` —
the same commit as 2026-08-09, so these rows are directly comparable and no
upstream drift occurred in that window.

Reported `must_compatibility` on the full profile: **100.0%**.

**That number is computed over tested requirements only, and must not be quoted
without this sentence.** Of the 114 MUST requirements the suite knows about, on
the full profile:

| Status | Count | Meaning |
|---|---:|---|
| `PASS` | 88 | measured and conformant |
| `FAIL` / `ERROR` | 0 | — |
| `SKIPPED` | 5 | not reachable from this card; see below |
| `NOT TESTED` | 21 | **the suite has no test for them at all** |

The 5 skipped on the full profile are `CARD-EXT-002`, `CORE-CAP-001`,
`CORE-CAP-002`, `CORE-CAP-003`, `CORE-CAP-004`. Three of those are graded by
the minimal profile and one by the extension profile — that is what the second
and third profiles exist for. The 21 `NOT TESTED` are not something this SDK
can close; they are enumerated per family in
`docs/official-tck-findings.md` §16.

MUST requirements carrying a per-transport entry on the full profile, split by
whether that entry is an actual verdict. **The unit is MUST requirements** —
`docs/official-tck-findings.md` §1 carries a second per-transport table reading
`jsonrpc` 95/102, which counts individual test results at every level (the
report's own `per_transport` block, built by `a2a-tck`'s
`reporting/aggregator.py:172-187` from `TestResult` objects). The two are not in
conflict and neither is wrong; they answer different questions from the same
`compatibility.json`:

| Transport | Graded (`PASS`) | `SKIPPED` | Entries total |
|---|---:|---:|---:|
| `jsonrpc` | 68 | 5 | 73 |
| `http_json` | 66 | 3 | 69 |
| `grpc` | 52 | 1 | 53 |
| `agent_card` | 5 | 0 | 5 |

There are no `FAIL`/`ERROR` entries, so graded and `PASS` coincide.

Until 2026-08-10 this read "MUST requirements carrying at least one
per-transport **verdict**: `jsonrpc` 73, `http_json` 69, `grpc` 53" — the
totals column above. That wording contradicted this page's own definition four
paragraphs up, where a verdict is `PASS`/`FAIL`/`ERROR` and `SKIPPED` expressly
is not one. The counts were right for what they measured; the noun was wrong,
and it inflated each transport by its skips. Split rather than reworded,
because the totals are still the useful number for "does the suite reach this
transport at all" and the graded column is the useful one for "what did it
actually decide". Neither figure changed — only what they are called.

## The 2026-08-12 re-measurement at `6ebf821`

The 2026-08-11 rows were taken at `c008ab0`, which `git rev-list --count
c008ab0..6ebf821` puts **39 commits** behind `main` — and PR #105 landed in that
window changing 96 files. Nothing in this page was known to be *wrong*; nothing
in it was known to still *hold*. That is the reason for this row, and it is the
only reason a run whose every count is identical is worth recording.

Run locally, mirroring `official-tck.yml` step for step, with all six exit codes
captured separately rather than through a pipe:

```text
EXIT_SUITE_FULL=0       EXIT_GATE_FULL=0
EXIT_SUITE_MINIMAL=0    EXIT_GATE_MINIMAL=0
EXIT_SUITE_EXTENSION=0  EXIT_GATE_EXTENSION=0
```

```text
  MUST compatibility : 100.0%
  MUST graded        : 88 (floor 88)     full
  MUST graded        : 66 (floor 66)     minimal
  MUST graded        : 1                 extension, --require-pass CORE-CAP-004
```

Every count is unchanged from 2026-08-11 — the **fourth** consecutive identical
run, now across four a2a-rust commits.

The MUST tables above were recomputed from this run's own
`reports/compatibility.json`, not carried forward. All of it reproduces
exactly: 114 MUST entries splitting 88 `PASS` / 5 `SKIPPED` / 21 `NOT TESTED` /
0 `FAIL`; the five skips being `CORE-CAP-001`, `CORE-CAP-002`, `CORE-CAP-003`,
`CORE-CAP-004`, `CARD-EXT-002`; and the per-transport split `jsonrpc` 68/5/73,
`http_json` 66/3/69, `grpc` 52/1/53, `agent_card` 5/0/5.

The 92-of-114 arithmetic was likewise re-derived across all three profiles
rather than restated, by reading each of the five full-profile skips out of the
other two profiles' reports:

| Requirement | full | minimal | extension |
|---|---|---|---|
| `CORE-CAP-001` | `SKIPPED` | **`PASS`** | `NOT TESTED` |
| `CORE-CAP-002` | `SKIPPED` | **`PASS`** | `NOT TESTED` |
| `CORE-CAP-003` | `SKIPPED` | **`PASS`** | `NOT TESTED` |
| `CORE-CAP-004` | `SKIPPED` | `SKIPPED` | **`PASS`** |
| `CARD-EXT-002` | `SKIPPED` | `SKIPPED` | `NOT TESTED` |

88 + 3 + 1 = **92 of 114 measurably PASS**, 0 FAILING, `CARD-EXT-002`
structurally inapplicable, 21 `NOT TESTED` upstream.

`a2a-tck` was re-cloned from floating `main` (W1) and again resolved to
`5996b79` — the same commit as every row above it, so all four dates are
directly comparable and no upstream drift has occurred since 2026-08-09.

> **The two figures above are superseded — do not quote `92 … 0 FAILING` as
> current.** They were true at `a2a-tck@5996b79` and are left standing because
> a dated measurement that gets quietly edited stops being a measurement. The
> pin has since moved, and upstream drift *has* occurred since: see the next
> section. The state to quote was **87 of 114 passing and 5 failing** (since
2026-10-16, 85 and 7 across the three profiles; the full profile alone is 81 and 7,
which `README.md` and `ROADMAP.md` quote: §23 of `docs/official-tck-findings.md`),
> measured 2026-09-26 on the 0.14.0 release PR (see "The 2026-09-26
> measurement" below) and recorded then in `tck/conformance-baseline.json`
> (five `known_failures`; seven, nine requirement-transport pairs, since
> 2026-10-16). `README.md`'s Project Status section and `ROADMAP.md`'s
> Conformance section quote the full profile alone: 81 passing and 7 failing.

## The 2026-09-01 re-measurement at `a2a-tck@de6af18`

This is the row the rest of the repository quotes, and it is the first in the
table where the pin moved rather than this SDK: `A2A_TCK_REVISION` floats
(register entry W1), and `a2a-tck` landed ten commits on 2026-08-31. The
2026-08-31 nightly and the 2026-09-01 nightly ran on the **identical** a2a-rust
commit, `b6f3afb`; the first was green and the second was not.

**88 of 114 MUST requirements PASS and 4 FAIL** — so 92 graded across the
three profiles, the same denominator as 2026-08-12 (the full profile's own
`MUST graded` is 88: 84 `PASS`, 4 `FAIL`), with four requirements moved from
the passing column to the failing one. The four are exactly the entries in
`tck/conformance-baseline.json`:

| Requirement | Binding that fails |
|---|---|
| `CORE-CANCEL-002` | `http_json` |
| `GRPC-ERR-002` | `grpc` |
| `HTTP_JSON-STATUS-001` | `*` |
| `STREAM-SUB-003` | `grpc` |

All four are the same cause, and it is not a deviation from the specification:
the suite grades §5.4's error-mapping table against the copy of the
specification it vendors, which its own `specification/version.json` records as
A2A v1.0.0 (2026-03-13), while A2A v1.0.1 (2026-05-28) rewrote six of that
table's nine rows. Each fails on exactly the one binding whose cell the two
copies disagree about and passes on the bindings where they agree. Reported
upstream as [a2aproject/a2a-tck#231](https://github.com/a2aproject/a2a-tck/issues/231);
the evidence is `docs/official-tck-findings.md` §20 and §21. The
22-requirement remainder — 21 `NOT TESTED`, `CARD-EXT-002` structurally
inapplicable — is unchanged.

**Why that row's pytest columns are `—`, and not zeroes.** This page records
pytest outcomes it has a run log for. It has none for 2026-09-01: the sources
that fix this measurement — `tck/conformance-baseline.json`, `README.md`'s
Project Status section, `ROADMAP.md`'s Conformance section and
`docs/official-tck-findings.md` §21 — state the MUST tally and the four
failing requirements, not the `Passed`/`Failed`/`Skipped`/`Deselected` counts
or the six exit codes. A dash is "not recorded here"; writing a plausible
number in its place would be the failure this page exists to prevent. The next
run taken locally, mirroring `official-tck.yml` step for step, should fill
those cells in.

## The 2026-09-24 re-measurement at `a2a-tck@263b9cf`

The two 2026-09-24 rows are taken from committed records, not from a run log
on this page: `docs/handoff-history.md` ("Verification of record, at `e5ee1518`")
gives full profile 88 MUST graded with 4 failing — exactly the baseline —
minimal 66 graded, and the extension profile's `CORE-CAP-004` passing, at
`a2a-tck@263b9cf` (upstream `main`, unchanged since 2026-09-01);
`README.md`'s Project Status section dated the measurement 2026-09-24 and
stated it as "the same result as 2026-09-01 at `de6af18`". The pytest columns
and exit codes are `—` for the same reason as the 2026-09-01 row.

## The 2026-09-26 measurement on the 0.14.0 release PR

Taken from the Official TCK job's log on PR #144
([job 108357365124](https://github.com/tomtom215/a2a-rust/actions/runs/36225060630/job/108357365124)),
which cloned `a2a-tck` at `main` and did not log the commit. The pytest counts
are the log's summary lines. The full and minimal exit codes are `—` because
the full step discards pytest's status (`|| true`) and the minimal step's is
not printed; the extension step's gate passed.

The full profile grades 88 MUSTs: **83 `PASS` and 5 `FAIL`**, so the figure to
quote is 87 of 114 passing and 5 failing. The fifth failure is
`CORE-SEND-003` on all three bindings. It is a defect in the suite, not a
regression here: the requirement declares no `expected_error`, so the harness
demands that a send with an unsupported media type succeed, and 0.14.0's
media-type enforcement (N34) now answers the `ContentTypeNotSupportedError` that
§3.1.1 and the requirement's own title call for. It passed before only because
the SDK accepted any media type. It is baselined; the evidence is
`docs/official-tck-findings.md` §22.

## The measurements of 2026-10-10, 2026-10-16 and 2026-10-18

All three are against `a2a-tck@263b9cf`, which was still upstream `main`'s
newest commit on 2026-10-18. Their pytest columns and exit codes are `—` for
the same reason as 2026-09-01's: the sources (`docs/official-tck-findings.md`
§1, §23 and §24) state the MUST tallies, not the pytest summary lines or the
exit codes. The 2026-10-18 a2a-rust column is `—` too: that run was taken from
a working branch and no commit is recorded.

* **2026-10-10** (CI on `main` at `f824b03`, and a local run of a branch): 83
  `PASS`, 5 `FAIL` (all baselined), 5 `SKIPPED`, 21 `NOT TESTED` of 114; 88
  graded, the gate's floor.
* **2026-10-16** (PR #172 at `d05a7fc`, in CI and locally): the full profile
  81 `PASS` and 7 `FAIL`, the minimal profile 60 `PASS` and 6 `FAIL` of 66. The
  two new failures are `HTTP_JSON-SVC-001` and `HTTP_JSON-ERR-001` (§23).
* **2026-10-18** (run locally): the gate counts failing *pairs*, a requirement
  on one binding. Full: 88 graded, 9 known failing pairs, 9 observed. Minimal:
  66 graded, 9 known, 8 observed, because that profile does not grade
  `STREAM-SUB-003`. Both gates passed with no regression.

2026-10-24 has no row. Findings §25 re-read the seven baselined requirements
statically against upstream `d4d6fae` (three commits after `263b9cf`, changing
only CI, `pyproject.toml` and a task-helper test file) and found no baseline
entry stale; the per-transport outcomes it relies on are the gate's own, in
Official TCK run `40097907886` on `2781bb6`, which succeeded.
