<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Suppression register

Part of [Conformance History](../conformance-history.md).

Every mechanism currently narrowing what a conformance job measures. A waiver
absent from this table is a bug in this table.

Line numbers are given as `file:line` **as of `dc4e6207`**, together with the
YAML key or step name, which is what to search for when the line has moved.
Audited 2026-08-11 by grepping all twelve workflows for `continue-on-error`,
`|| true`, `set +e`, `--skip`, `--deselect`, `-k`, and `if:` guards; the audit
is described below the table. Six of the nine citations were stale at that
point — W1 by 9 lines, W3 by 29, W4 by 31, W5/W6 by 81, W7 by 13 — and were
corrected then, against `c008ab0`. Re-checked 2026-09-25 against `dc4e6207`:
every line citation except W1's had moved again, and they are updated here.
That re-check also read the three workflows added since the audit
(`itk-nightly.yml`, `pin-freshness.yml`, `soak.yml`): only `itk-nightly.yml`
adds a row (W12), and `pin-freshness.yml` reads `tck.yml`'s skip list rather
than carrying one of its own. A citation that points at the wrong line is not
as bad as a missing row, but it costs the reader the one thing the row exists to give them.

| # | Where | Mechanism | Scope | Why | Removable when |
|---|---|---|---|---|---|
| W1 | `official-tck.yml:55` (`env: A2A_TCK_REVISION`) | `A2A_TCK_REVISION: main` | every run | Not a waiver but a measurement caveat: the harness **floats**. A green PR can go red on upstream drift, and two rows of [Official TCK runs](./official-tck.md) are only comparable if the a2a-tck column matches. | n/a — a deliberate trade-off. Pinning is a maintainer decision; see the comment at that line. |
| W2 | `official-tck.yml:194` (step *Run the official suite*); the steps *Run the suite against the minimal-capability profile* and *Run the suite against the required-extension profile* (the `case "$suite_status"` block that ends each) | `\|\| true` on the full suite; `case "$suite_status" in 0\|1)` on the minimal and extension suites | suite exit status only: the full step discards all of it, the other two only pytest's 1 (tests failed; 2-5 still fail the step) | The differential gate step, not the suite's exit code, is the verdict. | n/a by design. Backed since 2026-08-09 by `--min-graded 88` (`:243`), without which a zero-measurement run passed this gate. |
| ~~W3~~ | `official-tck.yml` (step *Run the suite against the minimal-capability profile*) | ~~`--deselect …TestRestStreaming::test_streaming_content_type`~~ | ~~1 test, minimal profile only~~ | Upstream harness defect: the HTTP+JSON client calls `.json()` on a streamed response it closed unread, so any conformant server returning non-2xx to `message:stream` trips `httpx.ResponseNotRead`. Diagnosis and standalone repro in `docs/official-tck-findings.md` §17. The requirement it belongs to, `HTTP_JSON-SSE-001`, is graded `PASS` by the full profile. | **Removed 2026-09-01.** [`#226`](https://github.com/a2aproject/a2a-tck/pull/226) ("read streamed error body before close so `_extract_error` survives non-2xx") merged upstream on 2026-08-31 as [`38ab89e`](https://github.com/a2aproject/a2a-tck/commit/38ab89e), closing [`#225`](https://github.com/a2aproject/a2a-tck/issues/225). Measured at `a2a-tck@de6af18`: the test now skips cleanly on *"Streaming not supported"*, and the minimal profile grades the same 66 MUST requirements and reports the same failures with the flag as without it. The waiver is closed, not traded. See `docs/official-tck-findings.md` §21. |
| W4 | `official-tck.yml:404` (step *Run the suite against the required-extension profile*, `:389`) | `-k "TestCapabilityExtensionRequired"` | scopes run to 2 tests | Required-extension enforcement is per-request (spec §3.3.4); the suite does not send `A2A-Extensions` on ordinary positive requests, so an unscoped run against this card fails 72 checks. Scoping, not waiving — every excluded requirement is graded by the full profile. | [`a2aproject/a2a-tck#193`](https://github.com/a2aproject/a2a-tck/issues/193) lands. **Re-verified still OPEN 2026-08-12.** Guarded by `--require-pass CORE-CAP-004` (`:436`), so an upstream rename fails loudly instead of selecting nothing. |
| ~~W5~~ | `tck.yml` js-sdk leg (`tck.yml:264`; the comment at `:274-291` records the removal) | ~~`--skip list_tasks_basic,a2a_media_type_accepted` (jsonrpc), `list_tasks_basic` (rest)~~ | ~~js-sdk leg~~ | Documented `@a2a-js/sdk` 1.0.0 defects, not deviations of this SDK. | **Removed 2026-08-30.** `list_tasks_basic` was fixed in `@a2a-js/sdk` 1.1.0, and the pin moved. `a2a_media_type_accepted` on JSON-RPC was never a divergence: §9 specifies `application/json`, and the check is now REST-only. See [2026-08-30 — the pins moved](./verification-log.md#2026-08-30--the-pins-moved-and-three-of-the-four-waivers-went-with-them). |
| ~~W6~~ | `tck.yml` java-sdk leg (the comment there records the removal) | ~~`--skip a2a_media_type_accepted` (rest only; the JSON-RPC half was removed 2026-08-30)~~ | ~~java-sdk leg, REST binding~~ | Documented `a2a-java` divergence on HTTP+JSON: 1.3.0.Final rejected `application/a2a+json`, which §11.1 says SHOULD be used. | **Removed 2026-10-12.** `a2a-java` 1.4.0.Final accepts it. Pin freshness graded 1.4.0.Final and reported the skip stale (run `38299384752`: 20/20 graded on REST, `STALE SKIP`), the pin moved, and the skip went with it; graded again locally at 1.4.0.Final, 21/21 on both bindings with no skip. See [2026-10-12 — the pins moved again](./verification-log.md#2026-10-12--the-pins-moved-again-and-the-last-waiver-went-with-them). |
| W7 | `tck.yml:96` (step *a2a-inspector card validation*, `:95`) | `continue-on-error: true` | `a2a-inspector` card validation | Not a conformance gate. The vendored inspector validator hard-requires a top-level `url` field that the v1.0 `AgentCard` no longer has (§13-14) — a fully compliant card must fail it. | `a2aproject/a2a-inspector` updates to v1.0 cards. |
| W8 | `itk.yml:103` | `continue-on-error: true` | opt-in `workflow_dispatch` job only | The upstream ITK resolves dependencies from a private Google Artifact Registry that 401s unauthenticated. The deterministic in-repo `itk-traversal-selftest` is the authoritative gate. | A public ITK lockfile exists. |
| W9 | `tck/conformance-baseline.json` | baselined known failures | 7 requirements, 9 (requirement, transport) pairs | **No longer empty.** `CORE-SEND-003` [`grpc`, `http_json`, `jsonrpc`] was added 2026-09-26 (`docs/official-tck-findings.md` §22): the requirement declares no `expected_error`, so the suite demands that an unsupported media type be accepted, and 0.14.0 now refuses it as §3.1.1 requires. Clears when the suite declares the expected error. Four more: `GRPC-ERR-002` [`grpc`] and `HTTP_JSON-STATUS-001` [`*`] were added 2026-08-30 (`docs/official-tck-findings.md` §20); `CORE-CANCEL-002` [`http_json`] and `STREAM-SUB-003` [`grpc`] on 2026-09-01 (§21). Those four are the same cause: `a2a-tck` grades §5.4 against a **vendored copy of the specification** pinned to A2A **v1.0.0** (per its own `specification/version.json`, and byte-identical to that tag), which A2A's **v1.0.1** release superseded on 2026-05-28, and each of the four fails on exactly the one binding whose cell the two copies disagree about — passing on the bindings where they agree. `HTTP_JSON-SVC-001` and `HTTP_JSON-ERR-001` [`*`] were added 2026-10-16 (§23): the same stale copy's §11.1 says `application/json` for HTTP+JSON, where v1.0.1 says `application/a2a+json` SHOULD be used, which this SDK has answered since 2026-10-15; the suite's substring test `"application/json" in content_type` cannot pass it. So six of the seven are the same stale copy (the four §5.4 rows and the two §11.1 rows), and `CORE-SEND-003` is a different defect. Not a waiver of this SDK's behaviour: the SDK answers what the published §5.4 says, corroborated by the official Python SDK. | [`a2aproject/a2a-tck#231`](https://github.com/a2aproject/a2a-tck/issues/231) (filed 2026-09-01) lands, i.e. `a2a-tck` refreshes its vendored specification and the six hand-written `ErrorBinding` constants and test literals with it (upstream #232 and #258 do both; a bare `make spec` does not). `HTTP_JSON-SVC-001` and `HTTP_JSON-ERR-001` need #238 as well, and `CORE-SEND-003` needs #213, #203 and #238 (findings §25). The gate is differential in both directions, so a baselined check that starts passing fails as a **stale baseline** and forces the entry out. |
| ~~W10~~ | `tck/src/equivalence.rs` (`fn bind_equiv_004`) | ~~`BIND-EQUIV-004` graded **structurally only**~~ | ~~1 of the 4 §5.1 requirements~~ | **Removed 2026-08-11, the same day it was added.** The row was correct when written: the check confirmed the card declares its schemes once with no per-interface override, and did not confirm every binding *enforces* them, because no job provided a target requiring credentials. `SUT_PROFILE=secured` now does, `fn bind_equiv_004_enforcement` grades both the rejection and acceptance sweeps against it, and `tck.yml` gates it. See [`BIND-EQUIV-004`'s enforcement half](./acts-and-in-repo.md#bind-equiv-004s-enforcement-half--closed-2026-08-11) for the run and for the probe defect the acceptance sweep caught. | already clear |
| W11 | `tck/src/runner.rs:354` (`run_test` at `:345`, `Scope::covers` at `:36`) | checks outside a binding's scope report `N/A` and leave the denominator | 2 of 22 checks, binding-dependent | Applicability, not waiver: `jsonrpc_envelope_format` has nothing to inspect on §10/§11, and `a2a_media_type_accepted` applies only to §11 (§9 specifies `application/json`; §10/§12 have no field for it). Listed because it does narrow what a run measures, and because it was a silent inflation bug until 2026-08-10 (`rest` scored 22/22 while 21 checks ran). Now guarded by three unit tests (`cargo test`): a scope may not name an unknown binding, may not cover all or none, and may not omit its reason. | n/a — removing it would re-introduce the inflation. The guard is the control. |
| W12 | `itk-nightly.yml:110-113` (step *Summarise the scenario results*); `env: A2A_ITK_REVISION: main` at `:50` | a failing `a2a-itk` scenario does not fail the job — per the workflow's own comment, the shared driver exits 0 on a nightly with failures, and the summary step never fails | nightly `a2a-itk` interop run only | Metrics, not a gate. Results go to the job summary, the `itk-nightly-results` artifact and the rolling `nightly-metrics` prerelease; no gate reads them, so this narrows nothing a gate claims. Listed because a green run of this workflow is not a passing interop result, and because the harness revision floats, as W1's does. | n/a by design — read the job summary, not the job status. |

## Why the register exists

A conformance score is only meaningful next to the list of things it did not
ask. This repo has more than once shipped a gate that could not fail — the mutation gate
reporting 100% over reports nobody read, and `exclude_re` entries in
`mutants.toml` that were silently ignored. (Since 2026-08-14 the second one
reads worse: the whole file is silently ignored, because cargo-mutants looks for
it at `.cargo/mutants.toml` and it sits at the repository root.) A conformance
suite has the same
failure mode in a nastier form, because *skipping* is a normal, expected
outcome there: the suite legitimately skips what the agent card does not
advertise. "0 failures" and "nothing ran" look identical.

So every run row in this chapter carries its suite exit code where one was recorded (`—`
means not recorded), and every waiver is listed with where it lives and what
would make it removable.

## The 2026-08-11 completeness audit

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
same day** — see [`BIND-EQUIV-004`'s enforcement half](./acts-and-in-repo.md#bind-equiv-004s-enforcement-half--closed-2026-08-11). Adding it and
retiring it within one session is not churn: writing the row is what made the
gap concrete enough to close, which is the argument for keeping the register
exhaustive even when an entry is expected to be short-lived.

**Deliberately not added.** Three hits are real suppressions that are *not*
conformance suppressions, and adding them would dilute the table's claim rather
than strengthen it:

* `ci.yml:619` — `continue-on-error: true` on the `nightly` job. An
  informational canary against an unpinned nightly toolchain, named
  "Nightly (informational)". It grades no conformance requirement.
* `mutants.yml:329` and `:1024` — `set +e` around cargo-mutants invocations, so
  a non-zero exit can be inspected rather than killing the step. Mutation, not
  conformance; that gate's own history is in
  [Mutation Testing History](../mutation-history.md).
* `release.yml:674` — `|| true` on a `cargo yank --undo` in a rollback path.

**Checked and found sound.** `tck.yml:326-344`'s four `if: matrix.sdk == …`
guards select which language toolchain a matrix leg installs. A leg matching
none of them would have no toolchain to start its agent with — but the "wait
for agent" step that follows exits 1 after 30 attempts, so it fails closed rather than grading nothing. The two
`if: always()` guards in `official-tck.yml` (the job summary and the report
upload) and its four `if: ${{ !cancelled() && … }}` guards (the minimal and
extension suite steps and their two gate steps) widen rather than narrow: the
gates run even after a red suite, and the minimal and extension suites are
skipped only when the SUT build or the TCK install failed, which has already
turned the job red. `itk.yml:151`'s `if: failure()` is a diagnostic upload.
