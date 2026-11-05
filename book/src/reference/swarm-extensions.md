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
| Lease | `Message.metadata["a2a-rust.com/lease"]`, renewed by `GetTask` | A child keeps running after its parent dies |
| Budget | `Message.metadata["a2a-rust.com/budget"]` | The caller's deadline and meters are not enforced |
| Idempotency key | `Message.metadata["a2a-rust.com/idempotency-key"]` | A retried send can start a second child |
| Failure class | The failed status message's `metadata["a2a-rust.com/failure"]` | A caller cannot tell a retryable failure from a final one |
| Approval | `metadata["a2a-rust.com/approval"]` on the asking status and the answering message | An action runs without the bound approval |

Each extension's URI is `https://a2a-rust.com/extensions/<name>/v1`. A
message that carries one SHOULD also list that URI in `Message.extensions`;
this SDK's server reads the metadata key whether or not it is listed. An
agent that honours one SHOULD declare the URI in its card's
`capabilities.extensions`. A breaking change gets a new URI.

The exact shape of lease, budget, idempotency, failure and approval is the
JSON this SDK's tests pin for each URI, in
[`crates/a2a-protocol-types/tests/golden/uri_contracts/`](https://github.com/tomtom215/a2a-rust/tree/main/crates/a2a-protocol-types/tests/golden/uri_contracts):
a sample value, the field names, the tokens and what they mean. The
behaviour is in ADR 0020 (lease), ADR 0023 (budget), ADR 0026 (idempotency)
and the [human oversight](../deployment/oversight.md) chapter (approval).
Lineage and halts are specified here in full.

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
| `depth` | integer, required | 0 to 16; 0 is the job's first task |
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
   `UNAVAILABLE`; gRPC as a status detail under `UNAVAILABLE`. A sender
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

## Where this stops

There is no middleware for a2a-python or a2a-go in this repository, no
language binding, and no upstream proposal yet (ADR 0049). The interop
harnesses check that the Python and Go SDKs' clients can carry a lineage to
this SDK's server and get the refusal of a malformed one.
