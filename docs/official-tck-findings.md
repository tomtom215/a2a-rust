<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Official A2A TCK — conformance findings

Running the A2A project's own conformance suite
([`a2aproject/a2a-tck`](https://github.com/a2aproject/a2a-tck)) against this
SDK, rather than only the in-repo TCK.

**Why:** conformance graded by the project that owns the specification is
worth more than conformance graded by the implementation being tested. The
in-repo TCK still earns its keep — it drives this SDK's *client* against
agents built on the official Python, JS, Go, and Java SDKs, which the official
TCK does not do — but where the two overlap, the official suite is
authoritative.

**Harness:** `.github/workflows/official-tck.yml`
**System Under Test:** `tck/sut`
**Environment for every measurement below:** `a2a-tck` at `main`, official
Python `a2a-sdk` 1.1.2 (PyPI), this SDK at the commit that adds this file.

---

## Correction notice

**An earlier revision of this document claimed a bug in `a2aproject/a2a-tck`:
that its JSON-RPC client sends snake_case parameters in violation of spec
§5.5, and that this made its pagination assertions "pass vacuously against
every SDK."**

**That claim was wrong, and it is retracted.** It was based on reading the
TCK's source and observing that this SDK rejected the request. Neither step
establishes that the TCK is at fault, and the decisive test — asking whether
the *reference implementation* accepts those requests — was not run before the
claim was published. It has now been run. The reference implementation accepts
them. See §3.

Two further claims in that revision are also retracted: that `CORE-HIST-002`
looked like a genuine `historyLength` defect (it is not — §4), and the
implied severity of the score comparison (§1).

The methodology failure is worth recording, because it is more instructive
than the finding: **an external project's behaviour was called a bug on the
strength of a single-implementation experiment.** One server disagreeing with
one client tells you the two disagree. It does not tell you which is wrong.

---

## 1. Score, and how CI gates on it

| Run | Passed | Failed | Skipped |
|---|---|---|---|
| Against `examples/echo-agent`, before fixes | 128 | 12 | 125 |
| Against `tck/sut`, before fixes | 87 | 15 | 157 |
| Against `tck/sut`, after the §2 fixes | 158 | 12 | 94 |
| Against `tck/sut`, after the §3.3 alias fix | 166 | 10 | 89 |
| Against `tck/sut`, after the §8 inline-push fix | 172 | 4 | 89 |
| Against `tck/sut`, after the §10 direct-message fix | 174 | 2 | 89 |
| Against `tck/sut`, after the §9 subscribe fix | 176 | 0 | 89 |
| …with the gRPC binding advertised too (§11) | 244 | 0 | 21 |
| …after §13, `full` profile | **246** | **0** | 19 |
| `minimal` profile (§12), which reaches `CORE-CAP-*` | 181 | 0 | 83 |

These numbers are **not** directly comparable to one another. The echo agent
advertises fewer capabilities, so the suite asks it less and *skips* where it
would otherwise fail — its 128 is not a better result than the SUT's 87. Only
the second and third rows share a subject and are comparable; that pair is the
real before/after.

Identical results were obtained with the SUT behind a recording proxy
(§3.2), confirming the proxy did not perturb the run.

**There are no remaining failures.** Reported MUST compatibility is
**100.0%** (85.4% → 93.9% after §3.3 → 97.6% after §8 → 98.8% after §10 →
100% after §9). Every failure closed along the way was at `MUST` level; an
earlier revision listed them without saying so, which understated them.

**100% here does not mean fully conformant, and the number should not be
quoted without this sentence.** It is computed over *tested* requirements
only. Of the **114 MUST requirements** the suite knows about:

| | Count | Meaning |
|---|---|---|
| Passing | 88 | measured and conformant on the `full` profile |
| Failing | **0** | — |
| `SKIPPED` | 5 | need a differently-configured SUT; 3 of them pass on the `minimal` profile (§12, §15) |
| `NOT TESTED` | 21 | **the TCK has no test for them at all** |

The last row is not something this SDK can close: those requirements
(`CARD-SIGN-*`, `AUTH-*`, `VER-*`, `BIND-EQUIV-*`, `GRPC-SVC-003`) have no
implementation in `a2a-tck`. Closing them means contributing tests upstream,
and until someone does, **no implementation's score says anything about
them** — including the reference's.

An earlier revision of this document said "21 further MUST requirements …
report `NOT TESTED` because the SUT does not exercise them". Two things were
wrong with that. `NOT TESTED` means the *suite* has no test, not that the SUT
declined one — that is what `SKIPPED` means — and the two were being counted
as one number, hiding 11 requirements that *were* the SUT's to fix. Six of
those are now closed (§11); the remaining 5 are the `SKIPPED` row above.

Historically the denominator was smaller; 21
further MUST requirements (the `CARD-SIGN-*`, `AUTH-*`, `VER-*`, and
`BIND-EQUIV-*` families) report `NOT TESTED` because the SUT does not
exercise them, and are a coverage gap rather than a pass — **not** progress
toward 100%.

### The actual ceiling, and why "0 skipped, 0 not-tested" is not reachable

Because the question "what is left to be *truly* 100% conformant" has a
finite, checkable answer, here it is in one place. Every line was verified
against a live run and against `a2a-tck`'s own source and issue tracker; the
per-item evidence is in §15–§17.

| MUST requirements | Count | Reachable from this repository? |
|---|---:|---|
| `PASS`, `full` profile | 88 | — already passing |
| `PASS`, `minimal` profile only (`CORE-CAP-001/002/003`) | 3 | — already passing (§15) |
| `PASS`, `extension` profile only (`CORE-CAP-004`) | 1 | — already passing (§18) |
| **Measured passing, all three profiles** | **92** | |
| `CARD-EXT-002` | 1 | **No** — structurally inapplicable; this SDK cannot declare `extendedAgentCard` and simultaneously have none (§12) |
| `NOT TESTED` | 21 | **No** — zero test functions exist upstream; 5 carry the suite's own `not-automatable` tag and a 6th (`GRPC-SVC-003`) the same verdict as an inline comment, 2 are an explicit upstream "Won't Do", 13 are open upstream backlog items (§16) |
| **Total** | **114** | |

So: **92 of 114 MUST requirements are measurably passing, 0 are failing, and
all 22 of the remainder are upstream-untested or structurally inapplicable —
none is a defect in this SDK.** The reported "100.0% MUST compatibility" is
100% *of the requirements the suite is able to grade*, and that is the
strongest true statement available. A number like "114/114" is not
achievable by any implementation against `a2a-tck` as it exists today,
including the reference implementations — and any project claiming it should
be asked which of the 22 it closed and how.

The same holds per level: `SHOULD` is 7 `PASS` / 0 `FAIL` / 4 `NOT TESTED`
(all four in the same upstream `AUTH-*` backlog item, `task-27`), and `MAY`
is 4/4 `PASS`.

Per transport, from the same verified `full`-profile run — all three core
bindings the ratified spec defines are graded, and none is failing:

| Transport | Result |
|---|---|
| `agent_card` | 10/10 |
| `jsonrpc` | 95/102 (7 skipped) |
| `http_json` | 91/96 (5 skipped) |
| `grpc` | 69/72 (3 skipped) |

WebSocket is deliberately absent: it is an a2a-rust custom binding under
spec §12, and the official suite has no mechanism to grade a binding the
specification does not define. It is covered by this repository's own
feature-gated tests instead.

### The gate

`.github/workflows/official-tck.yml` does **not** simply require a clean run,
and it does not paper over the failures either. It runs
`tck/scripts/check_conformance.py` against a checked-in baseline
(`tck/conformance-baseline.json`) at (requirement, transport) granularity, and
fails the job on:

- any MUST-level failure **not** in the baseline — a regression; **and**
- any baseline entry that **now passes** — a stale baseline.

The second direction is what keeps the first honest. A baseline that is
allowed to rot becomes a blanket exemption, at which point the check is green
for the same bad reason `continue-on-error: true` was.

Transport granularity matters: `PUSH-CREATE-001` used to fail on `jsonrpc`
while passing on `http_json`, and a requirement-level baseline would not have
noticed it starting to fail on `http_json` too. The baseline is now **empty**,
down from 16 pairs across 12 requirements — so the gate has become a plain
"no MUST-level failure" check, and any regression fails CI immediately. That
it still fires with nothing baselined was verified rather than assumed (table
below).

The stale-baseline direction is not theoretical — it fired on both fix
commits, which is how each shrink was forced:

```
STALE BASELINE — 7 baselined check(s) now pass:      # after §3.3
  CORE-HIST-002 [jsonrpc]   PUSH-CREATE-001 [jsonrpc]  PUSH-CREATE-002 [jsonrpc]
  PUSH-DEL-001 [jsonrpc]    PUSH-DEL-002 [jsonrpc]     PUSH-GET-001 [jsonrpc]
  PUSH-LIST-001 [jsonrpc]

STALE BASELINE — 6 baselined check(s) now pass:      # after §8
  PUSH-DELIVER-001 [jsonrpc]  PUSH-DELIVER-001 [http_json]
  PUSH-DELIVER-002 [jsonrpc]  PUSH-DELIVER-002 [http_json]
  PUSH-DELIVER-003 [jsonrpc]  PUSH-DELIVER-003 [http_json]

STALE BASELINE — 1 baselined check(s) now pass:      # after §10
  DM-MSG-001 [*]

STALE BASELINE — 2 baselined check(s) now pass:      # after §9
  STREAM-SUB-002 [jsonrpc]   STREAM-SUB-002 [http_json]
```

Both directions of the gate, and its behaviour on malformed input, are
exercised deliberately rather than assumed:

| Injected into the report | Gate |
|---|---|
| unmodified (control) | exit 0 |
| a MUST failure not in the baseline | exit 1 — regression |
| a baselined requirement failing on a **new** transport | exit 1 — regression |
| a baselined entry now passing | exit 1 — stale |
| a `SHOULD`-level failure | exit 0 — correctly not gated |
| report missing / not JSON / `null` / `[]` / a number | exit 1 with a readable message |

> **An earlier revision of this workflow reported `Success` while these 12
> checks failed**, because both suite steps carried `continue-on-error: true`.
> The annotation said `Process completed with exit code 1` and the badge said
> green. That is precisely the defect this document criticises elsewhere — a
> published signal that does not reflect reality — and it was shipped here. A
> green check nobody can trust is worse than a red one, because nobody reads
> a green check.

## 2. Fixed as a result

### 2.1 Unsupported `Content-Type` returned the wrong JSON-RPC error code

*Found by `JSONRPC-SSE-002`. Fixed. Confirmed by re-run.*

The JSON-RPC binding rejected an unsupported media type with
`ParseError (-32700)`. Spec §5.4 maps it to
`ContentTypeNotSupportedError (-32005)`. The body is never parsed on this
path, so "parse error" both misreported the cause and withheld the
machine-readable `CONTENT_TYPE_NOT_SUPPORTED` reason that §10.6 requires.
Routing it through `error_response` also attaches the `google.rpc.ErrorInfo`
detail, as every other A2A error on this binding does.

Three in-repo tests asserted the wrong code and passed confidently. A suite
that encodes a bug validates it forever.

### 2.2 The task state machine rejected conformant agents

*Found by six MUST-level checks (`CORE-SEND-001`, `CORE-SEND-003`,
`CORE-EXECUTION-MODE-001`, `CORE-MULTI-001a`, `CORE-MULTI-002a`,
`CORE-MULTI-003`). Fixed. Confirmed by re-run.*

`TaskState::can_transition_to` required `Submitted → Working` before any
finish state, so an agent completing in one step was rejected by its own SDK
with `InvalidParams: invalid state transition ... TASK_STATE_SUBMITTED →
TASK_STATE_COMPLETED`.

Spec §4.1.3 enumerates the states and classifies them as terminal or
interrupted; it defines no transition matrix and requires no intermediate
state. The reference SDK's SUT completes and requests input directly from
`Submitted`. The table now enforces only that terminal states are final and
that nothing re-enters `Submitted` or the proto-default `Unspecified`;
`Working → Rejected` is permitted too, per §4.1.3 allowing rejection "later
once an agent has determined it can't or won't proceed".

`state_validation_tests.rs` was rewritten to pin all 81 matrix cells against
an independently-computed predicate.

## 3. The retracted claim, and what is actually true

### 3.1 The reference implementation accepts snake_case

The A2A JSON data model is generated from a protobuf schema. Protobuf's
canonical JSON mapping requires parsers to accept **both** the `json_name`
(camelCase) and the original proto field name (snake_case). The official
Python SDK's types are protobuf messages, not hand-written models:

```
>>> import a2a.types as T
>>> T.TaskPushNotificationConfig.__mro__[:2]
(<class 'a2a_pb2.TaskPushNotificationConfig'>, <class 'google._upb._message.Message'>)
>>> [(f.name, f.json_name) for f in T.TaskPushNotificationConfig.DESCRIPTOR.fields]
[('tenant','tenant'), ('id','id'), ('task_id','taskId'), ('url','url'), …]
```

Measured against `a2a-sdk` 1.1.2 via `json_format.ParseDict`:

| Input | Reference SDK | Emits |
|---|---|---|
| `{"taskId": "t1", …}` | **accepted** | `{"taskId": "t1", …}` |
| `{"task_id": "t1", …}` | **accepted** | `{"taskId": "t1", …}` |
| both, agreeing | accepted | camelCase |
| both, conflicting | accepted, last wins | camelCase |
| `{"bogus": 1, …}` | **rejected** — `ParseError: has no field named "bogus"` | — |

`ListTasksRequest` with `{"context_id", "page_size", "page_token"}` and
`GetTaskRequest` with `{"history_length"}` are likewise accepted and
normalised to camelCase.

So **§5.5's "MUST use camelCase" governs emission, not acceptance** — the
reference emits camelCase unconditionally while accepting both, which is
exactly what a ProtoJSON implementation does. The TCK's requests are
legitimate against a ProtoJSON-based peer. There is no TCK bug here.

The related claim that the TCK's pagination assertions "pass vacuously
against every SDK" is refuted by the same table: the reference parses
`page_size` into `pageSize` correctly.

### 3.2 What the TCK actually sends, captured on the wire

Source-reading was not sufficient evidence, so the traffic was recorded. The
SUT gained a `SUT_ADVERTISE_URL` environment variable so its agent card can
point at a recording reverse proxy; 261 requests were captured across a full
run, with an identical 158/12/94 result.

Distinct snake_case JSON **keys** observed, by method:

| Method | Key | Requests |
|---|---|---|
| `CreateTaskPushNotificationConfig` | `task_id` | 6 |
| `GetTaskPushNotificationConfig` | `task_id` | 1 |
| `GetTask` | `history_length` | 5 |
| `ListTasks` | `context_id` | 5 |
| `ListTasks` | `page_size` | 2 |
| `ListTasks` | `include_artifacts` | 1 |

This confirms the source-reading, but it is the §3.1 table — not this one —
that determines who is at fault.

### 3.3 The real defect: this SDK silently ignores unrecognised parameters

*Part (a) fixed. Part (b) still open — see below.*

The reference rejects unknown fields (§3.1). This SDK ignores them, and for
filter parameters that converts a client-side mistake into **silently wrong
data** rather than an error. Measured on the wire against `tck/sut`, seeded
with 5 tasks of which 3 are in `ctx-A`:

| `ListTasks` params | Before | After |
|---|---|---|
| *(none)* | 5 tasks | 5 tasks |
| `{"contextId": "ctx-A"}` | **3** — correct | **3** — correct |
| `{"context_id": "ctx-A"}` | **5** — filter ignored | **3** — correct |
| `{"contextID": "ctx-A"}` | **5** — filter ignored | **5** — still ignored |
| `{"contxtId": "ctx-A"}` | **5** — filter ignored | **5** — still ignored |
| `{"totallyBogusField": 1}` | 5 — ignored | 5 — still ignored |
| `{"pageSize": 1}` | 1 task — correct | 1 task — correct |
| `{"page_size": 1}` | **5** — filter ignored | **1** — correct |
| `{"pagesize": 1}` | **5** — filter ignored | **5** — still ignored |

A caller who asks for one context's tasks and misspells the key by any means
receives **every** task instead of an error. The snake_case spelling the TCK
sends is one instance of this; a typo is another. Only the first is fixed.

The same class of bug is loud rather than silent where the field is required:
`CreateTaskPushNotificationConfig` with `task_id` used to fail with
`-32602 taskId is required`, which was at least visible. It now succeeds.

**Scope:** 73 multi-word fields across the 44 messages in
`proto/a2a_v1/a2a.proto`. An earlier revision of this document said 41, which
counted only the request-facing types — a client parsing another
implementation's *responses* and agent cards hits the same wall, so the fix
covers the whole schema.

#### (a) Accept the proto field name as an alias — **done**

`#[serde(alias = "<proto_name>")]` on all 67 fields that lacked it (`Part`
already accepted `media_type` by hand, and 5 are exempt — see below).

The list is derived from `a2a.proto`, not from the Rust field names, by
`crates/a2a-protocol-types/tests/proto_field_alias.rs`. Deriving it from the
Rust names would have been circular: it would prove the aliases match the Rust
identifiers rather than the wire contract, and would go stale silently when
the schema grows a field. The test parses the schema, computes the camelCase
spelling with protobuf's own `ToJsonName` rather than serde's `rename_all`,
and for every multi-word field asserts that:

- both spellings deserialize, and to **the same value**;
- the sample value is **distinguishable from the field being absent** — a case
  that would pass with no alias at all is reported as a bad case, not a pass;
- only the `json_name` is **emitted** (spec §5.5 governs emission), so a
  `rename`/`alias` swap cannot quietly put snake_case on the wire;
- every multi-word field in the schema is either covered or explicitly
  exempt, and no case references a field the schema no longer has.

Five counter-tests drive each of those directions to a failure on purpose, so
none of them is a gate that has never been observed firing. Two of them earned
their keep immediately: the first draft of the schema parser matched
`starts_with("option")`, which silently swallowed every `optional` field
(`page_size`, `history_length`, `include_artifacts`), and scanned for
`"message "` anywhere in the text, which the field `Message message = 2;`
derailed into skipping `Part`, `GetTaskRequest`, `StreamResponse` and
`ListTaskPushNotificationConfigsResponse`. Both produced a *smaller* case list
that passed everything asked of it.

**Deliberate divergence, recorded:** a request carrying *both* spellings of one
field is rejected here (`-32602 duplicate field`) where the reference accepts
it and takes the last key.

```text
ParseDict({"context_id": "A", "contextId": "B"}, ListTasksRequest())
  -> {"contextId": "B"}     # reference: accepted, last wins
  -> -32602 duplicate field `contextId`      # here
```

No conformant ProtoJSON printer emits both spellings, so only a hand-built or
buggy request reaches this path; refusing to guess which one the caller meant
is safer than silently picking one. Pinned by
`both_spellings_at_once_is_an_error` so it stays a decision.

**Exempt, and why:** the 5 arms of the `SecurityScheme` oneof. This SDK
encodes that type as an internally tagged union (`{"type": "apiKey", …}`)
rather than as a ProtoJSON oneof (`{"apiKeySecurityScheme": {…}}`). That is a
wire divergence an alias cannot fix — see §7.

#### (b) Reject unknown fields — **tried, measured, reverted. Must not be done.**

*An earlier revision of this section called this "worth doing, worth agreeing
on first", on the grounds that it matches the reference's `ParseError`. That
recommendation was wrong and is retracted.*

The specification says the opposite, in as many words:

> **Unrecognized Fields:**
>
> Implementations **SHOULD** ignore unrecognized fields in messages, allowing
> for forward compatibility as the protocol evolves.
>
> — `specification.md` §11

The official TCK grades exactly this as **`DM-SERIAL-005`**, sending

```json
{"params": {"message": {…, "tckUnknownField": "should-be-ignored"},
            "tckExtraParam": 42}}
```

and failing the implementation if the server returns an error.

This was not reasoned about — it was **measured**.
`#[serde(deny_unknown_fields)]` was added to all ten request-parameter types,
the whole workspace stayed green (2,099 tests), and the official suite then
reported:

```
DM-SERIAL-005 SHOULD {"jsonrpc": "FAIL", "http_json": "FAIL"}
  Server rejected request with unrecognized fields: invalid params:
  unknown field `tckExtraParam`, expected one of `tenant`, `message`, …
```

The run went from `172 passed` to `170 passed, 2 xfailed`. The change was
reverted.

**Two things are worth recording about how this was nearly shipped.**

First, the recommendation came from reading what the reference implementation
does — its JSON-RPC dispatcher calls `ParseDict` strictly — rather than from
the normative text. That is the §Correction-notice mistake in a new costume:
*an implementation's behaviour was treated as the authority when the
specification was sitting right there.* The spec is the authority. (What that
implies about the reference's own `DM-SERIAL-005` result is not claimed here:
this SDK's TCK run says nothing about another implementation's, and no run
against the reference server was made.)

Second, **the conformance gate would not have caught it.** The gate is
MUST-only by design, and this is a `SHOULD`. It went green on the exact commit
that introduced the regression; only reading the full suite output caught the
`170 passed, 2 xfailed` line. A MUST-only gate is still the right choice — a
`SHOULD` regression should not block a merge — but "the gate is green" and
"nothing regressed" are not the same statement, and this is the second time in
this document that a green signal proved narrower than it looked.

So the residual wart stands: `ListTasks` with `{"contxtId": …}` returns every
task. That is a cost the specification has explicitly chosen in exchange for
forward compatibility, and it is not this SDK's to unilaterally reprice. It is
pinned by `unrecognised_fields_are_ignored_not_rejected`, which exists to stop
someone re-applying the fix that was just measured to be wrong.

A mitigation that would satisfy both — counting or logging unrecognised fields
so an operator can *see* the silent case without the request failing — is not
implemented here, and is the shape any future attempt at this should take.

## 4. `CORE-HIST-002` is the §3.3 bug, not a `historyLength` defect

*Earlier called a probable genuine defect. Retracted. Closed by the §3.3(a)
alias fix — it no longer appears in the baseline.*

The TCK reported "`GetTask` with `historyLength=1` returned 2 messages".
`historyLength` is in fact honoured correctly; the TCK sends
`history_length`, which is ignored per §3.3. Measured on a task with two
history entries:

| `GetTask` params | History returned |
|---|---|
| *(no length param)* | 2 |
| `{"historyLength": 1}` | **1** — honoured |
| `{"history_length": 1}` | **2** — ignored |

One root cause, two reported symptoms.

## 5. Still open

**Nothing.** `tck/conformance-baseline.json` is empty and the suite reports
`176 passed, 0 failed, 89 skipped`.

That is not the same as "fully conformant" — see the warning in §1 about the
21 `NOT TESTED` MUST requirements, which is now the single largest gap in what
this measurement can tell you.


Closed since the previous revision:

- by §3.3(a) — `CORE-HIST-002`, `PUSH-CREATE-001`, `PUSH-CREATE-002`,
  `PUSH-DEL-001`, `PUSH-DEL-002`, `PUSH-GET-001`, `PUSH-LIST-001` (7 pairs,
  all `jsonrpc`);
- by §8 — `PUSH-DELIVER-001`, `PUSH-DELIVER-002`, `PUSH-DELIVER-003` on both
  bindings (6 pairs);
- by §10 — `DM-MSG-001` (1 pair). Its earlier "likely a SUT gap" guess was
  wrong: the SUT was correct and the server was not.

**A retracted assumption:** the previous revision said the `jsonrpc` legs of
`PUSH-DELIVER-*` "follow from `PUSH-CREATE-001`" while the `http_json` legs
needed separate investigation. That was wrong in both halves. `PUSH-CREATE-001`
passing did not fix any `PUSH-DELIVER-*` leg, and the two bindings shared one
cause (§8) rather than needing separate ones. The lesson is the same one this
document keeps recording: a plausible causal story about an undiagnosed
failure is not a diagnosis, and labelling it as one costs more than saying
"unknown".

## 6. Reproducing every measurement

```sh
# SUT — full profile (default), both JSON transports plus gRPC (§11)
cargo build --release -p a2a-tck-sut
SUT_HOST=127.0.0.1:9999 SUT_GRPC_HOST=127.0.0.1:9998 \
  ./target/release/a2a-tck-sut &

# SUT — minimal profile (§12), makes the capability-rejection paths
# (CORE-CAP-001/002/003) observable; run against its own ports so both
# profiles can be up at once
SUT_PROFILE=minimal SUT_HOST=127.0.0.1:9997 SUT_GRPC_HOST=127.0.0.1:9996 \
  ./target/release/a2a-tck-sut &

# Official suite
git clone --depth 1 https://github.com/a2aproject/a2a-tck /tmp/a2a-tck
cd /tmp/a2a-tck && uv venv && uv pip install -e .
./.venv/bin/python run_tck.py --sut-host http://127.0.0.1:9999   # full
./.venv/bin/python run_tck.py --sut-host http://127.0.0.1:9997   # minimal

# reports/compatibility.json is OVERWRITTEN by each run — copy it out
# immediately after each invocation if you need to compare both profiles,
# rather than re-reading it later and assuming it still holds the first
# run's data (§15 was a live lesson in exactly this mistake).

# Reference-SDK acceptance (§3.1)
uv venv && uv pip install 'a2a-sdk[http-server,grpc,sqlite]'
python -c "
import a2a.types as T
from google.protobuf import json_format
for p in ({'taskId':'t1'}, {'task_id':'t1'}, {'bogus':1}):
    try: print(p, '->', json_format.MessageToDict(
        json_format.ParseDict(p, T.TaskPushNotificationConfig())))
    except Exception as e: print(p, '-> REJECTED', e)"

# Wire capture (§3.2): point the card at a recording proxy
SUT_HOST=127.0.0.1:9999 SUT_ADVERTISE_URL=http://127.0.0.1:9990 \
  ./target/release/a2a-tck-sut &
```

```sh
# The gate CI runs (exit 0 only if failures match the baseline exactly)
python3 tck/scripts/check_conformance.py \
  --report /tmp/a2a-tck/reports/compatibility.json \
  --baseline tck/conformance-baseline.json

# After fixing something, shrink the baseline in the same commit
python3 tck/scripts/check_conformance.py --report … --baseline … --update
```

`--level must` restricts a run to hard conformance requirements; it was
verified to yield an identical set of gated failures to the full run, so CI
runs the full suite once. Reports land in `reports/` as HTML, JSON, and JUnit
XML; the gate reads `compatibility.json`.

## 7. `securitySchemes` was emitted in the v0.3 shape, not the v1.0 shape

*Fixed. Found while doing §3.3, not by the TCK — no `CARD-*` check covers it.
Confidence: verified by cross-implementation test against `a2a-sdk` 1.1.2 and
against the checked-in schema.*

`proto/a2a_v1/a2a.proto` is byte-identical to the specification copy vendored
by the TCK (`a2aproject/A2A` at `v1.0.0`, commit `1736957`; verified by
`diff`, 0 lines). It defines `SecurityScheme` as a **oneof of five arms** and
`APIKeySecurityScheme` with a field named **`location`**. The specification
text generates its data-model tables directly from that proto
(`{{ proto_to_table("SecurityScheme") }}`), so the proto is normative for the
JSON shape. This SDK instead encodes the type as an internally tagged union
with `"type"` and `"in"` — the OpenAPI-style v0.3 shape.

What this SDK emits today:

```json
"securitySchemes": {
  "apiKeyAuth": {"type": "apiKey", "in": "header", "name": "X-API-Key"},
  "bearerAuth": {"type": "http", "scheme": "bearer", "bearerFormat": "JWT"}
}
```

What the v1.0 schema defines:

```json
"securitySchemes": {
  "apiKeyAuth": {"apiKeySecurityScheme": {"location": "header", "name": "X-API-Key"}},
  "bearerAuth":  {"httpAuthSecurityScheme": {"scheme": "bearer", "bearerFormat": "JWT"}}
}
```

**Severity, measured rather than assumed.** That card JSON — emitted by this
SDK's own serializer — was fed to three parsers in the reference SDK:

| Parser | Before | After |
|---|---|---|
| `a2a.client.card_resolver.parse_agent_card` (what a reference client calls) | **accepted**, both schemes recovered in full | accepted, in full |
| `json_format.ParseDict(..., AgentCard())`, strict | **rejected** — `has no field named "type"` | **accepted**, in full |
| `json_format.ParseDict(..., AgentCard(), ignore_unknown_fields=True)` | **accepted, schemes silently empty** — `{"apiKeyAuth": {}, "bearerAuth": {}}` | **accepted**, in full |

Both columns were measured the same way: this SDK's own serializer produced the
card bytes, which were then fed to each parser.

So this is **not** an interop break with the reference SDK: its resolver
carries an explicit backward-compatibility shim that maps `type` → the oneof
arm and `in` → `location`, and its docstring calls the input "legacy". Row 1
is the path a real reference client takes.

It is nonetheless worth fixing, and row 3 is why: a peer that parses the card
with ProtoJSON and `ignore_unknown_fields=True` — the same option the
reference resolver itself passes — gets an agent card declaring **two security
schemes with no contents**, and would conclude the agent supports no usable
authentication. That is the §3.3 failure mode again (silently wrong data
rather than an error), pointed the other way across the wire.

**Fixed by emitting the v1.0 shape while accepting both.** `SecurityScheme`
now has a hand-written `Serialize`/`Deserialize` pair: it emits the ProtoJSON
`oneof` encoding unconditionally, and accepts the v1.0 encoding under either
spelling of the arm name (`apiKeySecurityScheme` or
`api_key_security_scheme`) *and* the v0.3 encoding, which normalises to the
v1.0 form on re-emission. `ApiKeySecurityScheme.location` is emitted as
`location`, the proto field name, with `in` retained as an alias.
`ApiKeyLocation` keeps its `header`/`query`/`cookie` string values, which are
exactly what the proto comment enumerates for `string location`.

This is a breaking change to a published wire format — the bytes on
`/.well-known/agent-card.json` change for every consumer of this SDK — so it
is deliberate rather than incidental, and it only breaks a consumer that reads
the card's raw JSON keys rather than parsing it with an A2A implementation.
Deserialization is strictly more permissive than before.

Deserialization is buffered through `serde_json::Map` rather than streamed,
because telling the two encodings apart needs the whole object: the v0.3
discriminator `type` may arrive after other keys. Agent cards are parsed at
discovery time, not per request, so this is not on a hot path — unlike `Part`,
which is, and therefore keeps its hand-rolled single-pass visitor.

With the arms no longer exempt, `proto_field_alias.rs` now covers **all 73**
multi-word schema fields with an empty `EXEMPT` list.

**Five in-repo tests asserted the old encoding and passed confidently** —
`tck_security_scheme_*_wire_format` in `tck_wire_format.rs` checked
`ser["type"] == "apiKey"`. That is the §2.1 trap a third time: a suite that
encodes a defect validates it forever. They now assert the v1.0 encoding
round-trips, that a v0.3 scheme still parses *and* normalises to the v1.0
form, and that an object matching neither encoding is rejected rather than
silently defaulted.

## 8. Inline push notification configs were parsed and dropped

*Fixed. Confirmed by re-run.*

*Found by `PUSH-DELIVER-001/002/003`, all six legs. Confidence: verified by
hand reproduction against a local webhook receiver, by the schema text, and
against the reference implementation's source.*

The schema is explicit that `SendMessage` is a way to register a push config:

```protobuf
// Configuration for the agent to send push notifications for task updates.
// Task id should be empty when sending this configuration in a `SendMessage` request.
TaskPushNotificationConfig task_push_notification_config = 2;
```

This SDK deserialised the field and never looked at it again. The TCK
registers its webhook exactly this way, so all six delivery checks failed with
"No webhook request received within timeout".

Hand reproduction against `tck/sut` with a local receiver on `:9877`:

| Flow | `ListTaskPushNotificationConfigs` | Webhooks delivered |
|---|---|---|
| inline via `configuration.taskPushNotificationConfig` (what the TCK does) | `{"configs": []}` | **0** |
| explicit `CreateTaskPushNotificationConfig` | 1 config | 2 |

The second row is what made the diagnosis conclusive: delivery, retry, auth
headers and payload shape were all working. Only registration was missing.

The reference implementation registers the inline config against the task id
before the executor starts (`default_request_handler_v2.py`, `_setup_task`),
and this SDK now does the same, at the point where the task is saved and
before the executor is spawned — so the first status transition is already
covered.

**A wrong turn worth recording.** The first pass at this reported *two*
defects: the missing registration, and a missing `Authorization` header on
delivery. The second was an artefact of the probe, not of the SDK — the
reproduction read `headers.get("Authorization")` from a `dict()` built out of
Python's `http.server` header object, where the key arrives lower-cased. The
header was being sent correctly all along. Dumping *all* headers instead of
probing one key by name is what caught it. A single-key lookup that returns
`None` looks exactly like a missing feature.

**Behaviour change:** a `SendMessage` carrying an inline push config against a
server with no push support now fails with `PushNotSupported` instead of
succeeding and silently ignoring the config. The reference skips silently
here; this SDK does not, on the same reasoning as §3.3 — a client that asked
for notifications and will never get any should be told. Registration reuses
the standalone create's validation (capability check, task existence, SSRF
screening, per-task and global quotas) rather than writing to the store
directly, so the inline path cannot become an unguarded back door; four
counter-tests in `inline_push_config_tests.rs` drive those guards to failure
through the inline path specifically.

## 9. `STREAM-SUB-002`: the subscribe stream ended with the executor, not with the task

*Fixed. Confirmed by re-run. Confidence: verified by hand reproduction, the
spec text, and a wire capture of the fixed stream.*

Spec §3.1.6:

> The stream MUST terminate when the task reaches a terminal state
> (`completed`, `failed`, `canceled`, or `rejected`).

Hand reproduction — create a task the executor leaves in `input_required`,
`SubscribeToTask`, then complete it from another thread:

```
task cd5e005b… state=TASK_STATE_INPUT_REQUIRED
subscribe HTTP 200 content-type=text/event-stream
  [stream closed by server]
1 SSE data frame(s):
  [0] task  state=TASK_STATE_INPUT_REQUIRED
VERDICT: last frame terminal? False
```

The stream closed **while the task was still non-terminal**, before the
transition it existed to report.

**Root cause.** A task's event queue lives exactly as long as one *executor
invocation*: the spawned executor ends with `drop(writer)` and
`event_queue_manager.destroy(&task_id)`, unconditionally — including when it
deliberately parks the task in a non-terminal interrupted state such as
`input_required` (§4.1.3). So the queue, and every stream reading it, died at
the turn boundary. The follow-up `SendMessage` that completed the task created
a *new* queue the earlier subscriber was not attached to.

### The design that was tried first, and why it was abandoned

The obvious fix is to keep the queue alive while the task is non-terminal and
destroy it at the terminal transition. That was implemented — queue retention,
a `rebind` that hands a continuation a fresh persistence channel over the
existing broadcast channel, and TTL eviction of retained queues under capacity
pressure — and then reverted, because it **deadlocks the background event
processor**:

> the persistence channel closes only when *all* senders drop, and the manager
> holds one of them. Retaining a queue therefore means `persistence_reader.recv()`
> never returns `None`, and the background processor's drain loop never exits —
> one leaked task per parked task.

Fixing *that* means reworking both drain loops (the background processor's and
the sync collector's) to stop on executor-exit rather than channel-close, on
top of the `QueueLease::Existing` rework that continuations already needed.
That is a redesign of the streaming core with real concurrency risk, and it is
not what this defect requires.

### What was actually done

The fix lives entirely in the subscribe path. `InMemoryQueueReader` gained an
optional **reattach hook**, consulted when its broadcast channel closes:

- the task is **terminal** → emit a `TaskStatusUpdateEvent` carrying that
  terminal status, then end;
- the task is **gone** → end;
- the task is still running → wait for the next turn's queue and continue on
  it.

Nothing else changes. The send path, the executor lifecycle, the persistence
channel and the queue manager's destroy semantics are all untouched, so none of
the deadlock hazards above arise. Every binding that already accepts an
`InMemoryQueueReader` inherits the behaviour with no signature change.

**The synthesized final frame is not cosmetic.** With only "wait for the next
queue", the reproduction still failed: the task completed in the window
between one queue closing and the next opening, so the hook saw a terminal
task and ended the stream — correct in *duration*, but the client never
observed a terminal state, which is exactly what `STREAM-SUB-002` asserts. The
frame is built from the authoritative stored status, and is suppressed when a
terminal frame has already been delivered, so a client never sees it twice.

Wire capture after the fix:

```
2 SSE data frame(s):
  [0] task          state=TASK_STATE_INPUT_REQUIRED
  [1] statusUpdate  state=TASK_STATE_COMPLETED  <-- TERMINAL
VERDICT: last frame terminal? True
```

**Two bounds, because "wait until terminal" is otherwise unbounded.**
`subscribe_reattach_interval` (250 ms) is how often an idle stream re-checks,
and `subscribe_max_idle` (5 min) is how long it waits on a task that never
progresses before ending — §3.5.2 makes reconnection an expected flow, and a
task parked forever must not pin a connection forever. Both are on
`HandlerLimits`.

**A test that asserted the defect.**
`resubscribe_nonterminal_no_queue_returns_snapshot_then_eof` explicitly
required the non-conformant behaviour — snapshot, then immediate EOF — citing
§3.5.2 reconnection. It is now
`resubscribe_nonterminal_no_queue_waits_for_the_terminal_state`, and asserts
the stream stays open while the task is `Working`, then reports `Completed`,
then ends. A second test pins the idle bound, since "stay open until terminal"
without one is a connection leak. That makes **four** in-repo tests found
encoding a defect over this work (§2.1's three error codes, §7's five security
schemes, and this one) — the pattern is worth more attention than any
individual instance.

## 10. `DM-MSG-001`: a blocking `SendMessage` could never return a `Message`

*Fixed. Confirmed by re-run. Confidence: verified by hand reproduction, the
spec text, and the reference implementation's source.*

Spec §3.1.1 gives `SendMessage` two possible outputs:

> - [`Task`]: A task object representing the processing of the message, **OR**
> - [`Message`]: A direct response message (for simple interactions that don't
>   require task tracking)

This SDK only ever produced the first. `SendMessageResponse::Message` existed
as a type and was never constructed anywhere in the server — `grep` for it
returned only `::Task` call sites. `collect_events` appended any
`StreamResponse::Message` the executor wrote to `Task.history` and returned
the task regardless.

Reproduced on the wire against the SUT's `tck-message-response` scenario,
whose executor writes exactly one `StreamResponse::Message` and nothing else:

```json
{"result": {"task": {"id": "ec6bb9d0…", "status": {"state": "TASK_STATE_SUBMITTED"}}}}
```

The response was not merely the wrong shape — it was a task stuck in
`Submitted`, because nothing ever moved it. A client got a task handle for
work that had already finished and would never progress.

**The earlier guess in §5 was wrong.** That entry read "likely a SUT gap —
writing `StreamResponse::Message` to the event queue may not be how this SDK
returns a message-instead-of-task". The SUT was doing the right thing; there
was simply no code path from that event to a `Message` response. It was
labelled *observed symptom only, no root cause established*, which is why it
was a guess rather than a claim — but it still pointed at the wrong component.

**The rule, and why it is narrower than the reference's.** The reference
treats *any* `Message` event as the response, and its `ActiveTask` consumer
raises `InvalidAgentResponseError` if further events follow one:

```python
if isinstance(event, Message):
    result = event
    # Do NOT break here as Message is supposed to be the only
    # event in "Message-only" interaction.
```

Adopting that verbatim would break every agent here that narrates progress —
emit a message, then keep working — since those streams would become errors.
So the response is a `Message` only when the executor produced a message
**and nothing task-shaped**: no status transition off `Submitted`, no
artifacts, no `Task` snapshot. For a well-behaved message-only agent that is
the same answer the reference gives; for a mixed stream the message stays in
`Task.history` and the task is still returned, exactly as before.

A task row is still created and persisted for a message-only interaction, so
`GetTask` continues to work for a client that wants one.

Four tests pin this in `send_sync_tests.rs`: the message-only case, and three
counter-tests — message-then-work still returns the Task, an executor that
emits no message still returns the Task, and the message-only interaction
still records a fetchable task. Without the second of those, "return the
message whenever there is one" would pass the first and break progress
narration.

## 11. Six MUST requirements were never being graded, because the SUT hid them

*Fixed. Confirmed by re-run. Confidence: verified by a full suite run with the
gRPC binding enabled.*

The score in §1 was computed over the requirements the suite actually ran, and
the SUT was quietly shrinking that set. Its agent card advertised only
`JSONRPC` and `HTTP+JSON`. The TCK builds one client per advertised interface,
so the entire `GRPC-*` family — and every core requirement's gRPC leg —
reported `SKIPPED`.

`SKIPPED` looks benign next to `FAIL`. It is not: this SDK **ships a gRPC
binding**, so those checks were not inapplicable, they were unmeasured. A
green run said nothing about a third of the product.

The SUT now serves gRPC on its own listener (`SUT_GRPC_HOST`, default one port
below the HTTP one) and advertises it. The result:

| | Before | After |
|---|---|---|
| Passed | 176 | **244** |
| Skipped | 89 | **21** |
| Failed | 0 | **0** |
| MUST `SKIPPED` | 11 | **5** |

The gRPC binding passed everything on first exposure — 68/72, the other 4
skipped — so this closed no defects. That is the finding: the binding was
already right, and had simply never been asked.

**Two measurement artifacts nearly got recorded as 25 gRPC defects**, and both
are worth writing down because both looked exactly like real failures:

1. `failed to connect to all addresses … HTTP proxy returned response code`.
   `grpcio` honours `http_proxy` even for loopback, and this sandbox sets one.
   The suite reported 25 failing MUST requirements. Running with the proxy
   variables unset fixed it — and cut the run from 8 minutes to 64 seconds,
   because the "failures" were proxy timeouts.
2. `errors resolving http://127.0.0.1:9998 … Misformatted domain name`. A gRPC
   target is a name-resolver string (`host:port`), **not** a URL, and the card
   was advertising a scheme. Same 25 failures, different cause, still nothing
   to do with the binding.

Both produced a plausible-looking list of binding defects. The tell in each
case was that the *errors* were transport-establishment failures, not
assertion failures — a distinction the summary line does not make. Reading the
grouped error text before believing the count is what separated them.

**What was left, and what happened to it** — see §12.

## 12. The capability-*rejection* paths were unreachable from one SUT

*Partly closed. Confidence: verified by a second full suite run.*

After §11, five MUST requirements still reported `SKIPPED`, and three of them
were unreachable **by construction**: `CORE-CAP-001/002/004` check that a
server rejects push and streaming operations it never advertised, and the
suite skips them against an agent that *does* advertise them. No single SUT
can be on both sides of that. The gate could never have caught a regression in
`ensure_streaming_supported` / `ensure_push_supported`, because nothing
official ever called them.

The SUT now takes a `SUT_PROFILE`. `full` (default) is the profile the gate
runs against; `minimal` advertises no streaming, no push and no extended card,
which is what makes the rejection paths observable. CI runs the suite once per
profile and applies the same requirement-level gate to both.

Result on the `minimal` profile:

| Requirement | Before | After |
|---|---|---|
| `CORE-CAP-001` (push rejected when unsupported) | SKIPPED | **PASS** (both bindings) |
| `CORE-CAP-002` (streaming rejected when unsupported) | SKIPPED | **PASS** (`jsonrpc`) |
| `CORE-CAP-003` (extended card rejected when unsupported) | SKIPPED | **PASS** (`jsonrpc`) — see §15 |
| `CORE-CAP-004` | SKIPPED | SKIPPED |

### One test errors under this profile (fault since assigned — see §17)

`TestRestStreaming::test_streaming_content_type` (`HTTP_JSON-SSE-001`) errors
rather than skipping. What is established:

- **This SDK's response is correct**, captured on the wire. With streaming
  unadvertised, `POST /v1/message:stream` returns
  `400` + `{"error": {..., "reason": "UNSUPPORTED_OPERATION"}}`, and the
  JSON-RPC binding returns `-32004` with the same reason. That is exactly what
  `CORE-CAP-002` requires, and `CORE-CAP-002` passes.
- **The error is raised inside the suite's own client**, in
  `http_json_client.py::_extract_error` → `response.json()`, while the test is
  building the message for a `pytest.skip` it had already decided to take.
  The response was opened as a stream and never read.

**No claim was made about whose defect that is** when this section was
written, because the decisive test had not been run. **It has now been run —
see §17.** The answer is that this is a defect in `a2a-tck`'s own HTTP+JSON
client, reproducible with no A2A SDK on either side of the connection, and
the paragraph that used to sit here (declining to assign fault) has been
replaced by that section rather than left standing as an open question.

The requirement-level gate is unaffected — the erroring test records no
requirement result, and the gate returns 0 on the minimal-profile report.

### Still open

- ~~`CORE-CAP-004`~~ — **closed, see §18.** It is now graded `PASS` on both
  `jsonrpc` and `http_json` by a third SUT profile. The upstream constraint
  (#193) is real and still open, but it bounds *how* the requirement can be
  measured, not *whether* it can be: a scoped run measures it without any
  change to the harness.
- `CARD-EXT-002`: needs a server that *declares* `extendedAgentCard` while
  having no extended card configured. This SDK cannot enter that state — the
  handler derives the extended card from the configured agent card (verified
  directly in `handler/lifecycle/extended_card.rs`), so declaring the
  capability and having no card are mutually exclusive. The requirement is
  structurally inapplicable here rather than unmeasured.
- The 21 `NOT TESTED` MUSTs remain untouchable from here — `a2a-tck` has no
  tests for them.

`CARD-EXT-001` is **not** on this list — §13 fixed it and it passes on all
three bindings. An earlier revision of this section still listed it here as
undiagnosed after §13 shipped; that was stale documentation, not a live
finding. See §15.

## 13. `AgentCard.url` is a v0.3 field the v1.0 schema does not have

*Fixed. Confirmed by re-run. Found only because §11 and §12 made
`CARD-EXT-001` runnable at all.*

Declaring `extendedAgentCard` on the SUT (§12) let `CARD-EXT-001` execute for
the first time, and it failed on both JSON bindings while **passing on gRPC**:

```
$: 'url' does not match any of the regexes: '^(default_input_modes)$',
   '^(default_output_modes)$', '^(documentation_url)$', '^(icon_url)$',
   '^(security_requirements)$', '^(security_schemes)$',
   '^(supported_interfaces)$'
```

The split by binding is the whole diagnosis. The suite validates JSON payloads
against a schema generated from `a2a.proto`, and the v1.0 `AgentCard` has **no
`url` field** — `supported_interfaces` replaced it. gRPC passed because
protobuf physically cannot carry a field the schema does not define; the JSON
bindings emitted it and were rejected.

`url` is still **accepted**, because a card published by a v0.3 peer carries
it and refusing those cards outright would be worse than ignoring an extra
key. That is what the reference implementation does too — its resolver pops
`url` and folds it into `supportedInterfaces`. So the field is now
`#[serde(skip_serializing)]`: parsed, never emitted. Same policy as §7 —
accept both vintages, emit only v1.0.

Result: `246 passed, 0 failed, 19 skipped`, `CARD-EXT-001` PASS on all three
bindings.

**Worth noting how this was found.** It was not found by looking for it. It
surfaced because closing a *coverage* gap (§11, §12) made a requirement
runnable that had been quietly skipped since the beginning — and the skip was
caused by the SUT's own configuration, not by the suite. Two of the three
things fixed in §11–§13 were harness gaps rather than SDK defects, and the
third was only reachable through them. A conformance score is only as
trustworthy as the set of checks it was allowed to run.

## 14. §13's fix broke `a2a-inspector`'s card check — a real, external tool lag, not an a2a-rust bug

*Waived, narrowly, in CI. Not "fixed" — this is upstream's gap to close, and
this repo does not control that timeline.*

`tck.yml`'s `TCK self-test (echo-agent)` job runs an additional check beyond
the TCK conformance suite: `itk/interop/inspector_card_check.py`, a headless
reproduction of the official [a2a-inspector](https://github.com/a2aproject/a2a-inspector)'s
agent-card validation (vendored from its `backend/validators.py`, since the
inspector itself ships web-UI-only with no CLI to script directly). PR #99
turned this red:

```
a2a-inspector card validation FAILED for http://127.0.0.1:9090:
  - Required field is missing: 'url'.
```

§13 is what caused it — `AgentCard.url` stopped being emitted there, for a
verified reason (the v1.0 schema, generated from `a2a.proto`, has no `url`
field). Before changing anything, that reasoning was re-checked against two
independent, live sources rather than trusted from memory:

1. `proto/a2a_v1/a2a.proto`'s `message AgentCard` (re-read directly): still no
   `url` field. `supported_interfaces` is still what carries it.
2. `a2aproject/a2a-inspector`'s `backend/validators.py`, fetched byte-for-byte
   from its upstream `main` branch on 2026-07-29 (not from memory, not from
   the vendored copy — the live file, to rule out our copy having drifted).
   It is **identical** to what's vendored in `itk/interop/inspector_card_check.py`,
   and it unconditionally requires a top-level `url`:
   ```python
   required_fields = frozenset([
       "name", "description", "url", "version", "capabilities",
       "defaultInputModes", "defaultOutputModes", "skills",
   ])
   ```
   No fallback to `supportedInterfaces` anywhere in the file.

So this is a genuine, confirmed conflict between two authoritative-ish
sources, not a stale vendored copy and not an a2a-rust defect: **any**
strictly v1.0-schema-compliant `AgentCard` — from any SDK, not just this one
— will fail the inspector's check today, because the inspector predates the
v1.0 schema's `url` → `supported_interfaces` change and hasn't been updated
for it.

**Resolution.** `a2a-inspector card validation` in `tck.yml` is now
`continue-on-error: true`, scoped to that one step only — the TCK conformance
steps above it in the same job (22/22 on both JSON-RPC and REST) are
untouched hard gates. §13's fix is not reverted: emitting `url` again to
satisfy a lagging external tool would reopen the real JSON-schema violation
`CARD-EXT-001` and `golden_fixtures_from_official_sdk_roundtrip` both exist to
catch. This is a documented, narrowly-scoped waiver for a known, cited,
external cause — not the "make it green and move on" pattern that's
otherwise off the table for this project.

**Not done here:** no issue has been filed against `a2aproject/a2a-inspector`
by this session — that's a human call on a repo this project doesn't own.
The actual fix is upstream, whenever it lands.

## 15. The fifth `SKIPPED` MUST was never named: it is `CORE-CAP-003`, not `CARD-EXT-001`

*Resolved by live re-run, not by re-reading this document. Confidence:
verified.*

§1 has said "5 `SKIPPED`" MUST requirements since §11, but between §12 and
§13 this document drifted into naming only four of them
(`CORE-CAP-001`, `CORE-CAP-002`, `CORE-CAP-004`, `CARD-EXT-002`) while §12's
"Still open" list carried `CARD-EXT-001` as a fifth, undiagnosed entry —
even though §13, immediately below it, fixed `CARD-EXT-001` and reported it
passing on all three bindings. That is two sections disagreeing about the
same requirement, and neither pointed at the actual fifth skip.

This was re-run rather than re-derived from the existing text, per this
project's own rule that the doc is not a source of truth for its own
disputed claims. Environment: `a2a-tck` at `5996b79`
(`main`, 2026-06-29), official suite run against `tck/sut` built from this
commit, both profiles, exactly as `official-tck.yml` runs them.

**`full` profile** (`SUT_HOST=127.0.0.1:9999 SUT_GRPC_HOST=127.0.0.1:9998`):

```
246 passed, 19 skipped in 125.57s (0:02:05)
MUST: 88 passed / 0 failed / 5 skipped / 21 not tested   (of 114)
```

Reading `reports/compatibility.json` directly (not the summary table) for
every MUST-level `SKIPPED` entry gives the exact five:

```
CARD-EXT-002   {jsonrpc: SKIPPED, http_json: SKIPPED, grpc: SKIPPED}
CORE-CAP-001   {jsonrpc: SKIPPED, http_json: SKIPPED}
CORE-CAP-002   {jsonrpc: SKIPPED}
CORE-CAP-003   {jsonrpc: SKIPPED}
CORE-CAP-004   {jsonrpc: SKIPPED, http_json: SKIPPED}
```

`CARD-EXT-001` is not in this list — its status in the same report is
`PASS` on all three transports (`grpc`, `jsonrpc`, `http_json`). §13's claim
is correct and current; §12's "still open" bullet about it was simply never
updated after §13 landed. The genuine, previously-unnamed fifth skip is
**`CORE-CAP-003`** — "`GetExtendedAgentCard` MUST return
`UnsupportedOperationError` when `capabilities.extendedAgentCard` is false or
absent" (`tck/requirements/core_operations.py`). It skips on the `full`
profile for the same structural reason `CORE-CAP-001`/`002` do: that profile
*does* advertise `extendedAgentCard`, so the test's own precondition
(`if caps.get("extendedAgentCard"): pytest.skip(...)`) takes the skip branch
before asserting anything.

**`minimal` profile** (`SUT_PROFILE=minimal`, no capabilities advertised)
makes it observable, exactly as it already does for `CORE-CAP-001`/`002` —
this is not a new SUT change, the existing minimal-profile CI job already
exercises it:

```
181 passed, 83 skipped (+ 1 pytest-level error, HTTP_JSON-SSE-001 — §12, unrelated)
CORE-CAP-001   {jsonrpc: PASS, http_json: PASS}
CORE-CAP-002   {jsonrpc: PASS}
CORE-CAP-003   {jsonrpc: PASS}
CORE-CAP-004   {jsonrpc: SKIPPED, http_json: SKIPPED}
```

`CORE-CAP-003` passes cleanly. It was already being closed by the
`minimal`-profile job that exists for `CORE-CAP-001`/`002` — it just had no
row in §12's table and no name anywhere in this document, so nobody had
verified it, and the "5 skipped" count had drifted into being explained by
the wrong four-plus-one.

**While tracing `CORE-CAP-004`'s "precondition not identified" claim**, the
precondition turned out to be identifiable, not mysterious: the test requires
the agent card to declare an extension with URI
`urn:a2a:tck:required-extension` and `required: true`
(`test_error_handling.py::TestCapabilityExtensionRequired`). Neither SUT
profile's card declares any `capabilities.extensions` at all — confirmed by
fetching both live cards and grepping `tck/sut/src/main.rs` for the sentinel
URI (zero hits). The server-side enforcement this would exercise already
exists and is unit-tested (`handler/capability.rs::ensure_required_extensions`,
covered by `missing_required_extension_is_rejected` and
`get_task_enforces_required_extension`).

**This is not a drop-in SUT-config change, though — it carries real
regression risk, traced (not assumed) through `builder.rs` and
`handler/messaging.rs`.** `ensure_required_extensions` is derived once from
*every* extension the card marks `required: true`
(`builder.rs`, precomputing `required_extensions` from
`agent_card.capabilities.extensions`) and is enforced on **every**
`SendMessage`/`SendStreamingMessage` call (`handler/messaging.rs:211`), not
scoped to the one TCK test that's supposed to trigger it. Declaring the
sentinel extension as required on either existing profile's card would make
*every other* message-send in the full suite subject to the same check —
and nothing else in the suite declares
`A2A-Extensions: urn:a2a:tck:required-extension`, so it would very likely
reject every other currently-passing test that sends a message against that
profile (176–181 of them, by the counts above), not just the one test meant
to exercise it. Both existing profiles run the *entire* suite, not a
filtered subset, so this is not a theoretical concern.

Closing this safely needs a third SUT profile carrying only the sentinel
extension, plus an empirical full-suite run against it to confirm nothing
else regresses — not a one-line card edit. That is a bounded, well-specified
piece of work, but it is a task in its own right with its own verification
burden, and it was not undertaken in this pass in order to avoid touching the
gate on a guess. Recorded here, with the exact mechanism, so the next pass
does not have to re-derive it or discover the regression risk the hard way.

**Net correction to §1 and §12:** the "5" was always right; the four-named
explanation was wrong, and `CARD-EXT-001` was wrongly on the open list. §1's
`SKIPPED` row and §12's table and "Still open" list have been updated to
match this section.

## 16. What the 21 `NOT TESTED` MUSTs actually are, one family at a time

*Investigated live. Confidence: verified — every claim below is either a
direct grep of `a2a-tck`'s `tests/` tree, a read of its `requirements/*.py`
registry, or a read of its own `backlog/tasks/*.md`, not an inference from
this document's prior wording.*

§1 already says the `NOT TESTED` row can't be closed from this repo,
because the suite has no test for those IDs. That claim is correct but was
not, until now, checked against the suite's actual test tree — it was
inherited from earlier sessions. It has now been checked directly.

**Method.** `_add_untested_requirements` in `a2a-tck`'s
`tck/reporting/aggregator.py` marks a requirement `NOT TESTED` if and only if
zero pytest results reference its ID — mechanically, not based on SUT
behaviour. So the operative question per family is not "why doesn't the SUT
trigger it" (there is nothing to trigger) but "does any test function
exist for this ID at all", which is answered by grepping
`tests/compatibility/` for the literal requirement ID string. Run for all 21:

```
$ for id in <all 21 IDs>; do grep -rln "\"$id\"" tests/; done
# zero matches for every single one
```

**All 21 have zero test implementations.** None of them are a SUT
configuration gap or a product-code gap in the sense the earlier framing
assumed ("the SUT doesn't currently exercise the code path") — there is no
code path in the suite to exercise. No change to `tck/sut`, and no change to
`crates/a2a-protocol-server`, can move any of these 21 out of `NOT TESTED`
via the official TCK as it exists today. They fall into three groups, each
confirmed from a different part of the suite's own source:

**Group 1 — the suite's own authors consider them unautomatable (6
requirements).** Five carry the literal `NOT_AUTOMATABLE` tag —
`CARD-SIGN-001..004` and `AUTH-TLS-001`, verified by parsing every
`RequirementSpec` block in `tck/requirements/*.py` rather than by eye. The
sixth, `GRPC-SVC-003`, carries **no tag**; it records the same verdict as an
inline source comment instead, so it is grouped here on the strength of that
comment, not of a tag. `tck/requirements/agent_card.py` tags all four
`CARD-SIGN-*` specs `NOT_AUTOMATABLE` — they describe internal properties of
the *signing process* (JCS canonicalization before signing, excluding the
`signatures` field from the signed payload, protected-header shape, stale-key
rejection over time), not externally observable request/response behaviour a
black-box HTTP client can assert on. `AUTH-TLS-001` ("production deployments
MUST use encrypted communication") carries the same tag for a different
reason — the suite talks to whatever host it's given and has no way to know
whether that endpoint is "production". `GRPC-SVC-003` ("gRPC over HTTP/2
with TLS") isn't tagged, but carries the source comment
`# Not tested: TLS is a production deployment concern.` directly above it —
same reasoning, undeclared as a formal tag. **This SDK's `signing` feature
already covers three of the four `CARD-SIGN-*` concerns in its own test
suite**, independently of the TCK's inability to grade any of them —
checked directly in `crates/a2a-protocol-types/tests/signing_tests.rs` and
`src/signing.rs`: JCS canonicalization is covered extensively (key sorting,
whitespace, escaping, nesting, numeric formatting — `CARD-SIGN-001`), the
`signatures` field is excluded from the canonical payload with a test and a
comment saying so (`CARD-SIGN-002`), and `alg`/`kid` protected-header
presence is asserted directly (`CARD-SIGN-003`). **`CARD-SIGN-004`
("expired or revoked keys MUST NOT be used for verification") has no
equivalent coverage, and on inspection that's because the concept doesn't
exist in this module at all** — `signing.rs`'s public surface is
`canonicalize` / `canonicalize_card` / `sign_agent_card` /
`verify_agent_card`; there is no key-expiry or revocation notion anywhere in
it. That may be a reasonable layering choice (key lifecycle is arguably a
caller/JWKS-resolution concern, not the raw sign/verify primitive's job),
but it means `CARD-SIGN-004` isn't "tested elsewhere" the way the other
three are — it's simply not a capability this module has today. This
corrects this document's earlier framing that CARD-SIGN was the
highest-value, most tractable gap to close: it is not tractable via the
official TCK at all, and only 3 of its 4 sub-requirements are actually
covered by this SDK's own tests.

**That "may be a reasonable layering choice" is now a decision rather than an
open question — see §19.**

**Group 2 — ruled out of scope by the suite's own backlog (2 requirements:
`VER-CLIENT-001`, `VER-CLIENT-002`).** `backlog/archive/tasks/task-30` (this
project's own `a2a-tck` checkout, `main` at `5996b79`) carries the
implementation note: *"Won't Do: Testing the A2A client (i.e. the TCK
itself) is out of scope for TCK conformance tests."* Both requirements
describe **client**-side obligations (send an `A2A-Version` header; ignore
patch versions when negotiating) — and the TCK is architecturally a client
that only ever tests servers. Testing these would mean testing the TCK's own
HTTP client code, which its maintainers have explicitly declined to do. This
is a permanent, structural exclusion, not a backlog item awaiting
implementation.

**Group 3 — genuine, roadmapped, upstream coverage gaps (13 requirements:
`AUTH-SERVER-002`, `AUTH-INTASK-001..004`, `AUTH-SCOPE-001..003`,
`BIND-EQUIV-001..004`, `VER-SERVER-001`).** These are real backlog items,
each with an open ticket, `status: To Do`, in `a2a-tck`'s own
`backlog/tasks/`:

- `task-27` (priority medium) covers the 9 `AUTH-*` MUSTs in this group
  (plus 4 more `SHOULD`-level ones not in our 21). Its own text: *"TLS
  requirements may need a separate SUT configuration with TLS enabled.
  In-task auth and scope requirements need SUT scenarios that exercise the
  auth flow."* — i.e. even upstream's plan requires new SUT behavioural
  contracts (an agent that actually enters `TASK_STATE_AUTH_REQUIRED`, and a
  multi-identity/multi-tenant scenario for scope isolation), not just a new
  assertion against the existing SUT.
- `task-28` (priority medium) covers all 4 `BIND-EQUIV-*`. Its own text:
  *"These require cross-transport comparison tests — send the same request
  via gRPC, JSON-RPC, and HTTP+JSON and verify the responses are
  semantically equivalent. This is a different testing pattern than
  single-transport tests."* — the suite's current tests all assert against
  one transport at a time; equivalence tests are a structurally different
  shape it doesn't have yet.
- `VER-SERVER-001` was bundled into the same (archived) `task-30` as the two
  `VER-CLIENT-*` "Won't Do" items, but the "won't do" reasoning is specific
  to testing the TCK's own client and does not obviously apply to
  `VER-SERVER-001`, which is a **server**-side requirement (the agent must
  process a request using the semantics of the version it declared). Its
  ticket is archived alongside its "won't do" siblings without its own
  separate disposition recorded. This project cannot resolve that ambiguity
  unilaterally — it is upstream's ticket.

One correction of scope, found while reading `versioning.py` in full for
this section: `VER-SERVER-002` and `VER-SERVER-003` are **not** in the 21 —
both already have tests and both **pass** (`test_unsupported_version_returns_error_*`,
`test_empty_version_treated_as_default_jsonrpc`). Only 3 of the 5 `VER-*`
MUSTs are untested, not the whole family.

**Net effect on the suggested next step.** Closing any of these 21
requires writing new test code in `a2aproject/a2a-tck`, not this repository
— for Group 1, that would mean overturning an explicit upstream design
decision (unlikely to be accepted); for Group 2, the same; for Group 3, it
means picking up an existing, already-scoped upstream backlog item, which is
real, valuable work but is a contribution to someone else's project.
Per this project's own rule that outward-facing action needs a human
decision first, **no upstream issue or PR has been filed or drafted by this
session** — this section is a report of what was found, not an action taken.
`CORE-CAP-004`, which is `SKIPPED` rather than `NOT TESTED`, looked like the
one item closeable inside this repository; §12's entry for it now records
that it is blocked on upstream `a2a-tck` #193 instead.

## 17. `HTTP_JSON-SSE-001`: fault assigned, with evidence — it is the harness

*Resolved. Confidence: verified by direct experiment, reproduced with no A2A
SDK on either side of the connection.*

§12 recorded an erroring test and explicitly declined to assign fault,
because the decisive experiment had not been run. This section runs it.

**What the experiment had to establish.** §12's untested hypothesis was that
the error "would be reached by any server that returns a non-2xx to
`send_streaming_message`". If true, the defect is in the harness and nothing
about this SDK is implicated. The obvious comparator — run the reference
Python implementation with streaming disabled — turned out to be unavailable:
`a2a-tck`'s own reference SUT (`sut/a2a-python/sut_agent.py`) imports
`a2a.server.apps.A2AStarletteApplication`, which does not exist in the
published `a2a-sdk` 1.1.2 *or* in `a2aproject/a2a-python` at `main`
(cloned 2026-07-30, `b74ee55`) — the TCK's reference SUT is itself stale
against the SDK it is generated from. That is worth knowing, but it is not
the decisive test either.

**A stricter experiment was available.** The hypothesis is about the
harness's client, so the clean test removes *both* SDKs: point the suite's
own client code at a bare `http.server` that returns `400` with a JSON error
body to a streamed POST, and call the same function the failing test calls.

```
ResponseNotRead MRO: ['ResponseNotRead', 'StreamError', 'RuntimeError', ...]
caught tuple in _extract_error: (json.JSONDecodeError, ValueError)
is ResponseNotRead a ValueError?       False
is ResponseNotRead a JSONDecodeError?  False

status: 400 >= 400 -> True
RESULT: _extract_error RAISED httpx.ResponseNotRead:
        Attempted to access streaming response content, without having called `read()`.
```

**The mechanism, read off `tck/transport/http_json_client.py`.**
`_request_streaming` sends with `stream=True`, and on any status ≥ 400 calls
`response.close()` *without* reading the body. `_extract_error` then calls
`response.json()`, which raises `httpx.ResponseNotRead` — a `RuntimeError`,
so it is not caught by that function's `except (json.JSONDecodeError,
ValueError)` — and the `return f"...{response.text}"` fallback fails
identically, for the same unread-body reason. There is no path through
`_extract_error` that survives a closed, unread, non-2xx streamed response.

**Verdict.** The defect is in `a2a-tck`, not in `a2a-rust`, and not in any
SDK: the reproduction has no A2A code in it at all. Any conformant server is
exposed to it, because returning non-2xx to `message:stream` while streaming
is unadvertised is exactly what `CORE-CAP-002` *requires* — and
`CORE-CAP-002` passes here on the same profile, in the same run, which is
what makes the pairing unambiguous. This satisfies the standard the
correction notice at the top of this document sets: the claim rests on an
experiment that isolates the variable, not on one implementation disagreeing
with one client.

**What changed in CI as a result.** The minimal-profile step in
`official-tck.yml` previously carried *both* `continue-on-error: true` and
`|| true` — a blanket waiver over the whole step, which is the pattern this
document criticises elsewhere. Both are now removed. The single upstream-broken
test is excluded by name with pytest's `--deselect`, and every other check in
that profile is a hard gate again. Measured before and after: the deselected
run is `181 passed, 83 skipped, 1 deselected`, exit code 0, and the
requirement-level gate returns `MUST compatibility 100.0%`, 0 observed
failing. `CORE-CAP-001`, `CORE-CAP-002` and `CORE-CAP-003` all still report
`PASS`, so nothing the profile exists to measure was lost.

**Deselecting it costs no coverage, and that was checked rather than
asserted.** `HTTP_JSON-SSE-001` is graded **`PASS` on the `full` profile** —
by the same test id, in the gate-bearing run. The requirement is measured;
the minimal-profile invocation of that test was only ever an unhandled
harness crash, and it recorded no requirement result even before it was
deselected.

**Not done here:** no issue has been filed against `a2aproject/a2a-tck`.
A search of that repo's issues found nothing covering this defect
(the nearest, #99, is a different REST streaming failure — "Event loop is
closed"). It was reported upstream on 2026-08-07 by the maintainer as
[a2aproject/a2a-tck#225](https://github.com/a2aproject/a2a-tck/issues/225).

A complete, ready-to-file report is prepared at
[`docs/upstream/a2a-tck-sse-001-report.md`](upstream/a2a-tck-sse-001-report.md),
with the standalone reproduction at
[`docs/upstream/repro_tck_sse_bug.py`](upstream/repro_tck_sse_bug.py). It
includes a fix that was applied to a local upstream checkout and verified:
with it, the previously-erroring test skips cleanly and the server's real
error text reaches the skip message.

Every claim in that report — the line citations, the isolated reproduction,
the full-suite reproduction and the fix — was re-verified first-hand against
upstream `5996b79` on 2026-08-07, which was still `main` tip that day, before
it was filed.

## 18. `CORE-CAP-004` is closed: a scoped third profile, not a harness patch

*Fixed. Verified by run.*

This requirement sat as the last `SKIPPED` MUST that was neither structurally
inapplicable nor untested upstream. §12 recorded it as "blocked pending
[#193](https://github.com/a2aproject/a2a-tck/issues/193)". That framing was
too strong, and this section corrects it: **#193 constrains how the
requirement can be measured, not whether it can be.**

### What the test actually needs

`TestCapabilityExtensionRequired` (in
`tests/compatibility/core_operations/test_error_handling.py`) skips unless the
agent card declares `urn:a2a:tck:required-extension` with `required: true`. Given
that card, it sends an ordinary `SendMessage` carrying no `A2A-Extensions`
header and requires `ExtensionSupportRequiredError` back, on `jsonrpc` and
`http_json`. There is no gRPC variant upstream.

Neither existing profile declares any extension, so under both the `full` and
`minimal` cards the requirement records `SKIPPED` and is never graded.

### Why an unscoped run cannot work, measured rather than assumed

Required-extension enforcement is per-request (spec §3.3.4), and this SDK
implements it that way — `ensure_required_extensions` is called from
`messaging.rs`, `get_task.rs`, `list_tasks.rs`, `cancel_task.rs`,
`subscribe.rs` and `push_config.rs`. The suite does not send `A2A-Extensions`
activation on its ordinary positive requests, which is exactly what #193
reports.

That prediction was tested, not taken on faith. Running the **whole** suite
against a card declaring the required extension produces:

```
72 failed, 56 passed, 129 skipped, 8 xfailed
```

— every `CORE-SEND-*`, `CORE-MULTI-*`, error-code and status-code check
answered with `ExtensionSupportRequiredError`. Notably `CORE-CAP-004` itself
reports `PASS` even in that run, which is what made clear the SDK behaviour
was never the problem.

### The fix: a third profile, scoped

`SUT_PROFILE=extension` serves the `full` capability set plus the sentinel
extension marked `required: true`, and the suite is run with
`-k TestCapabilityExtensionRequired`. Result:

```
2 passed, 263 deselected
CORE-CAP-004  {jsonrpc: PASS, http_json: PASS}
```

Verified end to end before wiring into CI, at the SDK's own edge:

| Request | Response |
|---|---|
| `SendMessage`, no `A2A-Extensions` | `-32008` `EXTENSION_SUPPORT_REQUIRED`, naming the missing URI |
| `SendMessage`, `A2A-Extensions: urn:a2a:tck:required-extension` | normal task result |

**This is scoping, not a waiver.** Every requirement excluded from this run is
graded by the full-profile run, which is the one the baseline gate reads.
Nothing is exempted from measurement; it is measured elsewhere. The
distinction matters because upstream notes other SDKs pass this requirement
with a `sitecustomize.py` shim that monkey-patches the harness into sending
the header — that changes the suite rather than the SUT, and is not used here.

### The silent-green hole this opened, and how it is closed

A scoped run that selects **zero** tests reports zero failures, and the
differential gate accepts zero failures as success. So if upstream renamed the
test class, the `-k` filter would match nothing and the job would go green
having measured nothing — the precise failure mode
`tck/scripts/check_conformance.py` exists to prevent.

`--require-pass REQ_ID` closes it: the gate now fails unless the named
requirement is graded `PASS`, treating absent, `SKIPPED` and `NOT TESTED`
alike as unmet. Verified in both directions — exit 0 against the scoped
extension report, exit 1 against the full-profile report where `CORE-CAP-004`
is `SKIPPED`.

### Also fixed here: the report-overwrite trap, in CI

Every profile run writes the same `reports/compatibility.json`. The workflow
ran full → minimal → extension, so the uploaded artifact's
`compatibility.json` was whichever profile ran last, presented as if it were
the gate-bearing full-profile report. The full run now copies its report to
`/tmp/full-compatibility.json` immediately, the gate reads that copy, and all
three per-profile reports are uploaded under distinct names.

### Corrected while doing this

`tck/sut/src/main.rs`'s `Profile` doc comment claimed the minimal profile
makes `CORE-CAP-001/002/004` observable. Two errors in one line: `CORE-CAP-004`
is `SKIPPED` under the minimal profile (that is this whole section), and
`CORE-CAP-003` — which the minimal profile genuinely does make observable — was
omitted. Checked against both runs' `compatibility.json`: the requirements the
minimal profile adds are exactly `CORE-CAP-001`, `CORE-CAP-002` and
`CORE-CAP-003`.

## 19. `CARD-SIGN-004` decided: key lifecycle is the caller's, and the API now says so

*Decided and implemented. §16 left this as "may be a reasonable layering
choice"; leaving it unstated was the actual defect.*

### What the spec requires, quoted rather than paraphrased

Spec §8.4.3 opens by naming who is bound: **"Clients verifying Agent Card
signatures MUST:"**, then lists six steps. Step 2 is *"Retrieve the public key
using the `kid` and `jku` (or from a trusted key store)"*. Among the security
considerations that follow is the `CARD-SIGN-004` sentence: *"Expired or
revoked keys **MUST NOT** be used for verification."*

Read in place, the obligation attaches to **step 2 — key retrieval** — not to
the signature check. There is no third state: a key is either one the verifier
was entitled to use or it is not, and that is settled before any curve
arithmetic happens.

### The decision

`verify_agent_card` implements **steps 3–6** and takes the public key as a
parameter. Steps 1–2, including lifecycle policy, belong to the caller.
Recorded here rather than merely believed, because the alternative was
considered and rejected on concrete grounds:

* A JWKS key is revoked by *removal from the `jku` endpoint*. Detecting that
  requires re-fetching over HTTPS. `a2a-protocol-types` has **no network
  dependency** — verified this session, its entire dependency list is `serde`,
  `serde_json`, `base64`, `ring`, `prost`, `prost-types`, `time` — and it is
  the shared wire-type crate everything else depends on. An HTTP client does
  not belong in it.
* An X.509 key expires per `notAfter` and is revoked per CRL/OCSP. Supporting
  that means an `x5c` chain, a trust store and a revocation fetcher, i.e.
  reimplementing `webpki`/`rustls` inside a serialization crate.

Both belong to the layer that already owns transport and trust policy. Note
that this SDK *does* already do JWKS-with-`kid` resolution — in
`a2a-protocol-server`'s `auth/jwt.rs`, for inbound request authentication.
That is the correct layer for it; agent-card key retrieval would sit there
too, not in the types crate.

### Why documenting alone would have been a dodge, and what was done instead

A caller told "retrieve the key using `kid` and `jku`" **could not do it**:
those fields live base64url-encoded inside `AgentCardSignature.protected`, and
nothing in the public API decoded them. `sign_agent_card` could *write* a
`kid`; nothing could *read* one back. So the obligation was assigned to a
caller who had no supported means of discharging it — the documentation would
have been true and useless.

Added: `signing::signature_header(&AgentCardSignature) -> A2aResult<SignatureHeader>`,
exposing `alg`, `kid` and `jku`. Additive, no new dependencies, and it makes
step 2 performable. `verify_agent_card` now reads its own `alg` through it,
so there is one header-parsing path rather than two.

The doc comment on `verify_agent_card` states the split, quotes the MUST NOT,
gives the four-step caller recipe, and warns that the header is attacker-
controlled until verification succeeds — so `jku` must never be trusted on the
card's own word.

### Known limitation, stated rather than left to be discovered

`sign_agent_card` does **not** emit `jku`; it writes only `alg` and an optional
`kid`. Cards signed by this SDK are therefore verifiable via a trusted key
store — which §8.4.3 explicitly permits ("or from a trusted key store") — but
they do not advertise where their JWKS lives. This is a spec-permitted subset,
not a violation, and it is not fixed here because changing `sign_agent_card`'s
signature is a breaking API change that belongs in a deliberate release.
`signature_header` reads `jku` from cards signed by anything else, so
verification interop is unaffected.

### Coverage

Five tests added to `signing.rs`: `alg`/`kid` round-trip, absent `kid`, `jku`
read back from the spec's own §8.4.2 example header, malformed input rejected
in three shapes (bad base64, non-JSON, missing `alg`), and a non-ES256 `alg`
refused rather than silently verified. `cargo test -p a2a-protocol-types
--features signing --lib signing`: 22 passed, 0 failed.

This does **not** make `CARD-SIGN-004` gradeable by the TCK — it carries the
suite's `not-automatable` tag and has no test function upstream, so it remains
in the 21 `NOT TESTED`. It removes the gap in *this SDK*, which is the part
that was ours to close.
