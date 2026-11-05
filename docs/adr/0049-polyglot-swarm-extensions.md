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
   repository's echo agent and read it back from the task, and send a
   malformed lineage and get `InvalidParams`: the contract is implemented by
   hand in another language and this server honours it.

## What is not built

* Middleware for a2a-python or a2a-go (option B), and language bindings
  (option C).
* Schemas and vectors for lease, budget, idempotency, failure and approval:
  the page states their contract in prose from the code, and lineage is the
  pattern to follow.
* Upstream proposals (option D).
* A halt sent over the wire to another agent. A halt is an operator's
  decision, made where the operator is authenticated; carrying it agent to
  agent would make a halt something any caller could send. Across a hop that
  shares no halt store it travels as the cascade's cancels.

## Consequences

* Another SDK can carry lineage and honour halts with nothing from this one
  but the page.
* A halt refusal's details grow by one entry; a client that matches the
  message text still works.
* The page is a contract: a change to an extension's wire shape changes the
  vectors, so its test fails until the page, the schema and the vectors agree.
