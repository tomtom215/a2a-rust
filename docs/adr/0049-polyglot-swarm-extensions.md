<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0049: The swarm controls are published as a wire contract another language can implement

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0, the maintainer's to veto in review.
**Author:** Tom F.

---

## Context

Audit H19: a delegation tree with one agent written in another language loses
the swarm controls at that agent. Every control already travels in a standard
A2A slot (`Message.metadata["a2a-rust.com/<name>"]`, the URI in
`Message.extensions`, task status metadata for the failure class), so a
non-Rust peer receives it and ignores it. What crosses a non-Rust hop today:

| Control | Crosses? |
|---|---|
| W3C trace context | Yes, when the peer's stack extracts `traceparent` |
| Cancel | One hop, as a standard `CancelTask`; further only if that agent cascades |
| Lease, budget, idempotency key, failure class, approval, provenance | Received and ignored |
| Halt | Never, outside replicas sharing one halt store |
| Lineage (ADR 0034) | Did not exist |

The contract for each extension lived only in ADRs and rustdoc, written for
this SDK's users. A Go or Python author had no single place that says, in
terms of bytes on the wire, what to read, what to write and what to refuse.
And a halt refusal on JSON-RPC was `-32603 "halted: <reason>"`, which a peer
could tell from a server bug only by parsing the message.

## Options

| Option | For | Against |
|---|---|---|
| **A. Publish the contract**: one reference page with each extension's URI, metadata key, JSON shape and the MUST/SHOULD of each role; a JSON Schema and conformance vectors for lineage, checked by this SDK's tests; a machine-readable halt refusal | Small, additive, no wire break; the vectors keep the page and the code from drifting; any language can implement it | Each SDK still has to write the code |
| B. Reference middleware for a2a-python and a2a-go, in this repository | A peer gets working code | Code in two other ecosystems to maintain and release here, against SDKs that change under it; the contract it implements still has to be written first (A) |
| C. Language bindings (a `pyo3` wheel of `Delegation`) | The Rust implementation everywhere | A product of its own; ties every other language to this SDK's release train |
| D. Propose the extensions upstream | The right long-term home | Outward-facing: needs the maintainer's say-so; the drafts in `docs/upstream/` are the material |

## Decision

**A**, now; B and D after it, each when there is an adopter or the
maintainer's say-so; C is out of scope.

1. **The reference page** `book/src/reference/swarm-extensions.md` states, for
   lineage, lease, budget, idempotency, failure class and approval: the URI,
   the metadata key and where it rides, the JSON shape, what a sender writes,
   what a receiver must do, and what is lost when a hop ignores it. Lineage
   and halts are written normatively (MUST, SHOULD), since they are new in
   this release and are what H19 asks for.
2. **Lineage has a JSON Schema and vectors** in
   `crates/a2a-protocol-types/extensions/lineage-v1/`, inside the crate so
   its tests can read them in a packaged build. A test runs every vector
   through `Lineage::read` and the child rule and fails when the code and the
   vectors disagree, so the page's examples are the code's behaviour.
3. **A halt refusal is machine-readable on every binding.** Its error carries
   a `google.rpc.ErrorInfo` detail with `reason: "HALTED"` and
   `domain: "a2a-rust.com"` (not `a2a-protocol.org`: A2A defines no halt). The
   code stays `-32603` on JSON-RPC and the HTTP and gRPC statuses stay `503` /
   `UNAVAILABLE`, so nothing a client already matches on changes.
4. **What a hop in another language must do to keep a job under control**,
   which the page states as the contract:
   * carry lineage: on every message it sends while serving a task, the
     lineage of that task with `depth + 1` and `parentTaskId` set, or refuse
     to delegate past depth 16;
   * cascade cancels: cancel the tasks it started for a task that is
     cancelled, so a halt that stops a Rust parent reaches the job's leaves
     one hop at a time;
   * stop on a `HALTED` refusal: a child's refusal is not a fault to retry;
   * optionally, share the halt store: the PostgreSQL table, its key grammar
     and its `NOTIFY` channels are written out on the page, so an agent in
     any language with a PostgreSQL driver can honour root halts directly.
5. **Interop is tested where the harnesses can express it.** The Python
   (a2a-sdk) and Go (a2a-go) clients send a message carrying lineage to this
   repository's echo agent and read it back from the task, and send a lineage
   past the depth cap and see it refused: the contract is implemented by hand
   in another language and this server honours it. On JSON-RPC the refusal
   reads as `InvalidParams` in both SDKs. On HTTP+JSON it is `400
   INVALID_ARGUMENT` without an `a2a-protocol.org` `ErrorInfo` (the server adds
   one only for A2A's own codes, for every invalid-params error), which a2a-go
   reads as an internal error and a2a-python as a bare HTTP 400; that is a
   server-wide gap, recorded for its own fix, not a lineage rule. (Closed by
   the second amendment below: both SDKs now read it as invalid params on
   every binding.)

   The first Python run found that a2a-python keeps metadata in a protobuf
   `Struct` and so writes the depth `2` as `2.0`. The contract therefore says a
   receiver MUST accept a whole number written as a float, and the vectors
   include one.

## Amendment, 2026-11-05: every extension, numbers, and the shims

Built since the decision above:

* **A schema and vectors for every extension** this SDK defines on the wire,
  beside lineage's: lease, budget (grant and status), idempotency, failure
  (with a `tolerated` list: values a receiver reads but a sender must not
  write), approval (request and decision), provenance, and the hand-off marker
  (ADR 0054), which has no URI. `tests/extension_vectors.rs` reads every vector
  through the SDK's own readers and writes each valid one back; the hand-off
  vectors run through the SQL claim stores, the marker's only reader;
  `scripts/check_extension_vectors.py` checks every valid value against its
  schema. The page states each extension's receiver and sender rules.
* **Integers in metadata.** A2A's `metadata` is a protobuf `Struct`, so
  a2a-python sends every integer as a double. Every integer an extension reads
  (lease `ttlMs`, budget `remainingMs`, `meters`, `spent`, lineage `depth`) now
  goes through one reader that takes a whole double up to 2^53 and refuses a
  fraction, a negative or a value past the field's range. The Python interop
  client sends a lease and a budget and checks they are honoured.
* **The shims, as they run.** The page includes, by mdBook `#include`, the
  functions the Python and Go interop clients use to attach an extension, from
  the files the interop run executes, so the page cannot drift from them.
* **Upstream drafts, not sent:** `docs/upstream/a2a-task-lineage-halt-draft.md`
  (a parent-task link, a standard halt reason, guidance on integers) and
  `docs/upstream/a2a-standard-error-reasons-draft.md` (below).

**Errors on HTTP+JSON, first decided: unchanged** (superseded by the second
amendment below). A refusal of invalid params is
`400 INVALID_ARGUMENT` with no `a2a-protocol.org` `ErrorInfo`, which a2a-go
v2.7.0 reads as an internal error and a2a-python 1.2.3 as a bare HTTP 400.
v1.0.1 §11.6 requires that `ErrorInfo` "for A2A-specific errors", which §3.3.2
lists as the nine `-3200x` errors, and for validation errors says implementations
"SHOULD use well-known types such as `google.rpc.BadRequest`". So the spec does
not require it, and the behaviour is left as it is. Both reference SDKs do send
and read a reason for invalid params (`INVALID_PARAMS`, in `a2a-protocol.org`),
so adding it is the maintainer's call; the draft asks the A2A project to define
it.

## Amendment, 2026-11-05 (second): the standard reasons on HTTP+JSON

The lead chose to build what the first amendment left to the maintainer.

**What is sent.** On the HTTP+JSON binding (`RestDispatcher` and the axum
router, which answer through one builder, `dispatch::rest::rest_error_details`),
an error whose code is one of JSON-RPC's standard codes carries, first in
`details`, a `google.rpc.ErrorInfo` in the `a2a-protocol.org` domain whose
`reason` is the one both reference SDKs map by:

| Code | `reason` | Read by |
|---|---|---|
| `-32602` invalid params | `INVALID_PARAMS` | a2a-go v2.7.0, a2a-sdk 1.2.3 |
| `-32603` internal error | `INTERNAL_ERROR` | both |
| `-32601` method not found | `METHOD_NOT_FOUND` | both |
| `-32600` invalid request | `INVALID_REQUEST` | both |
| `-32700` parse error | `PARSE_ERROR` | a2a-go (a2a-python has no such reason) |

What the error carried before follows it: a validation error's
`google.rpc.BadRequest` (the field, where it is known), a halt's `HALTED`
(`a2a-rust.com`), a rate limit's `RetryInfo`. First, because a2a-python reads
only the first `ErrorInfo` in `details`. Not sent: on a credential refusal,
which answers `401`/`403` whatever its code (`InvalidRequest`), since both SDKs
would read the reason over the status; and on the transport-level answers that
carry no error kind (a path refused as traversal, a body or query over its
limit, a body not received in time, an unknown route). A2A-specific errors keep
exactly the `ErrorInfo` they had. JSON-RPC and gRPC are unchanged.

**Why.** v1.0.1 does not require it (§11.6 asks for an `ErrorInfo` only for the
A2A-specific errors), but it asks for structured validation errors (§11.6:
"SHOULD use well-known types such as `google.rpc.BadRequest`"), and both
official SDKs send these reasons and map by them: without one, a2a-go read our
`400` as an internal error and a2a-python as a bare HTTP 400, so a caller in
either language could not tell a refusal of its input from a fault. The
addition is purely additive for a client that reads the status or the message.

**Shown by:** `tests/http_json_error_parity.rs`
(`each_standard_error_names_its_reason_on_both_dispatchers`: a body that does
not fit, a body that is not JSON, a lineage past its cap, a halt, an A2A error
and a credential refusal, on both dispatchers, byte for byte), the builder's
unit test for the codes no HTTP+JSON request reaches, and the interop clients:
a2a-go and a2a-python now read a refused lineage and a refused lease as invalid
params on HTTP+JSON as on JSON-RPC. The parity case also found the axum router
answering a body of the wrong shape with `at line 1 column N` in its message
where `RestDispatcher` had none; it now reads the body as a value, as
`RestDispatcher` does.

## What is not built

* Middleware for a2a-python or a2a-go (option B), and language bindings
  (option C): only the shims above, which the interop run executes.
* Sending the upstream drafts (option D): the maintainer's action.
* A halt sent over the wire to another agent. A halt is an operator's
  decision, made where the operator is authenticated; carrying it agent to
  agent would make a halt something any caller could send. Across a hop that
  shares no halt store it travels as the cascade's cancels.
* `scripts/check_extension_vectors.py` as a CI gate: it needs `jsonschema`,
  which CI's script gates do not install.

## Consequences

* Another SDK can carry lineage and honour halts with nothing from this one
  but the page.
* A halt refusal's details grow by one entry; a client that matches the
  message text still works.
* The page is a contract: a change to an extension's wire shape changes the
  vectors, so its test fails until the page, the schema and the vectors agree.
