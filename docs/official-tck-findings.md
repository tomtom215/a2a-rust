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

These numbers are **not** directly comparable to one another. The echo agent
advertises fewer capabilities, so the suite asks it less and *skips* where it
would otherwise fail — its 128 is not a better result than the SUT's 87. Only
the second and third rows share a subject and are comparable; that pair is the
real before/after.

Identical results were obtained with the SUT behind a recording proxy
(§3.2), confirming the proxy did not perturb the run.

**All 12 remaining failures are at `MUST` level.** An earlier revision of this
document listed them without saying so, which understated them — the
MUST-only run is `12 failed, 139 passed`, i.e. every failure is a hard
conformance requirement, not a `SHOULD` or `MAY`. Reported MUST compatibility
is **85.4%**, computed over tested requirements only; 21 further MUST
requirements (the `CARD-SIGN-*`, `AUTH-*`, `VER-*`, and `BIND-EQUIV-*`
families) report `NOT TESTED` because the SUT does not exercise them, and are
a coverage gap rather than a pass.

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

Transport granularity matters because `PUSH-CREATE-001` fails on `jsonrpc` and
passes on `http_json` today; a requirement-level baseline would not notice it
starting to fail on `http_json` too. The baseline currently holds **16
(requirement, transport) pairs across 12 requirements**.

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

*Open. Not yet fixed.*

The reference rejects unknown fields (§3.1). This SDK ignores them, and for
filter parameters that converts a client-side mistake into **silently wrong
data** rather than an error:

| `ListTasks` params | Result |
|---|---|
| *(none)* | 50 tasks |
| `{"contextId": "<nonexistent>"}` | **0 tasks** — correct |
| `{"context_id": "<nonexistent>"}` | **50 tasks** — filter ignored |
| `{"contextID": "<nonexistent>"}` | **50 tasks** — filter ignored |
| `{"contxtId": "<nonexistent>"}` | **50 tasks** — filter ignored |
| `{"totallyBogusField": 1}` | 50 tasks — ignored |
| `{"pageSize": 1}` | 1 task — correct |
| `{"pagesize": 1}` | 50 tasks — ignored |

A caller who asks for one context's tasks and misspells the key by any means
receives **every** task instead of an error. The snake_case spelling the TCK
sends is one instance of this; a typo is another, and neither is reported.

The same class of bug is loud rather than silent where the field is required:
`CreateTaskPushNotificationConfig` with `task_id` fails with
`-32602 taskId is required`, which is at least visible.

**Scope:** 41 multi-word public fields across the request-facing types
(`params.rs` 16, `agent_card.rs` 14, `events.rs` 5, `message.rs` 4,
`push.rs` 1, `task.rs` 1) currently accept only the camelCase spelling.

**Fix direction, not yet applied** — two changes, and they are separable:

1. *Accept the proto field name as an alias* on every request-facing field,
   matching the reference's ProtoJSON acceptance. Mechanical, but the alias
   list must be generated from `proto/a2a_v1/a2a.proto` rather than from the
   Rust field names, and pinned by a test asserting both spellings
   deserialize identically for every field in the schema.
2. *Reject unknown fields* rather than ignoring them, matching the
   reference's `ParseError`. This is the change that actually prevents the
   silent-wrong-data case, and it is a deliberate semantic decision with a
   forward-compatibility cost — the types are `#[non_exhaustive]` precisely
   so the spec can grow. Worth doing, worth discussing first.

Neither is applied here, because doing them properly means generating the
field list from the schema and testing every field — not fixing the six
spellings the TCK happens to exercise.

## 4. `CORE-HIST-002` is the §3.3 bug, not a `historyLength` defect

*Earlier called a probable genuine defect. Retracted.*

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

All `MUST` level, all baselined in `tck/conformance-baseline.json`. Listed
rather than diagnosed: no claim is made about cause.

| Requirement | Symptom | Status |
|---|---|---|
| `DM-MSG-001` (×2) | `tck-message-response` yields a Task, not a bare `Message` | Unknown. Likely a SUT gap — writing `StreamResponse::Message` to the event queue may not be how this SDK returns a message-instead-of-task. Needs an API review before any claim. |
| `PUSH-CREATE-001` (jsonrpc) | `task_id` rejected | Explained by §3.3. Fixed when §3.3 is. |
| `PUSH-DELIVER-001/002/003` (×6) | No webhook delivery observed | Unknown. The `jsonrpc` legs follow from `PUSH-CREATE-001`; the `http_json` legs do not and need separate investigation. |
| `STREAM-SUB-002` (×2) | Subscribe stream closes without a terminal-state final frame | Unknown. Needs a hand-built reproduction against §3.1.6 before it is called a defect. |

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
