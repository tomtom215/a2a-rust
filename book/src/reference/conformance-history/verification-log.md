<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Waiver re-verification log

Part of [Conformance History](../conformance-history.md). The register's waivers W5 and W6 named defects in official SDKs; this is how they were measured, re-measured against newer releases, and removed.

## W6 is now measured, not inherited

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

## W5 and W6 re-verified against the newest upstream releases — 2026-08-12

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

## 2026-08-30 — the pins moved, and three of the four waivers went with them

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

## 2026-10-12 — the pins moved again, and the last waiver went with them

Pin freshness's 2026-10-05 run graded every official SDK at its latest
release and found one stale skip: `a2a-java` 1.4.0.Final accepts
`application/a2a+json` on REST. Its python-sdk leg had died before grading
anything (no PyYAML on the interpreter it set up; fixed the same week), so it
was dispatched again on 2026-10-12 (run `38299384752`): python-sdk, js-sdk
and go-sdk passed at their latest releases, and java-sdk passed every graded
check and failed only on the stale skip. The maintainer moved all three
pins:

| Pin | Was | Now | Result |
|---|---|---|---|
| `@a2a-js/sdk` | 1.1.0 | **1.3.0** | no waiver; 21/21 on both bindings |
| `a2a-go/v2` | v2.5.0 | **v2.6.0** | no waiver; 21/21 on both bindings |
| `a2a-java` | 1.3.0.Final | **1.4.0.Final** | W6 **removed**; 21/21 on both bindings |

The 21/21 figures were measured locally with this repository's `a2a-tck`
against each agent in `itk/agents/` before the pins were committed.

**a2a-go v2.6.0 reads the spec's `securityRequirements` shape.**
`scripts/go_sdk_interop.sh` step 1b had pinned a2a-go v2.5.0's rejection of
the `{"list":[...]}` scope lists this SDK writes (§8.5, `StringList`), and
failed as designed on the first a2a-go that read them. It now runs the whole
a2a-go client battery against a card with `securityRequirements`: 36 checks
passed, beside step 1's 52 and step 2's zero failures.

**Python is not the only SDK that signs cards.** a2a-go v2.6.0 adds
agent-card signatures (`a2acrypto`; v2.5.0 had none). `@a2a-js/sdk` has
exported `generateAgentCardSignature` and `verifyAgentCardSignature` since at
least 1.1.0, although `itk/interop/card_signature_interop.py` said it had
none. That script still covers only the Python SDK.

No suppression in the [register](./suppressions.md) applies to an official SDK's agent any
more.
