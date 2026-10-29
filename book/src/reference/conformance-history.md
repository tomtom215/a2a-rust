<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Conformance History

A dated record of conformance runs — the official `a2aproject/a2a-tck` suite,
this repo's own `a2a-tck` runner and the A2A project's second suite, ACTS —
together with a register of every suppression that narrows what those runs
measure.

This is the conformance counterpart to
[Mutation Testing History](./mutation-history.md), and exists for the same
reason: a passing CI job is a notification, not a record. It says *this run*
was green. It does not say how much was measured, what was waived, or when the
number last moved.

To run these suites yourself, see [Conformance Testing](./conformance-testing.md).

## Current status

What the most recent recorded runs show. The pages below have every run and
how each figure was taken.

- **Official `a2aproject/a2a-tck`.** The full profile grades 88 MUST
  requirements and the minimal profile 66, which are the gate's floors. Seven
  requirements fail, on nine (requirement, binding) pairs, and all seven are
  baselined as defects in the suite: six because it grades against a stale
  copy of the specification, one because it demands success where §3.1.1
  requires an error ([W9](./conformance-history/suppressions.md)). Quote
  "85 of 114 passing and 7 failing" across the three profiles; the full
  profile alone is 81 and 7. Latest recorded run: 2026-10-18, both gates
  passed with no regression.
- **ACTS (`a2aproject/a2a-itk`).** Every test passed on 2026-10-08:
  JSON-RPC 101/101, gRPC 88/88, HTTP+JSON 94/94, at every level. Since
  2026-10-09 `acts.yml` runs it on every pull request and push to `main` and
  fails on any failed or errored test.
- **In-repo `a2a-tck`.** 22 checks per binding on all four bindings, plus
  the four §5.1 cross-binding equivalence requirements, including the
  enforcement half of `BIND-EQUIV-004`; gated in `tck.yml`.
- **Suppressions.** No suppression in the register applies to an official
  SDK's agent any more; the ones left are measurement caveats, scoping, jobs
  that are not gates, and the baseline above.

## Pages in this chapter

| Page | What it holds |
|---|---|
| [Official TCK runs](./conformance-history/official-tck.md) | Every run of the official suite since 2026-08-09, with its MUST tallies and per-transport split |
| [ACTS and in-repo TCK runs](./conformance-history/acts-and-in-repo.md) | ACTS results, the in-repo runner's per-binding rows, and cross-binding equivalence |
| [Suppression register](./conformance-history/suppressions.md) | Every mechanism that narrows what a conformance job measures, where it lives and when it can go |
| [Waiver re-verification log](./conformance-history/verification-log.md) | How the reference-SDK waivers were re-measured and, by 2026-10-12, all removed |

## How to add an entry

Run the profiles as documented in
[`docs/official-tck-findings.md` §6](https://github.com/tomtom215/a2a-rust/blob/main/docs/official-tck-findings.md),
then add a row to [Official TCK runs](./conformance-history/official-tck.md) with: the date, the a2a-rust commit, the **upstream a2a-tck
commit** (it floats — see the register, W1), the counts, and the process exit
code. A run whose numbers are unchanged is still worth recording; "still 246"
is signal.

## Deliberate deviations

Places where this SDK knowingly does not do what the specification
recommends. Each one is a **SHOULD**, never a MUST; each gives the evidence
for the choice and the condition under which it would be reversed.

### HTTP+JSON responses are `application/json`, not `application/a2a+json` — resolved 2026-10-08

**Resolved by negotiation.** Since 2026-10-08 HTTP+JSON responses are
negotiated, and since 2026-10-15 the default is the A2A type. `Accept` decides
when it names either JSON type; otherwise a request body labelled
`application/a2a+json` is answered in kind; otherwise a request asking only for
`text/event-stream` gets `application/json`; everything else, including a
request with no `Accept`, gets `application/a2a+json`
(`dispatch::rest::media_type`, on both `RestDispatcher` and the axum adapter;
`rest_and_axum_negotiate_the_response_media_type` pins it). Released a2a-go
v2.6.0 still decodes errors only under `application/json`, but its unary calls
send `Accept: application/json` and its streaming calls send only
`Accept: text/event-stream`, so both keep receiving it: `go_sdk_interop.sh`
passed every check on the change. ACTS then rated HTTP+JSON 94/94, every
level, against a2a-itk `82458cea` (`acts/reports/2026-10-08/`). a2a-go's
`main` already accepts both types in `isJSONErrorContentType`; once a release
carries it, the event-stream-only exception can go. That exception is what
remains of the deviation; it also goes if a2a-go otherwise reads
`application/a2a+json` errors (a report is drafted and held:
`docs/upstream/a2a-go-a2a-json-errors-report.md`), and `go_sdk_interop.sh` is
the check.

The record below is the deviation as it stood from 2026-09-25 to 2026-10-08.

**Specification.** §11.1: "`application/a2a+json` **SHOULD** be used for
requests and responses."

**This SDK.** `RestDispatcher` and the axum adapter label HTTP+JSON
responses, successes and errors, `application/json`. Requests in either
media type are accepted. JSON-RPC is unaffected: §9 specifies
`application/json`.

**Why.** Interoperability with the official Go SDK. a2a-go v2.5.0's HTTP+JSON
client (`internal/rest/rest.go`, `FromRESTError`) decodes an error response
only when its `Content-Type` begins with `application/json`, and otherwise
reports a generic server error. With `application/a2a+json`, a Go client
lost the identity of every HTTP+JSON error — `TaskNotFound`,
`TaskNotCancelable` and the rest. This was measured, not inferred: the change
was made on 2026-09-25 (`dfc69ed2`), `go_sdk_interop.sh` failed five checks
in CI, and it was reverted (`cf2a7e96`). For reference, a2a-go's own server
and the official Rust SDK (a2aproject/a2a-rs) also send `application/json`.

**Effect on conformance scores.** ACTS test `REST-CT-001` (level SHOULD)
fails on HTTP+JSON, and is the only ACTS failure on any binding:
HTTP+JSON 91/92, every MUST passing (a2a-rust `d04d64eb`, a2a-itk
`429945f6`, 2026-09-25; JSON-RPC 101/101 and gRPC 88/88 in the same run). The
official Rust SDK's agent fails the same test for the same reason.

**It does not block.** ACTS rates an agent conformant when every graded MUST
test passes (its specification §12.7; `is_conformant` in a2a-itk's
`scripts/acts_report.py`), and `run_acts.py` exits 0 on a conformant run, so
a failed SHOULD such as this one leaves the exit status green: 0 at
`d04d64eb`, where a run with a failed MUST (SEC-AUTH-002, earlier that day)
exited 1. No workflow here ran ACTS then. Since 2026-10-09 `acts.yml` runs
it on every pull request and push to `main` and fails on any failed or
errored test at any level, so a failed SHOULD such as REST-CT-001 would now
fail the merge; `release.yml` runs it on the tagged commit and attests the
summary onto each `.crate`.

**Reversed when** the widely used clients accept `application/a2a+json`
responses, a2a-go's among them. `rest_and_axum_answer_operations_with_the_same_headers`
pins the current headers, and `go_sdk_interop.sh` would show when the switch
is safe. Not reported upstream by the maintainer's decision (2026-09-25);
since 2026-10-15 a report is drafted and held, to be sent by the maintainer
(`docs/upstream/a2a-go-a2a-json-errors-report.md`).

## Transport coverage

The workspace implements four transports (a fifth, SLIMRPC, lives outside it).
TCK coverage is not uniform across them, and a transport with no conformance
job is a gap, not a pass.

| Transport | Spec | Official TCK | In-repo TCK | Other evidence |
|---|---|---|---|---|
| JSON-RPC | §9 | yes — 73 MUSTs | yes — 21 graded, 1 N/A (since 2026-08-30) | cross-SDK matrix; ACTS (every PR) |
| HTTP+JSON / REST | §11 | yes — 69 MUSTs | yes — 21 graded, 1 N/A | cross-SDK matrix; ACTS (every PR) |
| gRPC | §10 | yes — 53 MUSTs | yes — 20 graded, 2 N/A (since 2026-08-10) | golden wire fixtures vs official Python SDK; ACTS (every PR) |
| WebSocket | §12 *custom binding* | **no** | yes — 21 graded, 1 N/A (since 2026-08-10) | unit/integration tests |
| SLIMRPC (`bindings/a2a-protocol-slimrpc`, outside the workspace) | not in v1.0 | **no** | **no** | the binding's own tests (`ci.yml`'s `slimrpc-binding` job); outside the TCK conformance claim (`README.md`) |

WebSocket is a §12 custom binding, so the official suite has no notion of it —
its absence there is expected, not a defect. Its absence from the in-repo
runner was a genuine coverage gap and is closed as of 2026-08-10. The gRPC leg
is redundancy against the official suite rather than a new blind spot closed,
but it is the only outside-in run of gRPC this repo controls, and it is what
makes §5.1 equivalence gradeable across all four.

The four bindings are not served by one target. `examples/echo-agent`, as
`tck.yml` starts it (no `A2A_GRPC_BIND_ADDR`), answers three (no gRPC) and
`tck/sut` answers four, so the all-bindings and equivalence jobs run against
the SUT. That is deliberate: the example stays an example.

## What "100%, nothing skipped" would actually require

Ranked by what is within this project's control.

1. **Nothing, for MUST-level conformance as the suite can measure it** — as
   written when this list was made, when 88/88 graded MUSTs passed and the
   baseline was empty. **Superseded:** since A2A v1.0.1 rewrote §5.4's error
   table, four MUSTs fail against the suite's stale v1.0.0 copy and are
   baselined (§20 and §21 of `docs/official-tck-findings.md`). Since
   2026-09-26 a fifth, `CORE-SEND-003`, is baselined as a defect in the suite
   (§22); quote "87 of 114 passing and 5 failing", as
   [the 2026-09-26 measurement](./conformance-history/official-tck.md#the-2026-09-26-measurement-on-the-0140-release-pr)
   says.
   Since 2026-10-16 two more are baselined, §11.1's media type graded against
   the same stale copy (§23); quote "85 of 114 passing and 7 failing" across the
   three profiles (the full profile alone is 81 and 7, as `README.md` and
   `ROADMAP.md` quote).
   Corrected 2026-09-24 and 2026-09-26.
2. **~~W6's REST half.~~ Done 2026-10-12** — `a2a-java` 1.4.0.Final accepts
   `application/a2a+json`; the skip went with the pin move, and `tck.yml` sets
   no skip. W5 was removed 2026-08-30.
2.5. **~~`BIND-EQUIV-004`'s enforcement half.~~ Done 2026-08-11** — the last
   in-scope conformance claim this repo had recorded as unmeasured (as of
   2026-08-11; `docs/compliance/spec-ledger.md` lists four `PARTIAL`
   requirements — §3.5.2, §5.7, §7.6.1 and §12.4 — as open items, which are
   partly met rather than unmeasured). A credential-requiring `tck/sut`
   profile now exists and both the rejection and acceptance sweeps are graded
   and gated.
3. **W4 — an upstream TCK defect, filed.** It cannot be closed here without
   patching the harness, which this repo declines to do (§18). Removing it is
   gated on `#193`. (W3 was removed 2026-09-01, when upstream `#226` closed
   `#225`.)
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

Items 1, 3 and 5 mean a literal "100% with nothing skipped" is **not currently
reachable** from inside this repository. Any claim that it has been reached
should be read as a claim that one of those three rows changed upstream.

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

**Still not verified:** `itk.yml`'s opt-in `itk-current-mount` job (W8 — the
upstream ITK's `uv.lock` resolves from a private registry that 401s
unauthenticated). The upstream ITK is also driven by `itk-nightly.yml` (metrics
rather than a gate; W12) and, for ACTS, by `acts.yml` (a gate); this page
records no result from the nightly.
