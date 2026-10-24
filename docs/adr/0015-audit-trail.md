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
  guessing. The records are pseudonymised, not anonymised. The 2026-10-16
  amendment below protects both, opt-in.

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

## Amendment, 2026-10-16: who and what, protected; a person erased

**Why.** The decision above leaves two things readable to anyone who can
read the audit store: who made each call (`actor.subject`, personal data in
most deployments), and, by guessing, what a short message said (an unsalted
SHA-256 digest confirms a guess). And it gives no way to erase one person:
deleting a record from the middle of a chain breaks it. The control map
recorded all three as gaps (GDPR-25-1, GDPR-32-1a, GDPR-5-1e). The
maintainer approved closing them on 2026-10-16, crypto-shredding included.

**Decision.** `AuditLog::with_privacy(AuditPrivacy)` (features `audit` and
`encryption`) changes each record inside `append`, before it is hashed:

- `actor.subject` and `detail.requestedBy` are sealed under the named
  person's own key, and `detail.reason` under the actor's: the string
  `a2a-sealed:v1:<sid>:<base64>`, AES-256-GCM under a key derived for that
  value alone (HKDF-SHA-256 from the person's key), with
  `["a2a-audit-sealed/v1", chain, sid, field, seq]` as HKDF `info` and as
  associated data. A key per value means the SP 800-38D random-nonce budget
  is never shared across a person's values.
- Every digest becomes `hmac-sha256:<kid>:<hex>`, HMAC-SHA-256 under an
  operator key over `["a2a-audit-digest/v1", chain, seq, name, sha256]`.
  Bound to `seq`, so the same content in two records carries two tags: a
  reader of the store cannot confirm a guess even by sending the guessed
  content themselves and comparing.
- A person's key is 32 random bytes, kept by the audit store wrapped by an
  operator key-encryption key (bound to the key's row, lookup included),
  found by an HMAC of the person, and named in records only by a random
  `sid`.
- `AuditLog::erase_subject(chain, subject, by)` deletes it. Under the
  chain's lock, refused while a legal hold is on the chain (GDPR
  Art. 17(3)(e)), it first writes `subject.erased` (naming the `sid`s and,
  sealed, `by`), then deletes, so a failure part way is retried rather than
  lost. It reports each record that names the person unsealed, written
  before protection was on, which it cannot reach.
- Retention deletes, under each chain's lock, the subject keys no remaining
  record names.
- Erasure of task data is recorded (2026-10-24): `task.deleted` from the
  audited task store, `erasure` from `AuditLog::record_erasure` for the
  operator's `purge_expired` and `erase_caller` (the erased caller in
  `erasedCaller`, sealed under that caller's key), and `hold.placed` and
  `hold.released` naming the person. `place_hold` and `release_hold` take
  `by`. All four kinds are new in 0.15 and extend `a2a-audit/1` as record
  kinds do: the hash covers the kind, and a verifier that does not know one
  still verifies the chain.

The chain's hashes are taken over the protected values, so `verify_chain`
needs no key, before or after an erasure. `AuditLog::reveal` opens a record
and `AuditLog::digest_matches` checks content against it, for whoever holds
the keys.

**The schema stays `a2a-audit/1`.** A protected value is a self-describing
string in a field the schema already has. A new schema string would make
every verifier built before this amendment refuse every protected record as
an unknown schema; this way an export still verifies with any verifier of
this ADR, which sees opaque strings.

**Alternatives considered.** A keyed hash of the subject in place of
encryption: an auditor could then only test a named candidate, not ask who
acted, which is what the logs of Articles 12 and 26(6) are for. Deriving
each person's key from one master key: no single person could be erased
without destroying the master. Encrypting whole records: the chain could
then be verified only with the key, and an auditor's offline check needs
none.

**Cost, measured.** Release build with LTO off (`CARGO_PROFILE_RELEASE_LTO=off`,
16 codegen units; the disk could not hold the LTO build), 4-vCPU Intel Xeon
@ 2.10 GHz, sequential appends to one chain over 50 people, medians of five
runs (`audit::privacy::tests::cost`, ignored by default): in-memory store
17.6 µs plain against 47.2 µs protected (ranges 17.5–23.3 and 39.7–51.7);
SQLite file store 296 µs against 521 µs (ranges 276–339 and 477–571).
Protection adds a key lookup and up to three seals per record.

**What it does not do.** It does not unlink: an erased person's records keep
their common `sid`, one pseudonymous activity whose times, methods and ids
other data can tie back to them. It does not reach copies of a key outside
the live table (a database's write-ahead log or free pages until
overwritten, PostgreSQL's dead tuples until `VACUUM`, replicas, backups);
the SQLite store overwrites the freed page and truncates its log unless a
reader holds it, and warns when one does. The key-encryption key unwraps
any such copy. It finds a person's key only under an HMAC key still in the
ring. Across replicas, an erasure, a legal hold and an append are not
serialised. Records keep trace ids, task and message ids, times and states
in clear. An independent review on 2026-10-16 found the first version's
digests deterministic, its erasure lost on a failure part way, its keys
never purged, its key rows exchangeable and its claims of unlinking
overstated; each is fixed above or stated here.
