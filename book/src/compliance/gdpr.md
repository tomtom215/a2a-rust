# GDPR

If an agent you build with a2a-rust handles personal data, you, or the
organisation you build it for, will usually be its controller or a processor,
and the SDK is software you run, not a party to anything. The GDPR's
obligations are yours. This page says what the SDK
gives you toward each, with the code that does it, and where it stops.

> **Not legal advice, and not a claim of compliance.** The page follows the
> [control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md)'s
> 22 GDPR rows (Regulation (EU) 2016/679, article text read in the Official
> Journal edition on 2026-10-21). Of those, 2 are *supported*, 7 are *partial*
> and 13 are *yours by nature*. Each row cites the test that proves the SDK's
> part and says what is left.

## Where personal data can sit

| Where | What can be personal data | By default |
|---|---|---|
| Task stores (in-memory, SQLite, PostgreSQL) | Message and artifact content, task and context ids, `metadata`, the event log, the artifact journal, idempotency keys | The in-memory store evicts terminal tasks an hour after their last write and holds at most 10,000. The SQL stores keep everything until you purge |
| Push-config stores | Webhook URLs and their credentials | Delivered to `http://` webhooks unless the sender is `https_only` |
| Audit store | Who called, who approved, which operator halted or erased | Content is a digest, never text. Callers are named in the clear unless you use `AuditLog::with_privacy` |
| Spans, logs, metrics | Message content, only if you turn it on | Off. `with_span_content_capture(true)` turns it on |
| Peer agents, webhook receivers, telemetry back ends | Whatever you sent them | Outside the SDK's reach: it keeps no list of what was sent where (the audit trail records calls received) |

## What the SDK gives you, article by article

| Obligation | What the SDK provides | Map status | What is left |
|---|---|---|---|
| Data protection by design (Art. 25(1), 5(1)(c)) | Audit records hold content only as digests; spans carry no content unless you opt in; secrets are redacted from debug output; with the `encryption` feature, an audit log built with `with_privacy` seals each person an audit record names under a key of their own | Supported | The stores query ids, states, timestamps and `metadata`, so keep personal data out of those fields. The measures are yours to switch on |
| Data protection by default (Art. 25(2)) | See the table above, and caller isolation under an authenticating interceptor | Partial | SQL stores and the audit store keep everything until you purge; encryption and `with_privacy` are off; without an authenticating interceptor every caller shares every task |
| Security of processing (Art. 32(1)(a)) | TLS in transit (rustls client; the HTTP and gRPC listeners terminate TLS, mutual TLS included). `EncryptedTaskStore` seals every part of a task and its event log with AES-256-GCM, bound to its place in the store | Supported | Ids, states, timestamps, `metadata` and the order of messages stay readable. Whole messages removed or reordered, or a store rolled back, go undetected. You switch it on |
| Confidentiality, integrity, availability (Art. 5(1)(f), 32(1)(b)) | Authentication, caller and tenant isolation, sealed stores, signed content, a tamper-evident audit chain, body and stream limits | Partial | Rate limiting, the credential throttle and encryption are off until you install them |
| Storage limitation and erasure (Art. 5(1)(e), 17) | Erasure and retention calls, below | Partial | Erasure is yours to start: the A2A protocol has no method a caller can use to delete a task |
| Accountability (Art. 5(2), 24(1)) | With `audit`: every call, run and task event, each approval, halt, erasure and legal hold, each with who did it | Partial | A purge or `erase_caller` run on a concrete store is on record only if you then call `record_erasure` on the audit log |
| Access and portability (Art. 15, 20) | A caller lists and reads exactly its own tasks and webhooks; you export a tenant's audit chain | Partial | Nothing gathers what the server holds about one person |
| Accuracy and rectification (Art. 5(1)(d), 16) | A stored part that was altered fails the read; inaccurate data can be erased | Partial | Nothing corrects a stored task: the protocol has no edit method, and the event log keeps the events as first written |
| Restriction and objection (Art. 18, 21) | An interceptor can refuse a caller; tokens can be revoked; `RequestHandler::halt` stops a tenant; a legal hold keeps an audit chain from purge | Partial | No store can mark one person's data as restricted. A refusal is built per caller, and a halt acts on a whole tenant |

The rows below are yours, and the SDK offers a mechanism or evidence toward
each:

- **Legal basis, consent, purposes** (Art. 5(1)(a), 5(1)(b), 6 to 8). An
  authenticating interceptor can refuse a caller before anything runs, so you
  can check your own record of a basis or consent. The SDK records none.
- **Special categories** (Art. 9, 10). The SDK neither detects nor treats any
  category specially. If such data is routed through tasks at all, put it in
  parts, which `EncryptedTaskStore` seals, not in `metadata`.
- **Notices to people** (Art. 12 to 14). The agent card describes an agent to
  other software. It is not a privacy notice.
- **Automated decisions** (Art. 22). The [approval gate](../deployment/oversight.md)
  puts an authenticated approver between an agent and an action, and a quorum of
  more than one can be required. Which decisions need it is your judgement, and the gate
  cannot make an executor ask.
- **Telling recipients** (Art. 17(2), 19). The SDK keeps no list of recipients.
  An erasure reaches this server's stores, not a task delegated to another
  agent or a webhook that received it.
- **Processors and transfers** (Art. 28, 44 to 49). Push delivery can be limited
  to listed hosts (`HttpPushSender::with_allowed_hosts`). Whether a peer, a
  webhook receiver or a telemetry back end is a processor, and where it is, is
  yours to settle.
- **Records of processing, impact assessments, breach notification** (Art. 30,
  33 to 35). The audit trail logs operations, not the record Art. 30 describes.
  The [Security](../deployment/security.md) chapter and this map are inputs to
  your assessment, not substitutes for it. The audit chain, verified with
  `AuditLog::verify` or offline with `a2a audit verify`, shows an edited,
  deleted or reordered record, but nothing raises an alert.
- **Backup and testing** (Art. 32(1)(c), (d)). The SDK has no backup of its own.
  The SDK's own measures are tested on every pull request (fuzzing, mutation
  testing, CodeQL, OSV-Scanner), which covers the library, not your deployment.

## Erasing and retaining

Erasure is something an operator starts. What the calls do:

| Call | Reaches | Notes |
|---|---|---|
| `TaskStore::delete` | One task by id, with its event log and artifact journal | In a tenant store it touches no other tenant's |
| `purge_expired` | On the SQL stores, terminal tasks past an age, with their events and journal; expires idempotency keys | The SQL stores keep everything until you call it |
| `erase_caller` | On the tenant-aware task and push-config stores, everything one authenticated caller's partitions hold, in every tenant: tasks, events, idempotency keys, webhooks | Needs caller isolation |
| `AuditLog::purge` | Audit records past a retention floor (`AuditRetention`) | Refused under a legal hold |
| `AuditLog::erase_subject` | One person in a tenant's audit chain, by destroying their key (needs `with_privacy`) | Refused under a legal hold; the chain still verifies |

Each leaves a trail if you ask it to. The audited task store writes
`task.deleted` (the id, never the content); `AuditLog::record_erasure` records a
purge or an `erase_caller` and what it returned; `place_hold` and
`release_hold` write `hold.placed` and `hold.released` with who did it; and
`erase_subject` is recorded before the key is destroyed, so a failure part way
is still on record. [Audit Trail](../deployment/audit.md#retention-and-legal-holds)
has the retention floor, the legal holds and the erasure of a person in
detail, and [Multi-Tenancy](../deployment/multi-tenancy.md) covers erasing a
caller.

### What erasure does not reach

The map states these limits; plan around them.

- **Tasks about someone else.** The task store knows a task by the caller who
  made it, not by the people it concerns. `erase_caller` erases one caller's
  data, but a task about someone else is found only by id. Tasks in a tenant's
  shared partition (`share_tasks_across_callers`, unlabelled credentials, or
  tasks written before caller isolation) are attributed to no one.
- **Audit records.** An erased person's records stay linkable to each other by
  their common handle. Records written before protection was on cannot be
  erased (`erase_subject` reports them). A legal hold covers a tenant's whole
  chain, so it refuses every erasure there.
- **Bytes on disk.** The SQLite stores zero deleted content in the database file
  (`secure_delete`). They do not zero it in the write-ahead log, which keeps a
  page's earlier image until a checkpoint truncates it, nor in a backup or a
  copy of the file. The PostgreSQL stores use an ordinary `DELETE`, so the bytes
  stay as dead tuples until vacuumed.
- **Backups.** Restoring a backup taken before an erasure brings the data back.
  Data sealed by `EncryptedTaskStore` or `with_privacy` opens only with every
  key that sealed it, so losing a key loses the data.
- **Copies elsewhere.** Tasks delegated to other agents, webhook receivers and
  telemetry back ends are not reached.
- **Concurrency.** Across replicas, an erasure, a hold and an append are not
  serialised ([ADR 0015](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0015-audit-trail.md)).

## Where to go next

- [Compliance Overview](./overview.md): the protections that are off until you
  switch them on.
- [Security](../deployment/security.md#data-at-rest): what encryption at rest
  protects and what it leaves readable.
- [Task & Config Stores](../building-agents/stores/retention.md):
  the retention and erasure calls on each store.
- The [GDPR section of the control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md)
  has all 22 rows, with the tests that prove them.
