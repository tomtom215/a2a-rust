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

These numbers are **not** directly comparable to one another. The echo agent
advertises fewer capabilities, so the suite asks it less and *skips* where it
would otherwise fail — its 128 is not a better result than the SUT's 87. Only
the second and third rows share a subject and are comparable; that pair is the
real before/after.

Identical results were obtained with the SUT behind a recording proxy
(§3.2), confirming the proxy did not perturb the run.

**Both remaining failures are at `MUST` level**, as were the 12 before them.
An earlier revision of this document listed them without saying so, which
understated them — every failure is a hard conformance requirement, not a
`SHOULD` or `MAY`. Reported MUST compatibility is **98.8%** (85.4% → 93.9%
after §3.3 → 97.6% after §8 → 98.8% after §10), computed over tested
requirements only; 21
further MUST requirements (the `CARD-SIGN-*`, `AUTH-*`, `VER-*`, and
`BIND-EQUIV-*` families) report `NOT TESTED` because the SUT does not
exercise them, and are a coverage gap rather than a pass — **not** progress
toward 100%.

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
noticed it starting to fail on `http_json` too. The baseline now holds **2
(requirement, transport) pairs across 1 requirement**, down from 16 across 12.

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

## 5. Still open — genuinely unclassified

All `MUST` level, all baselined in `tck/conformance-baseline.json` (2 pairs
across 1 requirement).

| Requirement | Symptom | Status |
|---|---|---|
| `STREAM-SUB-002` (×2) | Subscribe stream closes without a terminal-state final frame | **Diagnosed — genuine defect. See §9.** Confidence: verified by hand reproduction plus the spec text. |


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
# SUT
cargo build --release -p a2a-tck-sut
SUT_HOST=127.0.0.1:9999 ./target/release/a2a-tck-sut &

# Official suite
git clone --depth 1 https://github.com/a2aproject/a2a-tck /tmp/a2a-tck
cd /tmp/a2a-tck && uv venv && uv pip install -e .
./.venv/bin/python run_tck.py --sut-host http://127.0.0.1:9999

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

## 9. `STREAM-SUB-002`: the subscribe stream ends with the executor, not with the task

*Open. Diagnosed, not yet fixed.*

*Confidence: verified by hand reproduction plus the spec text; root cause
located in the code but no fix attempted.*

Spec §3.1.6 is unambiguous:

> The stream MUST terminate when the task reaches a terminal state
> (`completed`, `failed`, `canceled`, or `rejected`).

and §3.5.2 adds:

> The task lifecycle is independent of any individual stream's lifecycle.

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

The stream closes **while the task is still non-terminal**, before the
transition it exists to report. That is a violation of §3.1.6 on its face.

**Root cause.** The event queue's lifetime is tied to an *executor
invocation*, not to the task. In `handler/messaging.rs` the spawned executor
ends with `drop(writer)` and `event_queue_manager.destroy(&task_id)`,
unconditionally — including when the executor deliberately leaves the task in
a non-terminal interrupted state such as `input_required` (spec §4.1.3). So:

1. `SubscribeToTask` on such a task finds no queue and falls back to
   `InMemoryQueueReader::snapshot_then_end` — snapshot, then EOF.
2. A subscriber attached *before* the executor finished would fare no better:
   the channel closes at executor exit regardless.
3. The later `SendMessage` that completes the task creates a *new* queue,
   which the earlier subscriber is not attached to.

Fixing this means decoupling queue lifetime from executor-invocation lifetime:
keep the queue alive while the task is non-terminal and close it at the
terminal transition.

**The hazard that makes this non-trivial**, found while scoping the fix and
recorded here so the next attempt does not walk into it: `send_message_inner`
currently *rejects* a send whose task already has a queue —

```rust
crate::streaming::QueueLease::Existing => {
    return Err(ServerError::UnsupportedOperation(format!(
        "task {task_id} is already being processed; wait for it to reach \
         input-required or a terminal state before sending again"
    )));
}
```

That guard is correct today precisely *because* a queue implies a live
executor. Make queues outlive executors and the implication breaks: every
`input_required` continuation — the single most common multi-turn flow, and
the one the TCK's own reproduction uses — would find `Existing` and be
rejected. So the fix is not "stop calling `destroy`". It is at minimum:

1. queue lifetime tied to task non-terminality, closed at the terminal
   transition rather than at executor exit;
2. `QueueLease` distinguishing *a queue exists* from *an executor is running*,
   so a continuation **reuses** the queue (attaching a fresh persistence
   channel) instead of being rejected;
3. an eviction story for tasks parked non-terminal indefinitely, interacting
   with `max_concurrent_queues`, plus the shutdown path in `handler/shutdown.rs`.

That is a redesign of the streaming core with real concurrency risk, not a
patch, and it is deliberately left for its own commit rather than bolted onto
a serde change.

`resubscribe_nonterminal_no_queue_returns_snapshot_then_eof` in
`handler/lifecycle/subscribe.rs` currently *asserts* the non-conformant
behaviour, citing §3.5.2 reconnection. That test encodes the bug and will need
to change with the fix — the same trap §2.1 records, where three in-repo tests
asserted the wrong JSON-RPC error code and passed confidently.

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
