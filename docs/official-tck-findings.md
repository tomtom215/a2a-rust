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
**System Under Test:** `tck/sut` (implements the TCK's `messageId`-keyed
behaviour contract, mirroring its reference SUT at `sut/a2a-python/sut_agent.py`)

## Score

| Run | Passed | Failed | Skipped |
|---|---|---|---|
| First run, against `examples/echo-agent` | 128 | 12 | 125 |
| Against `tck/sut`, before fixes | 87 | 15 | 157 |
| **Current** (`tck/sut`, after the fixes below) | **158** | **12** | 94 |

The echo agent scores misleadingly well because it skips rather than fails
most content assertions — it advertises fewer capabilities, so the suite asks
it less. `tck/sut` is the honest baseline.

## Fixed as a result

### 1. Unsupported `Content-Type` returned the wrong JSON-RPC error code

*Found by `JSONRPC-SSE-002`. Fixed.*

The JSON-RPC binding rejected an unsupported media type with
`ParseError (-32700)`. Spec §5.4 maps it to
`ContentTypeNotSupportedError (-32005)`. The body is never parsed in this
path, so "parse error" both misreported the cause and withheld the
machine-readable `CONTENT_TYPE_NOT_SUPPORTED` reason that §10.6 requires.

Routing it through `error_response` instead of `parse_error_response` also
attaches the `google.rpc.ErrorInfo` detail, matching every other A2A error on
this binding.

Notably, **three in-repo tests asserted the wrong behaviour** and passed
confidently:
`jsonrpc_rejects_wrong_content_type`, `jsonrpc_unsupported_content_type`, and
`unsupported_content_type_returns_parse_error`. A test suite that encodes a
bug validates it forever; this is the concrete argument for grading against
someone else's ruler.

### 2. The task state machine rejected conformant agents

*Found by `CORE-SEND-001`, `CORE-SEND-003`, `CORE-EXECUTION-MODE-001`,
`CORE-MULTI-001a`, `CORE-MULTI-002a`, `CORE-MULTI-003` — six MUST-level
checks. Fixed.*

`TaskState::can_transition_to` required `Submitted → Working` before any
finish state, so an agent that completed a task in one step got
`InvalidParams: invalid state transition ... TASK_STATE_SUBMITTED →
TASK_STATE_COMPLETED` **from its own SDK**.

That restriction is not in the specification. §4.1.3 enumerates the states and
classifies them as terminal or interrupted; it defines no transition matrix
and never requires an intermediate state. The reference SDKs agree — the
TCK's own SUT contract completes and requests input directly from
`Submitted`.

The table now enforces only what the spec supports:

1. Terminal states are final.
2. Nothing re-enters `Submitted` (the entry state) or the proto-default
   `Unspecified`; `Unspecified` as a *source* stays unconstrained.

`Working → Rejected` is now allowed too: §4.1.3 says an agent may reject
"later once an agent has determined it can't or won't proceed", not only at
creation.

`crates/a2a-protocol-server/tests/state_validation_tests.rs` was rewritten to
pin all 81 cells of the matrix against an independently-computed predicate.

## Upstream: a bug in the TCK itself

### `PUSH-CREATE-001` — the JSON-RPC client sends snake_case params

*Not an SDK defect. Should be reported to `a2aproject/a2a-tck`.*

Spec §5.5 is unambiguous:

> All JSON serializations of the A2A protocol data model **MUST** use
> camelCase naming for field names, not the snake_case convention used in
> Protocol Buffer definitions.

`tck/transport/jsonrpc_client.py` sends the Protocol Buffer spelling:

```python
def create_push_notification_config(self, task_id: str, config: dict):
    params = {"task_id": task_id, **config}      # MUST be "taskId"
```

and its `_build_params` helper only drops `None`s — it does not convert case —
so `page_size`, `page_token`, `history_length`, `status_timestamp_after`,
`include_artifacts`, and `context_id` all go out snake_case too.

Verified directly against this server:

| Params sent | Result |
|---|---|
| `{"taskId": …, "id": …, "url": …}` | `200` — config created |
| `{"task_id": …, "id": …, "url": …}` | `-32602 taskId is required` |

The corroborating detail: **`PUSH-CREATE-001` fails on `jsonrpc` but passes on
`http_json`**, because the REST client puts the task id in the path instead of
the body. Same requirement, same server, different result — the variable is
the client's field naming.

This is worth fixing upstream beyond this SDK: the optional snake_case params
(`page_size`, `history_length`, …) are silently ignored by any conformant
server, so the TCK's pagination and filtering assertions may be passing
vacuously against *every* implementation.

## Open — under investigation

Not yet classified as SDK defect vs. SUT gap vs. harness bug. Tracked here
rather than quietly skipped.

| Requirement | Symptom | First read |
|---|---|---|
| `DM-MSG-001` (×2) | `tck-message-response` returns a Task, not a bare `Message` | Likely a SUT gap: writing `StreamResponse::Message` to the event queue may not be the way this SDK returns a message-instead-of-task. Needs an API check. |
| `PUSH-DELIVER-001/002/003` (×6) | No webhook delivery observed within the timeout | Partly downstream of the snake_case bug on `jsonrpc`; the `http_json` legs need separate investigation — possibly a real delivery defect. |
| `CORE-HIST-002` | `GetTask` with `historyLength=1` returned 2 messages | Reads like a genuine SDK bug (§3.2.4 history-length semantics). |
| `STREAM-SUB-002` (×2) | Subscribe stream closes without a terminal-state event as its last frame | Reads like a genuine SDK bug (§3.1.6), possibly interacting with the snapshot-then-EOF resubscribe path added in 0.7.0. |

Two of these four look like real defects. They are listed rather than fixed
because each needs the same evidence standard applied to the two above —
reproduce by hand, check the spec text, then change behaviour.

## Running it locally

```sh
cargo build --release -p a2a-tck-sut
SUT_HOST=127.0.0.1:9999 ./target/release/a2a-tck-sut &

git clone --depth 1 https://github.com/a2aproject/a2a-tck /tmp/a2a-tck
cd /tmp/a2a-tck && uv venv && uv pip install -e .
./.venv/bin/python run_tck.py --sut-host http://127.0.0.1:9999
```

`--level must` restricts the run to hard conformance requirements. Reports
land in `reports/` as HTML, JSON, and JUnit XML.
