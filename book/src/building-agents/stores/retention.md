<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Retention, erasure and audit

What the stores keep, how to delete what you no longer may keep, and what the audit trail records about it.

The in-memory store forgets: terminal tasks expire after `task_ttl` (one hour by
default). The SQL stores keep every task until you say otherwise. Call
`purge_expired(&RetentionPolicy::new(max_age))` on a concrete store from your
own scheduler; it deletes only `Completed`, `Failed`, `Canceled` and `Rejected`
tasks, in batches, and is safe to run from several replicas
([Running More Than One Replica](../../deployment/horizontal-scaling.md)).

The tenant-aware stores also have `erase_caller(identity)`, which removes a
caller's partitions with their tasks, logged events and idempotency keys (the
push-config stores, its webhooks); the SQL stores do it in one transaction.
`migrate_caller_keys(map)` (SQLite and PostgreSQL, task and push-config stores)
moves partitions stored under the bare caller names earlier releases used to
`bearer:<label>`, `api-key:<label>` or `jwt:<issuer>:<sub>`; run it once per
store before serving 0.15
([Upgrading](../../reference/upgrading/0.14-to-0.15/authentication.md#a-caller-is-named-by-how-it-authenticated)).

With `audit`, a task deleted through the audited task store is recorded in the
hash chain. `purge_expired` and `erase_caller` are calls you make outside any
request, so you report what they returned yourself
([Audit Trail](../../deployment/audit.md#erasure-of-tasks-is-on-record-too)). With
`encryption`, `store::encrypted::EncryptedTaskStore` seals the parts of a
task's history, status message and artifacts before they reach the wrapped
store.
