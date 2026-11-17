<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Conformance statement

What this release claims about the A2A specification, what each claim rests on, and where it stops. The specification is
**A2A v1.0.1**. The figures were read on 2026-11-13, from the commands named, so they can be run again.

## What is claimed

* **Bindings:** JSON-RPC 2.0 over HTTP (§9), gRPC (§10) and HTTP+JSON (§11), all three graded by the A2A project's suites.
  The WebSocket binding (§12 custom binding, `websocket`) is this SDK's own and no external suite grades it. It is graded by
  the in-repo `a2a-tck` and the cross-binding equivalence checks.
* **Protocol version 1.0.** The client sends `A2A-Version: 1.0` and selects only interfaces whose `protocolVersion` is
  `1.0`, with the patch ignored (§3.6).
* **Requirements:** every keyword line of the v1.0.1 specification has a row in `docs/compliance/spec-ledger.json`.
  `python3 scripts/check_spec_ledger.py` prints:

  | Status | Rows | Meaning |
  |---|---|---|
  | TESTED | 455 | A test that exists is cited for it |
  | APPLICATION | 32 | It binds the application built on the SDK, not the SDK |
  | NOT_APPLICABLE | 19 | It binds a role this SDK does not play |
  | PARTIAL | 4 | Part is tested and the row says which part is not |
  | **Total** | **510** | |

  The ledger is the maintainer's own reading. It is checked mechanically for coverage and for citations that exist, not
  for whether each cited test proves its requirement.

## External suites

**Official `a2aproject/a2a-tck`**, pinned at `70f6f73c30208e80c889141907e094899e6e9f05`, against `tck/sut`. Run locally with
`scripts/run_workflow_job.py .github/workflows/official-tck.yml official-tck` (every step passed). The counts come from its
pytest summaries and from `/tmp/full-compatibility.json`.

| Profile | Passed | Failed | Skipped | MUST graded (floor) | Gate |
|---|---|---|---|---|---|
| Full | 240 | 4 | 19 | 87 (87) | Failures exactly the baseline |
| Minimal capability | 175 | 4 | 84 | 65 (65) | Failures exactly the baseline |
| Required extension | 2 | 0 | — | 1 (CORE-CAP-004) | Passed |

In the full profile the suite classifies the requirements it knows as follows:

| Level | Pass | Fail | Skipped | Manual | Not tested |
|---|---|---|---|---|---|
| MUST | 85 | 2 | 5 | 7 | 16 |
| SHOULD | 9 | 0 | 1 | 2 | 2 |
| MAY | 4 | 0 | 0 | 0 | 0 |

The two failing MUSTs are **known and baselined**, with the reasoning in `docs/official-tck-findings.md`:

* `CORE-SEND-003`, on all three bindings. A part whose media type the card does not declare is refused with an error, which
  is what §3.1.1 requires. The suite's check asserts the send succeeds, so it cannot pass against an agent that does what
  the requirement's own title says (findings §22).
* `HTTP_JSON-ERR-001`, on HTTP+JSON. Error responses are sent as `application/a2a+json` (§11.1, the maintainer's decision
  of 2026-10-15), where the suite expects `application/json` (findings §23).

A green gate here means "no failure beyond the baseline", **not** "no failure".

**ACTS (`a2aproject/a2a-itk`)**, pinned at `bcce1a15fdce343e2d816ee7e7119f68147385de`, over JSON-RPC, gRPC and HTTP+JSON:
`acts.yml` passed on `main` at `70dc337`
([run 43394485553](https://github.com/tomtom215/a2a-rust/actions/runs/43394485553)); the job fails on any failed or errored
test. The adversarial review of 2026-11-13 read 59, 47 and 49 MUST tests passed on the three bindings respectively, 29, 28
and 32 SHOULD, and 13 MAY on each. Those counts are the reviewer's; this page did not re-run ACTS.

## Where the claim stops

* **What the agent builder writes.** The 32 APPLICATION rows (for example, what an agent's skills do) are yours. The
  `conformance` feature grades an executor in process ([Grading an executor](../deployment/testing/conformance.md)).
* **Requirements no suite grades.** The 7 MUSTs the official suite marks manual and the 16 it marks not tested are covered
  only where the ledger cites a test of this repository's own.
* **The WebSocket binding** is outside both external suites.
* **Deployment.** Conformance is measured against `tck/sut` with in-memory stores on one process. Durability, multiple
  replicas and load are qualified separately ([Readiness bar](https://github.com/tomtom215/a2a-rust/blob/main/docs/readiness-bar.md)), and only in part.
* **Open findings.** The adversarial-review registers under `docs/audits/` list every open and partial row. At the time of
  writing, `main`'s CI is red on two macOS discovery tests (`CI1`), which are not part of the protocol surface. The release
  workflow refuses a tag until `main`'s CI is green.

How each suite is run, and what CI gates on, is in [Conformance Testing](./conformance-testing.md). Every run's history is in
[Conformance History](./conformance-history.md).
