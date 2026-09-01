<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Conformance History

A dated record of TCK conformance runs — the official `a2aproject/a2a-tck`
suite and this repo's own `a2a-tck` runner — together with a register of every
suppression that narrows what those runs measure.

This is the conformance counterpart to
[Mutation Testing History](./mutation-history.md), and exists for the same
reason: a passing CI job is a notification, not a record. It says *this run*
was green. It does not say how much was measured, what was waived, or when the
number last moved.

## Why the suppression register is half the page

A conformance score is only meaningful next to the list of things it did not
ask. This repo has twice shipped a gate that could not fail — the mutation gate
reporting 100% over reports nobody read, and `exclude_re` entries in
`mutants.toml` that were silently ignored. (Since 2026-08-14 the second one
reads worse: the whole file is silently ignored, because cargo-mutants looks for
it at `.cargo/mutants.toml` and it sits at the repository root.) A conformance
suite has the same
failure mode in a nastier form, because *skipping* is a normal, expected
outcome there: the suite legitimately skips what the agent card does not
advertise. "0 failures" and "nothing ran" look identical.

So every row below carries its suite exit code, and every waiver is listed with
where it lives and what would make it removable.

## How to add an entry

Run the profiles as documented in
[`docs/official-tck-findings.md` §6](https://github.com/tomtom215/a2a-rust/blob/main/docs/official-tck-findings.md),
then add a row with: the date, the a2a-rust commit, the **upstream a2a-tck
commit** (it floats — see the register, W1), the counts, and the process exit
code. A run whose numbers are unchanged is still worth recording; "still 246"
is signal.

## Official `a2aproject/a2a-tck`

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

### The 2026-08-12 re-measurement at `6ebf821`

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

## In-repo `a2a-tck` runner

22 conformance checks per binding. `Skipped` counts documented deviations of
the *target* implementation, never of this SDK — see register entries W5/W6.

| Date | Target | Binding | Passed | Failed | N/A | Skipped | Exit |
|---|---|---|---:|---:|---:|---:|---:|
| 2026-08-09 | `examples/echo-agent` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-09 | `examples/echo-agent` | rest | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-09 | `itk/agents/js-sdk` (`@a2a-js/sdk` 1.0.0) | jsonrpc | 20/20 | 0 | 0 | 2 | 0 |
| 2026-08-09 | `itk/agents/js-sdk` (`@a2a-js/sdk` 1.0.0) | rest | 21/21 | 0 | 0 | 1 | 0 |
| 2026-08-10 | `examples/echo-agent` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-10 | `examples/echo-agent` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `examples/echo-agent` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `tck/sut` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-10 | `tck/sut` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `tck/sut` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `tck/sut` | grpc | 20/20 | 0 | 2 | 0 | 0 |
| 2026-08-11 | `tck/sut` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-11 | `tck/sut` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-11 | `tck/sut` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-11 | `tck/sut` | grpc | 20/20 | 0 | 2 | 0 | 0 |
| 2026-08-12 | `tck/sut` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-12 | `tck/sut` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-12 | `tck/sut` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-12 | `tck/sut` | grpc | 20/20 | 0 | 2 | 0 | 0 |

The `N/A` column is new on 2026-08-10 and is not cosmetic. Until then
`jsonrpc_envelope_format` opened with `if binding != "jsonrpc" { return Ok(()) }`,
so the `rest` rows above scored 22/22 while only 21 checks ran. Not-applicable
outcomes are now excluded from the denominator and printed with their reason,
and a run that grades zero checks exits 2 rather than green.

### Cross-binding equivalence (§5.1)

A separate mode — `--equivalence` — because these four requirements are about
the relation *between* bindings and each is trivially satisfied by any one of
them. Requirement texts and IDs are quoted from
`a2aproject/a2a-tck@5996b79` (2026-06-29).

| Date | Target | Bindings compared | Passed | Failed | Exit |
|---|---|---|---:|---:|---:|
| 2026-08-10 | `tck/sut` | JSONRPC, HTTP+JSON, GRPC, WEBSOCKET | 4/4 | 0 | 0 |
| 2026-08-11 | `tck/sut` | JSONRPC, HTTP+JSON, GRPC, WEBSOCKET | 4/4 | 0 | 0 |
| 2026-08-12 | `tck/sut` | JSONRPC, HTTP+JSON, GRPC, WEBSOCKET | 4/4 | 0 | 0 |

**`4/4` counts requirements, not bindings, and the two are independent.** The
"Bindings compared" column is copied from the run's own `Comparing N bindings:`
line, which is a separate number the `4/4` does not constrain: the tool's floor
is **two** bindings (`equivalence.rs:854`, `ifaces.len() < 2` → hard error,
"reporting a pass would mean nothing"), not four. A run against a card
advertising three bindings prints `Comparing 3 bindings` and then `4/4`, and
exits 0.

Measured 2026-08-11, by starting `tck/sut` without `SUT_WS_HOST` so the card
advertises no WebSocket interface: `--equivalence` reported `Comparing 3
bindings` … `Results: 4/4 requirements passed` and **exit 0**. Nothing in the
tool objected.

That is not a hole in CI, because `tck.yml`'s `tck-all-bindings` job asserts
the four-interface card *before* grading, in a step of its own. Proven the same
way on the same day — against the same three-binding SUT that step printed
`card is missing WEBSOCKET` and exited 1. The four-binding claim in this table
rests on that assertion, not on the `4/4`. It is recorded here because reading
`4/4` as "four bindings agreed" is the natural misreading and it would be
wrong.

### `BIND-EQUIV-004`'s enforcement half — closed 2026-08-11

Until 2026-08-11 this section read: *"`BIND-EQUIV-004` is graded structurally
only … Proving each binding enforces those schemes identically needs a target
configured to require credentials, which no job in this repo currently
provides. That half is unmeasured, not passing."*

There is now such a target. `tck/sut` gained a `SUT_PROFILE=secured` profile
whose card declares a bearer scheme and whose handler enforces it with a single
`BearerTokenAuthInterceptor` — one interceptor above the dispatchers, which is
the property being verified: JSON-RPC, HTTP+JSON, gRPC and WebSocket are guarded
by one implementation reading one `CallContext`.

| Date | Target | Bindings | Structural | Enforcement | Exit |
|---|---|---|---|---|---:|
| 2026-08-11 | `tck/sut` (`SUT_PROFILE=secured`) | 4 | PASS | **PASS — both halves** | 0 |
| 2026-08-12 | `tck/sut` (`SUT_PROFILE=secured`) | 4 | PASS | **PASS — both halves** | 0 |

"Both halves" is the load-bearing phrase. The check sweeps twice:

* **without credentials, every binding must refuse** — one binding serving an
  anonymous caller while the others refuse is the asymmetry §5.1 forbids, and
  it is the realistic defect, since a transport that forgets to forward the
  header its authenticator reads looks completely normal until someone tries it;
* **with credentials, every binding must serve** — without this, the check
  passes trivially against a server that is simply broken.

That second sweep earned its place immediately. The first draft of the probe
sent the JSON-RPC method as `tasks/list` where this SDK's name is `ListTasks`;
it authenticated correctly and then failed method dispatch, so JSON-RPC and
WebSocket both reported a refusal and the check declared a binding asymmetry
that did not exist. **On the rejection sweep alone, a probe that can never
succeed is indistinguishable from enforcement working.** Recorded because it is
the same failure shape as the five gates this repo has found that could not
fail, caught this time before it shipped.

Measured the same day, by injection, that the check can fail: run with a
deliberately wrong token it exits 1 and names all four bindings as refusing.
Run against the ordinary `full` profile it correctly declines to grade
enforcement at all and says the card declares no `securityRequirements` —
against an unsecured target the probe could not fail, so it is not run.

A secured run grades `BIND-EQUIV-004` **and nothing else**: `BIND-EQUIV-001..003`
compare answers about a fixture task an anonymous client cannot create there.
Those three are graded by the ordinary unsecured run in the table above.
Scoping, not waiving — the same argument as the official suite's extension
profile, and neither run alone covers §5.1. The pair does.

Both are gated in `tck.yml`'s `tck-all-bindings` job.

One upstream discrepancy, found while reading the requirement definitions:
`a2a-tck`'s backlog ticket `task-28` summarises `BIND-EQUIV-004` as "Streaming
equivalence", while `tck/requirements/interop.py` — the file the suite actually
loads — defines it as the authentication-scheme requirement. This repo follows
`interop.py`.

## Suppression register

Every mechanism currently narrowing what a conformance job measures. A waiver
absent from this table is a bug in this table.

Line numbers are given as `file:line` **as of `c008ab0`**, together with the
YAML key or step name, which is what to search for when the line has moved.
Audited 2026-08-11 by grepping all twelve workflows for `continue-on-error`,
`|| true`, `set +e`, `--skip`, `--deselect`, `-k`, and `if:` guards; the audit
is described below the table. Six of the nine citations were stale at that
point — W1 by 9 lines, W3 by 29, W4 by 31, W5/W6 by 81, W7 by 13 — and are
corrected here. A citation that points at the wrong line is not as bad as a
missing row, but it costs the reader the one thing the row exists to give them.

| # | Where | Mechanism | Scope | Why | Removable when |
|---|---|---|---|---|---|
| W1 | `official-tck.yml:55` (`env: A2A_TCK_REVISION`) | `A2A_TCK_REVISION: main` | every run | Not a waiver but a measurement caveat: the harness **floats**. A green PR can go red on upstream drift, and two rows above are only comparable if the a2a-tck column matches. | n/a — a deliberate trade-off. Pinning is a maintainer decision; see the comment at that line. |
| W2 | `official-tck.yml:142` | `\|\| true` on the full suite | suite exit status only | The differential gate step, not the suite's exit code, is the verdict. | n/a by design. Backed since 2026-08-09 by `--min-graded 88`, without which a zero-measurement run passed this gate. |
| ~~W3~~ | `official-tck.yml` (step *Run the suite against the minimal-capability profile*) | ~~`--deselect …TestRestStreaming::test_streaming_content_type`~~ | ~~1 test, minimal profile only~~ | Upstream harness defect: the HTTP+JSON client calls `.json()` on a streamed response it closed unread, so any conformant server returning non-2xx to `message:stream` trips `httpx.ResponseNotRead`. Diagnosis and standalone repro in `docs/official-tck-findings.md` §17. The requirement it belongs to, `HTTP_JSON-SSE-001`, is graded `PASS` by the full profile. **Removed 2026-09-01.** [`#226`](https://github.com/a2aproject/a2a-tck/pull/226) ("read streamed error body before close so `_extract_error` survives non-2xx") merged upstream on 2026-08-31 as [`38ab89e`](https://github.com/a2aproject/a2a-tck/commit/38ab89e), closing [`#225`](https://github.com/a2aproject/a2a-tck/issues/225). Measured at `a2a-tck@de6af18`: the test now skips cleanly on *"Streaming not supported"*, and the minimal profile grades the same 66 MUST requirements and reports the same failures with the flag as without it. The waiver is closed, not traded. See `docs/official-tck-findings.md` §21. |
| W4 | `official-tck.yml:314` (step *Run the suite against the required-extension profile*) | `-k "TestCapabilityExtensionRequired"` | scopes run to 2 tests | Required-extension enforcement is per-request (spec §3.3.4); the suite does not send `A2A-Extensions` on ordinary positive requests, so an unscoped run against this card fails 72 checks. Scoping, not waiving — every excluded requirement is graded by the full profile. | [`a2aproject/a2a-tck#193`](https://github.com/a2aproject/a2a-tck/issues/193) lands. **Re-verified still OPEN 2026-08-12.** Guarded by `--require-pass CORE-CAP-004`, so an upstream rename fails loudly instead of selecting nothing. |
| W5 | `tck.yml:227-228` (matrix `sdk: js-sdk`), applied at `tck.yml:300,306` | `--skip list_tasks_basic,a2a_media_type_accepted` (jsonrpc), `list_tasks_basic` (rest) | js-sdk leg | Documented `@a2a-js/sdk` 1.0.0 defects, not deviations of this SDK. | Upstream fixes them. Since 2026-08-09 the runner exits 1 on a skipped test that passes, so this cannot rot silently. **Verified still failing 2026-08-09** against `@a2a-js/sdk` 1.0.0, and **2026-08-12 against `@a2a-js/sdk` 1.0.1** — the newest release. See "W5 and W6 re-verified against the newest upstream releases" below: a version bump would not remove this waiver. |
| W6 | `tck.yml:246-247` (matrix `sdk: java-sdk`), applied at `tck.yml:300,306` | `--skip a2a_media_type_accepted` (both bindings) | java-sdk leg | Documented `a2a-java` 1.0.0.CR1 divergence: rejects `application/a2a+json`. Version is pinned exactly in the POM, so the behaviour is stable. | Upstream fixes it. Since 2026-08-09 a skipped test that passes exits 1, as for W5. **Verified still failing 2026-08-10** against `a2a-java` 1.0.0.CR1 — logged `[FAIL] … failed as documented` on both bindings in run `31382900862`; see "Not verified" below. **Re-verified 2026-08-12 against `a2a-java` 1.2.0.Final** — two minor releases past the pinned RC — and it still fails on both bindings. See below. |
| W7 | `tck.yml:94` (step *a2a-inspector card validation*) | `continue-on-error: true` | `a2a-inspector` card validation | Not a conformance gate. The vendored inspector validator hard-requires a top-level `url` field that the v1.0 `AgentCard` no longer has (§13-14) — a fully compliant card must fail it. | `a2aproject/a2a-inspector` updates to v1.0 cards. |
| W8 | `itk.yml:101` | `continue-on-error: true` | opt-in `workflow_dispatch` job only | The upstream ITK resolves dependencies from a private Google Artifact Registry that 401s unauthenticated. The deterministic in-repo `itk-traversal-selftest` is the authoritative gate. | A public ITK lockfile exists. |
| W9 | `tck/conformance-baseline.json` | baselined known failures | 4 (requirement, transport) pairs | **No longer empty.** `GRPC-ERR-002` [`grpc`] and `HTTP_JSON-STATUS-001` [`*`] were added 2026-08-30 (`docs/official-tck-findings.md` §20); `CORE-CANCEL-002` [`http_json`] and `STREAM-SUB-003` [`grpc`] on 2026-09-01 (§21). All four are the same cause: `a2a-tck` grades §5.4 against a **vendored copy of the specification** pinned to A2A **v1.0.0** (per its own `specification/version.json`, and byte-identical to that tag), which A2A's **v1.0.1** release superseded on 2026-05-28, and each of the four fails on exactly the one binding whose cell the two copies disagree about — passing on the bindings where they agree. Not a waiver of this SDK's behaviour: the SDK answers what the published §5.4 says, corroborated by the official Python SDK. | `a2a-tck` refreshes its vendored specification. The gate is differential in both directions, so a baselined check that starts passing fails as a **stale baseline** and forces the entry out. |
| ~~W10~~ | `tck/src/equivalence.rs` (`fn bind_equiv_004`) | ~~`BIND-EQUIV-004` graded **structurally only**~~ | ~~1 of the 4 §5.1 requirements~~ | **Removed 2026-08-11, the same day it was added.** The row was correct when written: the check confirmed the card declares its schemes once with no per-interface override, and did not confirm every binding *enforces* them, because no job provided a target requiring credentials. `SUT_PROFILE=secured` now does, `fn bind_equiv_004_enforcement` grades both the rejection and acceptance sweeps against it, and `tck.yml` gates it. See "`BIND-EQUIV-004`'s enforcement half" above for the run and for the probe defect the acceptance sweep caught. | already clear |
| W11 | `tck/src/runner.rs:338` (`run_test`, `Scope::covers`) | checks outside a binding's scope report `N/A` and leave the denominator | 2 of 22 checks, binding-dependent | Applicability, not waiver: `jsonrpc_envelope_format` has nothing to inspect on §10/§11, and `a2a_media_type_accepted` has no field to carry on §10/§12. Listed because it does narrow what a run measures, and because it was a silent inflation bug until 2026-08-10 (`rest` scored 22/22 while 21 checks ran). Now guarded by three compile-time tests: a scope may not name an unknown binding, may not cover all or none, and may not omit its reason. | n/a — removing it would re-introduce the inflation. The guard is the control. |

### The 2026-08-11 completeness audit

The table claims to be exhaustive, so it is worth saying how that was checked
and what was deliberately left out.

**Method.** All twelve workflows were grepped for `continue-on-error`,
`|| true`, `set +e`, `--skip`, `--deselect`, `-k`, and every `if:` guard, and
each hit was read in context and classified. The runner's own sources were then
read for in-tool narrowing that no workflow grep would reach — which is where
W10 and W11 came from.

**Added: W10 and W11.** Both narrow what a conformance run measures and neither
was in the table. W10 was the more serious omission: it is a MUST requirement
graded on half its definition, and while the run did disclose it on stdout, the
register is where a reader looks for exactly that. **W10 was then closed the
same day** — see "`BIND-EQUIV-004`'s enforcement half" above. Adding it and
retiring it within one session is not churn: writing the row is what made the
gap concrete enough to close, which is the argument for keeping the register
exhaustive even when an entry is expected to be short-lived.

**Deliberately not added.** Three hits are real suppressions that are *not*
conformance suppressions, and adding them would dilute the table's claim rather
than strengthen it:

* `ci.yml:216` — `continue-on-error: true` on the `nightly` job. An
  informational canary against an unpinned nightly toolchain, named
  "Nightly (informational)". It grades no conformance requirement.
* `mutants.yml:315` and `:897` — `set +e` around cargo-mutants invocations, so
  a non-zero exit can be inspected rather than killing the step. Mutation, not
  conformance; that gate's own history is in
  [Mutation Testing History](./mutation-history.md).
* `release.yml:549` — `|| true` on a `cargo yank --undo` in a rollback path.

**Checked and found sound.** `tck.yml:254-272`'s four `if: matrix.sdk == …`
guards select which agent a matrix leg starts. A leg matching none of them
would start no agent — but the "wait for agent" step that follows exits 1 after
30 attempts, so it fails closed rather than grading nothing. The four
`if: always()` guards in `official-tck.yml` widen rather than narrow: they make
the gate steps run even after a red suite. `itk.yml:147`'s `if: failure()` is a
diagnostic upload.

## Transport coverage

The repo implements four transports. TCK coverage is not uniform across them,
and a transport with no conformance job is a gap, not a pass.

| Transport | Spec | Official TCK | In-repo TCK | Other evidence |
|---|---|---|---|---|
| JSON-RPC | §9 | yes — 73 MUSTs | yes — 22 checks, both agent legs | cross-SDK matrix |
| HTTP+JSON / REST | §11 | yes — 69 MUSTs | yes — 21 graded, 1 N/A | cross-SDK matrix |
| gRPC | §10 | yes — 53 MUSTs | yes — 20 graded, 2 N/A (since 2026-08-10) | golden wire fixtures vs official Python SDK |
| WebSocket | §12 *custom binding* | **no** | yes — 21 graded, 1 N/A (since 2026-08-10) | unit/integration tests |

WebSocket is a §12 custom binding, so the official suite has no notion of it —
its absence there is expected, not a defect. Its absence from the in-repo
runner was a genuine coverage gap and is closed as of 2026-08-10. The gRPC leg
is redundancy against the official suite rather than a new blind spot closed,
but it is the only outside-in run of gRPC this repo controls, and it is what
makes §5.1 equivalence gradeable across all four.

The four bindings are not served by one target. `examples/echo-agent` answers
three (no gRPC) and `tck/sut` answers four, so the all-bindings and
equivalence jobs run against the SUT. That is deliberate: the example stays an
example.

## What "100%, nothing skipped" would actually require

Ranked by what is within this project's control.

1. **Nothing, for MUST-level conformance as the suite can measure it.** 88/88
   graded MUSTs pass on the full profile; the baseline is empty. This is done.
2. **W5/W6 (in this project's control only to remove, not to fix).** Both are
   upstream SDK defects. The runner now fails if either starts passing, so they
   cannot linger unnoticed.
2.5. **~~`BIND-EQUIV-004`'s enforcement half.~~ Done 2026-08-11** — the last
   in-scope conformance claim this repo had recorded as unmeasured. A
   credential-requiring `tck/sut` profile now exists and both the rejection and
   acceptance sweeps are graded and gated.
3. **W3 and W4 — upstream TCK defects, both filed.** Neither can be closed here
   without patching the harness, which this repo declines to do (§18). Removing
   them is gated on `#225` and `#193`.
4. **WebSocket and gRPC legs in the in-repo runner.** ~~The only genuinely
   open, in-scope coverage gap in the table above.~~ **Done 2026-08-10** —
   both legs exist and are gated in `tck.yml`.
5. **The 21 `NOT TESTED` MUSTs.** Not closable by this SDK at all — the suite
   has no tests for them. Closing them means contributing tests upstream.
   Four of them (`BIND-EQUIV-001..004`) are now graded *here* by
   `--equivalence`, which does not change the upstream count: the suite still
   reports them `NOT TESTED`, because the tests that would change that have to
   live in `a2aproject/a2a-tck`. What changed is that this repo no longer ships
   a multi-binding server with nothing checking the bindings agree.

Items 3 and 5 mean a literal "100% with nothing skipped" is **not currently
reachable** from inside this repository. Any claim that it has been reached
should be read as a claim that one of those two rows changed upstream.

## Not verified

Recorded so the gaps in this page are as visible as its contents.

**Closed on 2026-08-10.** The 2026-08-09 session did not exercise the
cross-language matrix, the gRPC wire-compatibility job, or the official Python
client job, and this section said so. TCK workflow run
[`31382900862`](https://github.com/tomtom215/a2a-rust/actions/runs/31382900862)
(head `627a285`) has since run all twelve jobs to `success`: the `go`, `java`,
`python`, `javascript`, `go-sdk`, `java-sdk`, `python-sdk` and `js-sdk` legs,
`TCK self-test (echo-agent)`, `TCK all bindings (SUT)`, `gRPC wire
compatibility (official Python SDK)` and `Official Python SDK client vs our
server`.

That run's head is `627a285`, not `af7a1f8`. The two trees differ only in
`book/src/reference/benchmark-dashboard.html` and
`book/src/reference/benchmarks.md`, both generated by the benchmarks workflow;
no source, workflow, proto, SUT or agent file differs. The result therefore
transfers, and this note is here so the reader can check that claim rather than
take it.

**Still not verified:** the ITK jobs (W8 — the upstream ITK resolves from a
private registry that 401s unauthenticated).

### W6 is now measured, not inherited

Worth stating separately, because it was this page's weakest claim.

`--skip` in the in-repo runner does **not** deselect: `runner::run_all` executes
every check and the skip list is applied afterwards, when partitioning results
(`tck/src/main.rs:165-184`). A skipped check that passes is then reported as
`STALE SKIP` and exits 1 (`tck/src/main.rs:271-287`). So a *green* leg carrying
a skip is positive evidence that the skipped check still fails.

Run `31382900862`'s `java-sdk` leg logged, on both bindings:

```text
  [FAIL] a2a_media_type_accepted
  SKIP  a2a_media_type_accepted — failed as documented
```

That is `a2a-java` 1.0.0.CR1 rejecting `application/a2a+json`, executed and
observed on 2026-08-10 — the divergence W6 documents, re-measured rather than
carried forward. The same mechanism covers W5 on the `js-sdk` leg.

A green CI job remains weaker evidence than a locally reproduced run for
anything whose verdict is the job's own exit code. It is *not* weaker for this
particular claim, because the specific line above is the measurement, and it is
in the log either way.

### W5 and W6 re-verified against the newest upstream releases — 2026-08-12

The rows above verify W5 and W6 against the versions this repo *pins*. That
answers "does the documented defect still exist there", but not the question the
"Removable when" column actually asks, which is "has upstream fixed it". Those
diverge the moment upstream ships and the pin does not move — and both had.

| Waiver | Pinned | Available upstream when checked |
|---|---|---|
| W5 | `@a2a-js/sdk` **1.0.0**, exact, via `itk/agents/js-sdk/package-lock.json` (`^1.0.0` in `package.json`) | **1.0.1**, published 2026-07-28 — *before* the 2026-08-09 verification |
| W6 | `a2a-java` **1.0.0.CR1** — a release *candidate* — at `itk/agents/java-sdk/pom.xml:28` | **1.0.0.Final, 1.1.0.Final, 1.2.0.Final**; Maven Central `maven-metadata.xml` gives `<release>1.2.0.Final</release>`, `lastUpdated 20260807124045` |

So the ledger's earlier "still failing" rows were true and also, by themselves,
unable to tell anyone whether the waiver was still *needed*.

**W5 — measured.** Two scratch agents were built from this repo's own
`itk/agents/js-sdk/index.js`, one on 1.0.0 (control) and one on 1.0.1
(candidate), and probed with the wire shape `tck/src/tests/helpers.rs` uses.
Both defects reproduce **identically on 1.0.1**:

```text
a2a_media_type_accepted   1.0.0: FAIL   1.0.1: FAIL
  -32005 Unsupported Content-Type "application/a2a+json"; expected
  application/json.   (CONTENT_TYPE_NOT_SUPPORTED)
list_tasks_basic          1.0.0: FAIL   1.0.1: FAIL
  ListTasks -> {"result":{"pageSize":50}} — the `tasks` key is ABSENT
```

The second confirms the mechanism `tck.yml:261-266` describes, with one detail
worth stating precisely: the response does not carry an *empty* array, it
carries **no array at all**, proto3 JSON having omitted the empty repeated
field. The check fails on "missing `tasks` field", not on a length assertion.

**W6 — measured.** This repo's own `itk/agents/java-sdk` was rebuilt with
`<a2a.sdk.version>1.2.0.Final</a2a.sdk.version>` and run. The divergence
survives two minor releases:

```text
jsonrpc  control (application/json)      PASS — task returned
jsonrpc  probe   (application/a2a+json)  FAIL — HTTP 415
rest     control (application/json)      PASS — task returned
rest     probe   (application/a2a+json)  FAIL — HTTP 415
  {"error":{"code":415,"status":"INVALID_ARGUMENT",
            "message":"Incompatible content types",
            "details":[{"reason":"CONTENT_TYPE_NOT_SUPPORTED", ...}]}}
```

**Both waivers stand, on stronger evidence than before: bumping either pin
would not let a single `--skip` be dropped.** That is the useful result — it
converts "we have not looked" into "we looked and the fix is not upstream yet".

**A probe defect, recorded because it is the failure mode this page exists to
catch.** The first W5 pass omitted the `a2a-version: 1.0` header that
`helpers.rs:479-482` sends on every request, and both agents answered
`VERSION_NOT_SUPPORTED ('0.3')` — which, read carelessly, is two more SDK
defects. It was a defect in the probe. What caught it was the plain
`application/json` **control**, which failed when it had no business failing.
Every probe above therefore carries a control, and the controls are reported
next to the results rather than assumed: a measurement artifact and a real
defect are indistinguishable from the failing line alone.

### 2026-08-30 — the pins moved, and three of the four waivers went with them

The paragraph above ends "the fix is not upstream yet". It stayed true for
eighteen days and then stopped being checked, which is the failure this page
was written to prevent: the register recorded that both pins were behind
upstream and nothing moved them. `.github/workflows/pin-freshness.yml` now
re-resolves every SDK pin on a three-week cadence so this cannot depend on
someone remembering.

All three third-party pins were behind:

| Pin | Was | Now | Result |
|---|---|---|---|
| `@a2a-js/sdk` | 1.0.0 | **1.1.0** | W5 (`list_tasks_basic`) **removed** — fixed upstream |
| `a2a-go/v2` | v2.3.1 | **v2.5.0** | no waiver; 21/21 on both bindings |
| `a2a-java` | 1.0.0.CR1 | **1.3.0.Final** | W6 REST half stands; JSON-RPC half **removed** |

**W6's JSON-RPC half was never a divergence.** Neither `a2a-java` nor
`@a2a-js/sdk` was wrong to reject `application/a2a+json` on JSON-RPC: §9
specifies `Content-Type: application/json`, and §14.1.1 registers the A2A
media type with the note *"This media type is intended for the HTTP+JSON/REST
binding"*. This kit was asserting it on the wrong binding. Both SDKs were
listed here as divergent for as long as that check was mis-scoped. The REST
half is real and still fails at 1.3.0.Final, so it alone is retained.

**Bumping `a2a-java` required a migration, not just a version.** At
1.3.0.Final the agent built and then failed eleven JSON-RPC checks, all
cascading from `SendMessage` answering `TASK_NOT_FOUND`. The cause is
upstream hardening, not a regression: 1.3.0 enforces a fail-closed default for
task authorization, and `DefaultRequestHandler.enforceRead` throws
`TaskNotFoundError` when no `TaskAuthorizationProvider` bean exists and
`authorizationRequired` is set — a denied read is reported as "not found"
rather than leaking that the task exists, which is why a working task store
looked broken. `itk/agents/java-sdk` now produces a permissive provider, which
grants rather than switching the check off, so the authorization path still
executes. Controlled comparison, single variable: same 1.3.0.Final build,
without the bean 11 failures, with it 0.
