<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Operations Runbooks

Step-by-step procedures for the operations a deployment has to be able to carry out on a bad day: erasing or reporting
on one person, holding and retaining records, halting work, rotating keys, losing the audit store, restoring a
backup, and upgrading. The alert-by-alert runbooks are in [Alerts and runbooks](./observability/alerts.md); this page is
for operations someone starts on purpose.

Each step names the call that does it. The SDK has no admin endpoint and no command that runs these for you: call
them from an internal route, a job or a tool you already guard, so that who may run them is your access control's
decision. Steps marked **(record it)** are calls made outside any request, which the audit trail does not see unless
you report them.

## Before you need any of this

- Log the handler's [profile report](./production.md#choosing-a-profile) at startup
  (`handler.profile_report()`). It lists what `build()` checked, what each escape hatch gave up, and what the
  deployment declared about controls the process cannot see.
- Know, for each store, its concrete type: the erasure and retention calls are on the concrete stores
  (`TenantAwareSqliteTaskStore`, `TenantAwarePostgresTaskStore`, …), not on the `TaskStore` trait. Keep a handle
  to each one where your admin code can reach it.
- Keep the checkpoint key's public half, and every content and HMAC key ever active, somewhere other than the
  machine that uses them.
- With `Profile::RegulatedData`, write down how each declared control is met; the declarations are in the report,
  and nothing checks them.

## Erase one person

The task store knows a task by the caller who made it, the audit trail by the people its records name, and the
step journal by a run's recorded owner. Each is erased by its own call, and none reaches the others.
[GDPR](../compliance/gdpr.md#erasing-and-retaining) has the full map and what erasure does not reach.

1. **Find the identities.** A caller is named by how it authenticated: `bearer:<label>`, `api-key:<label>` or
   `jwt:<issuer>:<sub>`. List every one the person used, and every tenant they used it in.
2. **Check for a legal hold** on each tenant's audit chain (see [Legal hold](#legal-hold)). An erasure under a
   hold is refused; that is GDPR Art. 17(3)(e), and the refusal is the right outcome. Release the hold only if
   the reason for it has ended.
3. **Tasks and webhooks:** `erase_caller(identity)` on the tenant-aware task store and on the tenant-aware
   push-config store. It removes the caller's partitions in every tenant: tasks, logged events, idempotency
   keys, webhooks. The SQL stores do it in one transaction. **(record it)**: `log.record_erasure(chain, by,
   Erased::Caller { caller, partitions, rows })` with what it returned
   ([Audit Trail](./audit.md#erasure-of-tasks-is-on-record-too)).
4. **Step journal** (tool loop): `journal.runs_of(principal, Some(tenant))` to see what will go, then
   `journal.erase(principal, Some(tenant))`, for each tenant. A person named only inside a run's content is not
   found by it.
5. **Audit trail** (needs `AuditLog::with_privacy`): `log.erase_subject(chain, subject, by)` for each tenant's
   chain. It destroys the person's keys and writes a `subject.erased` record. Read the returned `Erasure`:
   `in_clear` lists the records that name the person in the clear (written before protection was on); those
   cannot be erased without breaking the chain, and an empty list is the only answer that means nothing names
   them any longer.
6. **Tasks about the person made by someone else** are found only by id: delete each with `TaskStore::delete`
   through the audited store, which records `task.deleted`.
7. **Outside the SDK:** backups, a SQLite file's write-ahead log, PostgreSQL's dead tuples until `VACUUM`,
   telemetry back ends, webhook receivers and agents that tasks were delegated to. Restoring a backup taken
   before the erasure brings the data back (see [Restore from a backup](#restore-from-a-backup)), so keep a
   record of erasures outside the databases they erased.

## Answer an access request

What the SDK gives here is partial; [GDPR](../compliance/gdpr.md#what-the-sdk-gives-you-article-by-article) grades it.

- **Tasks and webhooks:** a caller lists and reads exactly its own, through `ListTasks`, `GetTask` and the push-config
  methods, with its own credential. The SDK has no operator call that lists one caller's tasks.
- **Step journal:** `journal.runs_of(principal, tenant)` lists the person's runs (key, revision, owner, whether it
  completed, when it was last written), without their content.
- **Audit trail:** `log.export(chain)` gives the chain's records; with `with_privacy`, `log.reveal(&record)`
  opens what a record says about people, and its `erased` field names what was erased. Verify the chain first
  (`log.verify(chain)`): `reveal` does not check a record against the chain.

## Legal hold

A hold covers one tenant's whole audit chain. While it is placed, `AuditLog::purge` deletes nothing in that chain
and `erase_subject` is refused there. It does not hold tasks: `purge_expired` and `erase_caller` on the task stores
do not read it, so suspend those jobs for the tenant yourself.

1. `log.place_hold(chain, reason, by)`; it writes `hold.placed`, naming `by`.
2. Suspend the task-store retention job and any erasure for that tenant.
3. When it ends: `log.release_hold(chain, by)`, which writes `hold.released` and returns whether a hold was
   there. Resume the jobs.

A hold and a purge are serialised across replicas sharing a SQLite or PostgreSQL audit store: a hold that has
completed stops a purge that began before it.

## Retention jobs

Nothing deletes on its own. Run each of these from a scheduler you already have, outside peak traffic; each is safe to
run from several replicas.

- **Finished tasks on the SQL stores:** `store.purge_expired(&RetentionPolicy::new(max_age))`. It deletes terminal
  tasks only, with their events and journal, and parked tasks only with `with_parked_max_age`. **(record it)** with
  `Erased::Expired { tasks }`.
- **Audit records:** `log.purge(AuditRetention::six_months(), now_ms)`. It needs the checkpoint signer, because it
  signs an anchor before deleting, and it never deletes a chain's newest record or anything in a held chain.
- **Step journal:** `journal.sweep(completed_before_ms, unfinished_before_ms)`. Keep the unfinished cutoff well past
  your longest run and its approvals: a run swept while still going is refused its next write.
- **Unresolved calls** (required audit): `log.reconcile_unresolved(chain, after, older_than)`. This deletes nothing;
  it closes the calls a crash left open. Run it at startup and on a schedule; see
  [The audit store is unavailable](#the-audit-store-is-unavailable).

[Retention, erasure and audit](../building-agents/stores/retention.md) and
[Audit Trail](./audit.md#retention-and-legal-holds) have the details.

## Halt a tenant or a job

1. See what would stop: `handler.running_in_root(tenant, root)` for one job.
2. `handler.halt(HaltScope::Tenant(tenant), by, reason)`, `HaltScope::root(tenant, root)` for one job, or
   `HaltScope::All`. New sends are refused with reason `HALTED`; running tasks are cancelled; reads and cancels
   are still served. It returns a `HaltReport` with the tasks it stopped.
3. Across replicas, the handlers must share a halt store (`with_halt_store(PostgresHaltStore)`): each reads it
   every `with_halt_poll_interval` (one second by default), so a halt given on one replica stops the others
   within one interval. Without a shared store a halt reaches only the process it was given to.
4. To lift it: `handler.resume(scope, by)`.

With the audit trail, both are `halt` records naming `by`. [Human Oversight](./oversight.md#halting) covers what a
halt does to a running executor and to delegated sub-tasks. Before 0.16 is on every replica, give no root halts: see
[Upgrade from 0.15 to 0.16](#upgrade-from-015-to-016).

## Rotate keys

There are three kinds of key. None is re-sealed by the SDK: a value stays sealed under the key that sealed it, so
an old key stays in the ring for as long as anything it sealed is stored, backups included.

**Content keys** (`EncryptedTaskStore`, the SQL push-config stores, the journal cipher, and the key-encryption key of
audit privacy), all `ContentCipher`s:

1. Build the cipher with the new key active and the old ones added:
   `Aes256GcmCipher::new("2026-11", new_key)?.with_key("2026-10", old_key)?`.
2. Roll it out to every replica. Each seals under the new key from then on and opens under any key in the ring.
3. Rotate before the active key nears its seal budget: `seal_count()` is this process's count, and the bound of
   2^32 is shared by every process holding the key ([Security](./security.md#data-at-rest)).
4. Drop an old key only once nothing sealed under it is stored, including in backups you may restore. A value
   whose key is gone cannot be opened: a task part that does not open fails the whole read.

**The audit HMAC key** (`AuditPrivacy`): build it with the new key active and add the old ones,
`AuditPrivacy::new(kek, "h2", new_hmac)?.with_hmac_key("h1", old_hmac)?`. Keep each old one for as long as people's
keys made under it may exist; without it, `erase_subject` cannot find them and reports nothing erased.

**The checkpoint key:**

1. Make the new key ([Audit Trail](./audit.md#the-checkpoint-key)) under a new `kid`.
2. Sign one last checkpoint under the old key (`log.checkpoint(chain)` for each chain), then restart with
   `with_signer(new_signer, every)`.
3. Give the log the old public key: `.with_retired_key(old_public_key)`. Without it, `log.verify` reports the
   chain's earlier checkpoints as unverifiable, since each names the old `kid`.
4. Give auditors both public keys: `a2a audit verify --keys` reads a JWK Set, which can hold both.

## The audit store is unavailable

What happens depends on whether the log is required (`require_record(true)`):

- **Not required:** calls are served unrecorded. Each failed append is counted in `log.failures()` and reported as a
  persistence error on `audit_append` (alert `A2APersistenceErrors`). The records are not recovered.
- **Required:** a call whose `call.started` cannot be written is refused, so the outage of the audit store is an
  outage of the agent. Records refused after a call is admitted are held in memory and written when the store
  answers, oldest first.

Then:

1. Watch `log.pending_outcomes()` (records waiting) and `log.outcomes_lost()` (records that did not fit the
   1,024-record wait, or were refused eight times). Any non-zero `outcomes_lost` is a record that does not exist.
2. Do not restart a process with records waiting unless you accept losing them: the wait is memory.
3. When the store is back, run `log.reconcile_unresolved(chain, 0, older_than)` for every chain
   (`log.store().chains()`), with `older_than` longer than your longest request. Each call started and never
   closed gets a `call.unresolved` record. It is safe to run twice and from several replicas.
4. Verify the chains (`log.verify(chain)`).

[Audit Trail](./audit.md#required-mode) explains the mode.

## Restore from a backup

A restore is a rollback of state, and some of what the SDK guarantees is about state moving forward. Before you
restore:

1. **Keys.** Have every content and HMAC key that was active when the backup was taken, and the checkpoint public
   keys. Without them the restored data does not open.
2. **Copy SQLite files consistently.** A copy of the database file alone, taken while the process runs, can miss
   what is still in its write-ahead log. Take backups with SQLite's own backup API or `VACUUM INTO`, or with the
   process stopped.

After the restore, before serving:

3. **Re-apply erasures** made since the backup was taken, from your own record of them: the restored databases,
   the audit chain included, no longer show them.
4. **Verify each audit chain** against checkpoints kept off the machine. A restored chain that ends before a
   checkpoint you hold is reported as a tail removed, which is what happened; the records written after the
   backup are gone.
5. **Reconcile** unresolved calls (`reconcile_unresolved`), as after an audit outage.
6. **Halts** in a restored halt store are the ones in force when the backup was taken: check them against what
   should be halted now.

What follows from restoring older state, though nothing in the SDK reports it: webhook deliveries in the restored push outbox that were
sent after the backup are pending again; tasks running when the backup was taken are restored as running.

## Upgrade from 0.15 to 0.16

[Upgrading from 0.15 to 0.16](../reference/upgrading/0.15-to-0.16.md) lists every code change. For the data:

1. **Back up** every database.
2. **Replace the replicas.** On PostgreSQL, one at a time, in any order: the database keeps 0.15 and 0.16 replicas
   off each other's turns ([Upgrading a fleet](./horizontal-scaling.md#upgrading-a-fleet-to-this-release)). If the
   0.15 fleet ran with session affinity, keep it until the last 0.15 replica is gone. Give no job halt until then: a
   0.15 replica cannot read a job halt's row in a shared halt store and stops reading the store. Processes sharing
   one SQLite file are replaced together.
3. **Start 0.16.** Each task store brings its schema up to date when it is opened. `with_migrations` runs the
   numbered migrations and records them in `schema_versions`: SQLite from version 8 to 9, PostgreSQL from 7 to 9.
   `new` and `from_pool` create what is missing in place without recording a version. On SQLite the change is one
   added table (`a2a_task_claims`) and its index.
4. **Verify:** the profile report, each audit chain, and a read of known tasks.

To roll back on SQLite, stop 0.16 and start 0.15 on the same files: 0.15 ignores the added table, opens the audit
records 0.16 wrote (privacy-sealed ones included), appends to the chain, and the chain verifies. That was rehearsed with
one program built against both versions
([migration inventory](https://github.com/tomtom215/a2a-rust/blob/main/docs/audits/2026-11-13-adversarial-review/migration-inventory.md)
§4). A rollback on PostgreSQL has not been rehearsed: restore the backup from step 1 instead.
