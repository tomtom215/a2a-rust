<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0015: A tamper-evident audit trail, per tenant

**Date:** 2026-10-08
**Status:** Accepted
**Author:** Tom F.

---

## Context

A provider of a high-risk AI system must give it "the automatic recording of
events (logs) over the lifetime of the system" (EU AI Act, Regulation (EU)
2024/1689, Article 12(1)), events that let risk situations, post-market
monitoring and deployer monitoring be traced (12(2)), and keep them at least
six months (Articles 19(1), 26(6)). Post-market monitoring must include "an
analysis of the interaction with other AI systems" (72(2)). None of this
binds this SDK — it is not an AI system or a provider — but every agent
built on it that falls under those articles needs those records, and the
SDK is the only layer that sees every agent-to-agent call.

What the SDK had (inventory, 2026-10-08):

- the task event log of ADR 0012: what happened to each task, durable and
  replayable, but with columns `task_id, seq, payload, created_at` only — no
  caller, no authentication scheme, nothing tamper-evident;
- OpenTelemetry spans and metrics (ADR 0013): sampled, exported and expired
  by the backend, so not a record anyone can keep for six months and show
  is complete.

No A2A SDK, official or community, ships an audit trail of any kind, by a
source survey of the six official SDKs and the community crates on
2026-10-08 (made by a sub-agent reading each repository; its per-SDK
findings were not all re-checked by hand).

## Decision

### What is recorded

One record per:

| Kind | Written when | By |
|---|---|---|
| `call` | an RPC ends — succeeded, failed, refused by authentication, or dropped | an interceptor's `on_complete`, installed first in the chain |
| `call.started` | an RPC is admitted, only when the log is *required* | the same interceptor's `before` |
| `run.started` | an executor starts a run of a task | the handler, inside the executor's future, before the executor runs |
| `task.event` | an event is written to the task's event log | a `TaskStore` wrapper, after the wrapped store accepts it |
| `task.cancel_requested` | `CancelTask` signals a running task | the cancel handler |

Each carries the actor (authenticated subject and scheme), the W3C trace and
span ids, and the ids the kind concerns. Content — the message, each part,
each event — is recorded as `sha256:<hex>` over its RFC 8785 canonical JSON,
never verbatim: the record proves what was said without becoming a second
copy of it (GDPR Articles 5(1)(c) and 25(1)).

**Attribution.** An event is the agent's, not a caller's. It is attributable
to whoever started the run that emitted it, so each `task.event` names that
run's `run.started` by `runSeq`, and that record names the actor. The link is
held in an in-process registry, bounded at 65,536 runs and cleared when a
task reaches a terminal state.

### How records are made tamper-evident

- **A hash chain per tenant.** Each record carries `prev`, the hash of the
  record before it, and `hash`, SHA-256 over its own canonical JSON without
  `hash`. Changing, inserting, deleting or reordering a record breaks the
  chain at that point. Tenants get separate chains so one tenant's export
  reveals nothing about another's.
- **Signed checkpoints.** A chain alone cannot show that its tail was not
  cut off. Every *n* records, and on demand, the log signs a checkpoint —
  "chain *c* had reached `seq` *s* with hash *h* at time *t*" — with ES256
  or Ed25519 (`ring`, JWS over JCS, the shape agent-card signatures already
  use). A verifier holding a checkpoint at *s* refuses records that end
  before *s*; what lies after the newest checkpoint is reported as unsigned.
- **Anchors.** Retention deletes a prefix of a chain. It first signs an
  *anchor* — the hash of the last record it will delete — so the first
  remaining record can still be checked. Purge refuses to run without a
  signer: an unsigned anchor is a claim anyone with write access could make.

`a2a_protocol_types::audit::verify_chain` checks all of this and reports the
first failure. It is in the types crate so that it runs without a server —
an auditor verifies an export on their own machine with keys they hold.

### Where records are kept

`AuditStore`, with in-memory, SQLite and PostgreSQL implementations. Stores
store; only `AuditLog` seals, so the chaining is right in one place. The one
guarantee a store owes is that `(chain, seq)` is written at most once: that
is what lets replicas sharing a PostgreSQL store interleave on one chain
instead of forking it (the loser of a race re-reads the head and re-seals,
up to eight times).

### Failure

By default a record that cannot be written is logged, counted
(`AuditLog::failures`) and reported to metrics as `audit_append`, and the
call goes ahead. `AuditLog::require_record(true)` makes the interceptor write
`call.started` before the handler runs and refuse the call if it cannot —
for deployments where an unrecorded action is worse than a refused one.

### Retention

`AuditRetention::six_months()` is 184 days, the longest six consecutive
calendar months. A shorter floor needs
`allowing_shorter_than_six_months`. Purge deletes nothing younger than the
floor, never a chain's newest record, and nothing from a chain under a legal
hold. Nothing runs on a timer, as with the task store's retention (ADR 0011).

## Alternatives considered

- **Add columns to the event log.** The event log serves SSE replay and is
  keyed per task; an audit record also covers calls that create no task and
  refused calls, and must be ordered per tenant, not per task. Two logs with
  two jobs.
- **Emit audit events as OpenTelemetry logs.** Delivery is best-effort and
  sampling is the backend's choice; neither gives a complete, verifiable
  record. The trace id is recorded instead, so the two can be joined.
- **Record content verbatim.** Simpler to read, and a second copy of every
  message's personal data under a six-month floor. Digests prove content
  without holding it; a deployment that must keep content keeps it in the
  task store, where the GDPR's erasure applies to one copy.
- **A Merkle tree or transparency log.** Gives compact inclusion proofs for
  one record. Nothing here needs them yet; a linear chain with checkpoints is
  verified in under 150 lines (`audit/chain.rs`) with no new dependency.

## Consequences

- **Cost, measured.** Release build, 4-vCPU Intel Xeon @ 2.10 GHz, sequential
  appends to one chain: in-memory store, median 9.2 µs per append (5 runs of
  5,000, range 9.0–10.9 µs); SQLite file store, median 272 µs (5 runs of
  1,000, range 252–280 µs). Appends to one chain are serialised, so with
  SQLite one tenant's chain takes about 3,700 records a second on this
  machine. A blocking send that runs a task to completion writes at least
  `call` + `run.started` + one `task.event` per event.
- **What tamper evidence does not cover.** Record times are the writer's
  clock, not a trusted timestamp. Whoever holds the checkpoint key can
  rewrite a chain and re-sign it; the key must live away from the store, and
  exported checkpoints off the machine. Records after the newest checkpoint
  can be cut without trace — checkpoint before export and at shutdown.
- **What is not recorded.** Events of a task served by a store without an
  event log (every bundled store has one). Attribution of events from a run
  that started before a restart, or that fell out of the registry's bound:
  those `task.event` records carry no `runSeq`. In `required` mode the
  `call.started` record is written before authentication runs, so it names
  no actor; the `call` record that follows does.
- **Personal data.** The actor's subject is personal data in most
  deployments, and a digest of short, guessable text can be reversed by
  guessing. The records are pseudonymised, not anonymised.

## Verification

- `crates/a2a-protocol-types/src/audit/tests.rs`: 21 tests. The record hash
  matches an independent implementation (Python `json` + `hashlib`); SHA-256
  matches FIPS 180-2's "abc" vector; an edit, a re-sealed edit, a deletion,
  a reordering, a foreign record, a lost start, a truncated tail, an
  untrusted key, a key under the wrong algorithm, an edited checkpoint and a
  forged anchor are each found.
- `crates/a2a-protocol-server/src/audit/tests.rs`: one store contract run
  against the in-memory, SQLite and PostgreSQL stores; 64 concurrent appends
  to one chain without a fork; lost races retried; checkpoints, purge,
  holds; and a parse of the `TaskStore` trait proving the wrapper forwards
  every method.
- `crates/a2a-protocol-server/tests/audit_trail.rs`: through a real server, a
  refused, a served and a failed call, the run that names its caller, every
  event pointing at that run, the message text absent from the chain, the
  export verifying offline, a cancel naming who asked, required mode
  refusing when the store is down, and one chain per tenant.
