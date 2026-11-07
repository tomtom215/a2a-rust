<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Swarm Extensions: the Wire Contract

This page is for an agent written in another language that takes part in a
delegation tree with agents built on this SDK. It says, in terms of bytes on
the wire, what each of this SDK's swarm controls is, what a hop must do to
keep it, and what is lost when a hop ignores it (ADR 0049). Nothing here
needs this SDK: every control rides in a standard A2A slot.

The words MUST, SHOULD and MAY are used as in RFC 2119.

## What crosses a hop today

| Control | Carried in | When a hop ignores it |
|---|---|---|
| Trace context | W3C `traceparent` / `tracestate` HTTP headers | The trace breaks at that hop |
| Cancel | A standard `CancelTask` | The cascade stops at that hop unless it cancels its own children |
| [Lineage](#lineage-v1) | `Message.metadata["a2a-rust.com/lineage"]` | The job's halt does not reach the tasks below it, except as the parent's cancels |
| [Halt](#halts) | Not on the wire; the refusal is | That hop keeps working for a halted job |
| [Lease](#lease-v1) | `Message.metadata["a2a-rust.com/lease"]`, renewed by `GetTask` | A child keeps running after its parent dies |
| [Budget](#budget-v1) | `Message.metadata["a2a-rust.com/budget"]` | The caller's deadline and meters are not enforced |
| [Steering](#steering-v1) | `Message.extensions` on a `SendMessage` naming a running task; the receipt in the response task's `metadata["a2a-rust.com/steering"]` | A message to a working task is refused, as before |
| [Idempotency key](#idempotency-v1) | `Message.metadata["a2a-rust.com/idempotency-key"]` | A retried send can start a second child |
| [Failure class](#failure-v1) | The failed status message's `metadata["a2a-rust.com/failure"]` | A caller cannot tell a retryable failure from a final one |
| [Approval](#approval-v1) | `metadata["a2a-rust.com/approval"]` on the asking status and the answering message | An action runs without the bound approval |
| [Provenance](#provenance-v1) | `metadata["a2a-rust.com/provenance"]` on a message or an artifact | Content loses its AI-generated marker and its signature |
| [Hand-off marker](#the-hand-off-marker) | The working status message's `metadata["a2a-rust.com/hand-off"]` | A caller does not know to send the turn again after a deploy |

Each extension's URI is `https://a2a-rust.com/extensions/<name>/v1`. A
message that carries one SHOULD also list that URI in `Message.extensions`;
this SDK's server reads the metadata key whether or not it is listed. An
agent that honours one SHOULD declare the URI in its card's
`capabilities.extensions`. A breaking change gets a new URI.

**Numbers.** `metadata` is a protobuf `Struct` in A2A, whose only number is a
double, so an SDK that keeps metadata that way (a2a-python does) writes every
integer of every extension as a float: `30000` arrives as `30000.0`. A
receiver MUST read a whole number written as a float as that integer, up to
2^53, and MUST refuse a fraction, a negative, a value past the field's range
and a float past 2^53, as invalid params. This SDK does so for every integer
it reads (lease `ttlMs`, budget `remainingMs`, `meters` and `spent`, lineage
`depth`), and the Python interop client sends each of them as a double.

**Schemas and vectors.** Each extension below has a JSON Schema and
conformance vectors in
[`crates/a2a-protocol-types/extensions/`](https://github.com/tomtom215/a2a-rust/tree/main/crates/a2a-protocol-types/extensions),
one folder per extension. A vectors file lists `valid` values (each with what
it `reads` as), `invalid` values a receiver MUST refuse as invalid params, and
for some extensions `tolerated` values a receiver accepts but a sender MUST NOT
write. This SDK's tests read every vector through its own readers and write
each valid one back (`tests/extension_vectors.rs`, and the hand-off marker's
in the server's claim stores); `scripts/check_extension_vectors.py` checks
every valid value against its schema and lists the invalid ones a schema
cannot refuse (it needs `jsonschema` and is not yet a CI gate). Build against
the files, not against this page: they are what the tests hold the SDK to. The JSON this SDK sends for each URI
is also pinned in
[`tests/golden/uri_contracts/`](https://github.com/tomtom215/a2a-rust/tree/main/crates/a2a-protocol-types/tests/golden/uri_contracts).

## Lineage v1

URI `https://a2a-rust.com/extensions/lineage/v1`; metadata key
`a2a-rust.com/lineage`; ADR 0034. A lineage names the **job** a task
belongs to and where in the job's tree it is.

```json
{
  "metadata": {
    "a2a-rust.com/lineage": { "root": "job-42", "depth": 2, "parentTaskId": "t-9" }
  },
  "extensions": ["https://a2a-rust.com/extensions/lineage/v1"]
}
```

| Member | Type | Rule |
|---|---|---|
| `root` | string, required | 1 to 1024 bytes of UTF-8, no control character (U+0000 to U+001F, U+007F to U+009F) |
| `depth` | integer, required | 0 to 16; 0 is the job's first task. `2.0` is `2` (see Numbers above) |
| `parentTaskId` | string, optional | As `root`; the delegating task's id on its own agent. Absent at depth 0 |

Unknown members MUST be ignored. The JSON Schema is
[`extensions/lineage-v1/schema.json`](https://github.com/tomtom215/a2a-rust/blob/main/crates/a2a-protocol-types/extensions/lineage-v1/schema.json);
it cannot express the limit in bytes, so a receiver also checks the length
in bytes. The conformance vectors,
[`extensions/lineage-v1/vectors.json`](https://github.com/tomtom215/a2a-rust/blob/main/crates/a2a-protocol-types/extensions/lineage-v1/vectors.json),
list values to accept (with what they read as, and the child each makes) and
values to refuse; this SDK's tests run every one.

**A receiver (the server of a task):**

1. MUST refuse a message whose lineage is malformed or past a bound, as
   invalid params (JSON-RPC `-32602`), before any work.
2. For a **new task**: the task's lineage is the message's, or, when the
   message carries none, `{"root": <the task's contextId>, "depth": 0}`. It
   SHOULD record a carried lineage in `Task.metadata` under the same key, so a
   tree can be built from tasks; it SHOULD NOT write one for a message that
   carried none.
3. For a **continuation**: the task keeps the lineage it was created with. A
   message naming a different `root` MUST be refused as invalid params.
4. MUST treat the lineage as the caller's claim: it scopes halts and labels
   telemetry, and MUST NOT authorize anything.

**A sender (an agent delegating while it serves a task):** on every message
it sends to start a child, it SHOULD attach its own task's lineage with
`depth` plus one and `parentTaskId` set to its own task's id, keeping
`root`. At depth 16 it MUST NOT delegate further with a lineage.

**A hop in another language that does nothing else** still keeps the job
under control if it carries the lineage and cascades cancels (below).

## Halts

A halt is an operator's decision, taken on an authenticated call to one
agent's own code. It is not sent from agent to agent. It reaches the rest of
a job in three ways.

1. **The refusal.** A send refused by a halt is answered on every binding
   with an `ErrorInfo` detail:

   ```json
   {
     "@type": "type.googleapis.com/google.rpc.ErrorInfo",
     "reason": "HALTED",
     "domain": "a2a-rust.com"
   }
   ```

   JSON-RPC carries it in `error.data` (code `-32603`, message
   `halted: <reason>`); HTTP+JSON in `error.details` under HTTP `503`, status
   `UNAVAILABLE`, after the `a2a-protocol.org` `ErrorInfo` `INTERNAL_ERROR`
   that the official SDKs map by; gRPC as a status detail under `UNAVAILABLE`.
   So a receiver MUST look for it among all `ErrorInfo` details, not only the
   first. A sender
   receiving it SHOULD stop the work that sent it and SHOULD NOT retry it as a
   transient failure.
2. **The cascade.** A halted task is cancelled. Its agent cancels the tasks
   it delegated (this SDK's `Delegation` does). A hop in another language
   MUST cancel the tasks it started for a task that is cancelled, or the
   cascade ends there.
3. **The shared halt store.** Agents that share a PostgreSQL database can
   follow halts directly. The table is

   ```sql
   CREATE TABLE IF NOT EXISTS a2a_halts (
       scope      TEXT PRIMARY KEY,
       reason     TEXT NOT NULL,
       halted_by  TEXT NOT NULL,
       halted_at  BIGINT NOT NULL   -- milliseconds since the Unix epoch
   );
   ```

   with one row per halt. `scope` is `all` (everything), `tenant:<tenant>`
   (one tenant; `""` is a server without tenants), or
   `root:<n>:<tenant><root>` for one job, where `<n>` is the tenant's length
   in bytes in decimal, with no sign or leading zero: the job `root` `job-42`
   of tenant `acme` is `root:4:acmejob-42`. A row whose `scope` names nothing
   above MUST be read as a halt of everything. A change is announced with
   `NOTIFY a2a_halts` (empty payload: read the table), and a cancel of a task
   running on another replica with `NOTIFY a2a_cancels`, payload
   `{"tenant": "...", "task": "..."}`. An agent following the table MUST
   refuse the sends and stop the running tasks of every halted scope, and
   poll the table (this SDK polls every second) even when it listens. A table
   it cannot read MUST NOT be taken as empty: refuse sends until the first
   read succeeds, and keep the halts last read after that.

A halt never stops a task parked at `input-required` or `auth-required`;
the send that would continue it is refused.

## Lease v1

URI `https://a2a-rust.com/extensions/lease/v1`; metadata key
`a2a-rust.com/lease` on the delegating send; ADR 0020. Files: `lease-v1/`.

```json
{ "a2a-rust.com/lease": { "ttlMs": 30000 } }
```

* **Receiver.** It MUST refuse a lease outside its own range as invalid
  params (this SDK: 1 000 ms to its configured maximum, one hour by default).
  It stops the task, which ends `TASK_STATE_CANCELED`, when `ttlMs` passes
  without a renewal.
* **Renewal.** A `GetTask` of the task whose request activates the URI (the
  `A2A-Extensions` service parameter, or its header) moves the deadline to
  now + `ttlMs`. A renewal that also activates
  `https://a2a-rust.com/extensions/lease/v1/renewal-only` is answered with the
  task's id, context and status only (ADR 0044).
* **Sender.** It renews at least once per `ttlMs` while it waits for the
  child, and stops renewing when it stops caring, which is how a dead parent's
  children are stopped.

## Budget v1

URI `https://a2a-rust.com/extensions/budget/v1`; metadata key
`a2a-rust.com/budget`; ADR 0023. Files: `budget-v1/grant.*` (on the
delegating send) and `budget-v1/status.*` (on status updates).

```json
{ "a2a-rust.com/budget": { "remainingMs": 30000, "meters": { "tokens": 50000 } } }
```

* A grant names `remainingMs`, at least one meter, or both. A meter name is 1
  to 64 of `[a-z0-9._-]`; at most 16 meters.
* **Receiver.** Its deadline is the admission time plus `remainingMs`; it
  MUST refuse `remainingMs` 0 as invalid params. When the deadline passes the
  turn ends `TASK_STATE_FAILED` with failure class `budget-exhausted`. On
  every status update of the turn it writes what is left and spent
  (`remainingMs`, `meters`, `spent`) under the same key.
* **Sender delegating further.** It grants a child no more than what it has
  left.

## Idempotency v1

URI `https://a2a-rust.com/extensions/idempotency/v1`; metadata key
`a2a-rust.com/idempotency-key` on a `SendMessage` or
`SendStreamingMessage`; the value is the key itself. Files:
`idempotency-v1/`.

* A key is 16 to 255 of ASCII letters, digits and `- _ . :`. It MUST be
  unguessable (16 or more random bytes): within a tenant it is the handle to
  the task it created.
* **Receiver.** A second send with the same key in the same tenant returns
  the task the first created, in whatever state it reached, and runs nothing.
  It honours a key whether or not the URI is listed in `Message.extensions`.

## Steering v1

URI `https://a2a-rust.com/extensions/steering/v1`; receipt metadata key
`a2a-rust.com/steering` on the response's task; ADR 0036. A message to a task
that is still `working`, which A2A does not define.

```json
{
  "message": {
    "messageId": "m-2", "role": "ROLE_USER", "taskId": "t-1",
    "parts": [{ "text": "skip the retail segment" }],
    "extensions": ["https://a2a-rust.com/extensions/steering/v1"]
  }
}
```

* **Sender.** MUST name the task in `taskId` and list the URI in
  `Message.extensions`; nothing else makes the message steering. SHOULD give
  it an idempotency key (a retry then delivers once).
* **Receiver** (an agent that declares the URI). A message that names a
  running task (`working`, or `submitted` with a turn running it) and lists the
  URI MUST NOT start a turn: it is put in that turn's inbox, and the response is
  the task as it stands with `{"delivery": "inbox" | "handed-off" | "replayed"}`
  in its `metadata` under the key. The receipt says where the message went, not
  that the executor read it. A message to a task that is not running is not
  steering (parked: continued; finished: refused) and its response carries no
  receipt. A full inbox is `503` with `Retry-After` (JSON-RPC: `-32603` with
  `RetryInfo`); a turn that ended before delivery is `503`; a message over the
  agent's size limit is `413`. An unrecognised `delivery` reads as accepted.
* **Authority.** The sender's over the task and no more: it does not cancel,
  approve or lift a budget.
* **A peer that does not declare the URI** refuses the message as it refuses
  any message to a working task.

## Failure v1

URI `https://a2a-rust.com/extensions/failure/v1`; metadata key
`a2a-rust.com/failure` on the failed task's status message; the value is a
class token. Files: `failure-v1/`.

| Class | Retry the same request? | Needs a person? |
|---|---|---|
| `transient` | yes | no |
| `invalid-request` | no | no |
| `policy-refusal` | no | yes |
| `budget-exhausted` | not with the same budget | no |
| `internal` | no | no |

* **Reader.** It MUST read an unrecognised token as `internal`, so that a
  peer classifying more finely never has its failures become unreadable. A
  value that is not a string means no class: the task's state still says it
  failed. Nothing is refused.
* The same token MAY ride in a JSON-RPC error's `data` object under the same
  key.

## Approval v1

URI `https://a2a-rust.com/extensions/approval/v1`; metadata key
`a2a-rust.com/approval`. Files: `approval-v1/request.*` and
`approval-v1/decision.*`.

* **The request** is on the status message of a task at `input-required`:
  `requestId`, a one-line `summary`, and `digest`, `sha256:` and 64 lowercase
  hex digits over the RFC 8785 canonical JSON of the exact action. The server
  sets `requestedBy` to the authenticated caller whose run asks.
* **The decision** is on the message that continues the task: `requestId`,
  `decision` (`approve` or `deny`), the `digest` the person was shown, and an
  optional `comment`. The receiver MUST check that the digest equals the
  request's, and that the decision comes from someone allowed to make it,
  before acting.

## Provenance v1

URI `https://a2a-rust.com/extensions/provenance/v1`; metadata key
`a2a-rust.com/provenance` on a message or an artifact. Files:
`provenance-v1/`.

* `aiGenerated` (an empty object reads as `false`), an optional `generator`,
  and an optional `signature`: a detached-payload JWS (`protected`,
  `signature`, base64url) over the RFC 8785 canonical JSON of the whole message
  or artifact with only `signature` removed, ES256 or EdDSA, verified against
  the signer's JWK Set by `kid`.

## The hand-off marker

No URI: a server writes it; metadata key `a2a-rust.com/hand-off` on the
`TASK_STATE_WORKING` status message of a turn its shutdown handed back to the
caller (ADR 0054). Files: `hand-off/`.

* Only the JSON boolean `true` marks a hand-off; any other value does not.
* **Caller.** It sends the turn's message again, to any replica, within 600
  seconds. After that the task ends `TASK_STATE_FAILED`, class `transient`.

## Carrying the extensions from another SDK

These are the shims the interop run executes against this SDK's server
(`itk/interop/`), included here from those files. Attaching an extension is
setting a metadata key and listing the URI; the rest is each extension's
rules above.

a2a-python (`python_client_vs_rust.py`):

```python
{{#include ../../../itk/interop/python_client_vs_rust.py:propagate}}
```

a2a-python, any extension (`extension_numbers.py`):

```python
{{#include ../../../itk/interop/extension_numbers.py:carrying}}
```

a2a-go (`go-sdk-client/lineage.go`):

```go
{{#include ../../../itk/interop/go-sdk-client/lineage.go:propagate}}
```

A hop that forwards a task MUST also carry the lineage onward (its own task's
lineage, `depth` plus one, `parentTaskId` set) and cascade its cancels, as
the [halts](#halts) section says.

## Where this stops

There is no middleware for a2a-python or a2a-go in this repository and no
language binding: the shims above are what is shown to run. The interop
harnesses check that the Python and Go SDKs' clients carry a lineage to this
SDK's server and get the refusal of a malformed one, and that the Python SDK's
leases and budgets, whose numbers it sends as doubles, are honoured. Draft
upstream proposals (a parent-task link and a halt reason in A2A, and the
error-reason gap described there) are in
[`docs/upstream/swarm-proposals.md`](https://github.com/tomtom215/a2a-rust/blob/main/docs/upstream/swarm-proposals.md);
nothing has been sent.
