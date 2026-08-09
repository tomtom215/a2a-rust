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
`mutants.toml` that were silently ignored. A conformance suite has the same
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

MUST requirements carrying at least one per-transport verdict on the full
profile: `jsonrpc` 73, `http_json` 69, `grpc` 53, `agent_card` 5.

## In-repo `a2a-tck` runner

22 conformance checks per binding. `Skipped` counts documented deviations of
the *target* implementation, never of this SDK — see register entries W5/W6.

| Date | Target | Binding | Passed | Failed | Skipped | Exit |
|---|---|---|---:|---:|---:|---:|
| 2026-08-09 | `examples/echo-agent` | jsonrpc | 22/22 | 0 | 0 | 0 |
| 2026-08-09 | `examples/echo-agent` | rest | 22/22 | 0 | 0 | 0 |
| 2026-08-09 | `itk/agents/js-sdk` (`@a2a-js/sdk` 1.0.0) | jsonrpc | 20/20 | 0 | 2 | 0 |
| 2026-08-09 | `itk/agents/js-sdk` (`@a2a-js/sdk` 1.0.0) | rest | 21/21 | 0 | 1 | 0 |

## Suppression register

Every mechanism currently narrowing what a conformance job measures. A waiver
absent from this table is a bug in this table.

| # | Where | Mechanism | Scope | Why | Removable when |
|---|---|---|---|---|---|
| W1 | `official-tck.yml:46` | `A2A_TCK_REVISION: main` | every run | Not a waiver but a measurement caveat: the harness **floats**. A green PR can go red on upstream drift, and two rows above are only comparable if the a2a-tck column matches. | n/a — a deliberate trade-off. Pinning is a maintainer decision; see the comment at that line. |
| W2 | `official-tck.yml:142` | `\|\| true` on the full suite | suite exit status only | The differential gate step, not the suite's exit code, is the verdict. | n/a by design. Backed since 2026-08-09 by `--min-graded 88`, without which a zero-measurement run passed this gate. |
| W3 | `official-tck.yml:232` | `--deselect …TestRestStreaming::test_streaming_content_type` | 1 test, minimal profile only | Upstream harness defect: the HTTP+JSON client calls `.json()` on a streamed response it closed unread, so any conformant server returning non-2xx to `message:stream` trips `httpx.ResponseNotRead`. Diagnosis and standalone repro in `docs/official-tck-findings.md` §17. The requirement it belongs to, `HTTP_JSON-SSE-001`, is graded `PASS` by the full profile. | [`a2aproject/a2a-tck#225`](https://github.com/a2aproject/a2a-tck/issues/225) lands. **Verified still OPEN 2026-08-09.** |
| W4 | `official-tck.yml:283` | `-k "TestCapabilityExtensionRequired"` | scopes run to 2 tests | Required-extension enforcement is per-request (spec §3.3.4); the suite does not send `A2A-Extensions` on ordinary positive requests, so an unscoped run against this card fails 72 checks. Scoping, not waiving — every excluded requirement is graded by the full profile. | [`a2aproject/a2a-tck#193`](https://github.com/a2aproject/a2a-tck/issues/193) lands. Guarded by `--require-pass CORE-CAP-004`, so an upstream rename fails loudly instead of selecting nothing. |
| W5 | `tck.yml:146-147` | `--skip list_tasks_basic,a2a_media_type_accepted` (jsonrpc), `list_tasks_basic` (rest) | js-sdk leg | Documented `@a2a-js/sdk` 1.0.0 defects, not deviations of this SDK. | Upstream fixes them. Since 2026-08-09 the runner exits 1 on a skipped test that passes, so this cannot rot silently. **Verified still failing 2026-08-09** against `@a2a-js/sdk` 1.0.0. |
| W6 | `tck.yml:165-166` | `--skip a2a_media_type_accepted` (both bindings) | java-sdk leg | Documented `a2a-java` 1.0.0.CR1 divergence: rejects `application/a2a+json`. Version is pinned exactly in the POM, so the behaviour is stable. | Upstream fixes it. **Not re-verified in the 2026-08-09 session** — see "Not verified" below. |
| W7 | `tck.yml:81` | `continue-on-error: true` | `a2a-inspector` card validation | Not a conformance gate. The vendored inspector validator hard-requires a top-level `url` field that the v1.0 `AgentCard` no longer has (§13-14) — a fully compliant card must fail it. | `a2aproject/a2a-inspector` updates to v1.0 cards. |
| W8 | `itk.yml:101` | `continue-on-error: true` | opt-in `workflow_dispatch` job only | The upstream ITK resolves dependencies from a private Google Artifact Registry that 401s unauthenticated. The deterministic in-repo `itk-traversal-selftest` is the authoritative gate. | A public ITK lockfile exists. |
| W9 | `tck/conformance-baseline.json` | baselined known failures | — | **Empty (`{}`).** No MUST failure is currently waived. | already clear |

## Transport coverage

The repo implements four transports. TCK coverage is not uniform across them,
and a transport with no conformance job is a gap, not a pass.

| Transport | Spec | Official TCK | In-repo TCK | Other evidence |
|---|---|---|---|---|
| JSON-RPC | §9 | yes — 73 MUSTs | yes — both agent legs | cross-SDK matrix |
| HTTP+JSON / REST | §11 | yes — 69 MUSTs | yes — both agent legs | cross-SDK matrix |
| gRPC | §10 | yes — 53 MUSTs | **no** (`--binding` accepts only `jsonrpc`/`rest`) | golden wire fixtures vs official Python SDK |
| WebSocket | §12 *custom binding* | **no** | **no** | unit/integration tests only |

WebSocket is a §12 custom binding, so the official suite has no notion of it —
its absence there is expected, not a defect. Its absence from the in-repo
runner is a genuine coverage gap, since that runner is ours to extend. gRPC is
graded by the official suite, so the in-repo runner's lack of a gRPC binding is
a redundancy gap rather than a blind spot.

## What "100%, nothing skipped" would actually require

Ranked by what is within this project's control.

1. **Nothing, for MUST-level conformance as the suite can measure it.** 88/88
   graded MUSTs pass on the full profile; the baseline is empty. This is done.
2. **W5/W6 (in this project's control only to remove, not to fix).** Both are
   upstream SDK defects. The runner now fails if either starts passing, so they
   cannot linger unnoticed.
3. **W3 and W4 — upstream TCK defects, both filed.** Neither can be closed here
   without patching the harness, which this repo declines to do (§18). Removing
   them is gated on `#225` and `#193`.
4. **WebSocket conformance leg in the in-repo runner.** The only genuinely
   open, in-scope coverage gap in the table above.
5. **The 21 `NOT TESTED` MUSTs.** Not closable by this SDK at all — the suite
   has no tests for them. Closing them means contributing tests upstream.

Items 3 and 5 mean a literal "100% with nothing skipped" is **not currently
reachable** from inside this repository. Any claim that it has been reached
should be read as a claim that one of those two rows changed upstream.

## Not verified

Recorded so the gaps in this page are as visible as its contents. The
2026-08-09 session did not exercise:

- the `go-sdk`, `python-sdk`, `java-sdk`, `python`, `javascript`, `go` and
  `java` legs of the cross-language matrix (toolchain/registry cost);
  **W6 in particular rests on an inherited claim, not a fresh measurement**;
- `gRPC wire compatibility (official Python SDK)` and
  `Official Python SDK client vs our server`;
- the ITK jobs.

Those jobs were observed passing in CI, which is weaker evidence than a
reproduced run — that distinction is the whole reason this page exists.
