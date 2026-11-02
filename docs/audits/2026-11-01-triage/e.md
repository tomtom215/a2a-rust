<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Slice e: several replicas (E-F7, E-F8, E-F9, E-F11 to E-F26)

Baseline `4a8243c66df5ae07a54a69168ab419ac609b06e9` (confirmed with `git rev-parse HEAD`). Everything here was read, not run: no cargo, no database, no live probe. Paths are under `crates/a2a-protocol-server/`. The row details are in `e.json`, which adds two keys to each row: `replica_failure` and `production_refuse`.

## Counts (19 rows)

| Verdict | Rows |
|---|---|
| confirmed | 14: E-F7, E-F8, E-F9, E-F14, E-F15, E-F16, E-F17, E-F19, E-F20, E-F21, E-F22, E-F23, E-F24, E-F26 |
| partly_fixed | 1: E-F11 (every-start case fixed as A11 by 8a31cf7, with a test; one-time DDL still has no lock_timeout) |
| needs_decision | 4: E-F12, E-F13, E-F18, E-F25 |
| already_fixed / not_a_defect / cannot_check / duplicate | 0 |

The four conjectures are now settled by reading:

- **E-F8:** confirmed.
- **E-F9:** confirmed.
- **E-F24:** confirmed. The window is about 2 to 3 round trips, and the existing test starts after it.
- **E-F11:** partly fixed. There is a `pg_locks` probe that splits the fixed half from the residual.

Two sub-claims were misread:

- **E-F19:** the CancelTask runSeq is never set on any replica, so its absence has nothing to do with replicas.
- **E-F20:** the failure class survives, because it rides in `status.message`.

New findings: 2 (NEW-e-1 medium, NEW-e-2 low).

## The five that matter most for "replicas work without affinity on PostgreSQL"

1. **E-F13: needs_decision, raised to high.** `check_production` (`builder.rs:1599-1630`) only checks `is_none()`, and ADR 0025 allows in-memory stores on purpose.
   - **Kill switch:** on a fleet, a halt stops one pod only.
   - **Leases:** the in-memory lease store is worse. A parent's GetTask renewal that the balancer sends to another pod is a no-op (`store/lease/mod.rs:135-147`). The pod running the child then stops it while the parent is still renewing. Per TTL that happens with probability ((N-1)/N)^3: 1/8 at 2 pods, about 1/2 at 5.
   - **Recommendation:** an additive `process_local()` marker on the halt, lease, push-config and audit store traits. Production refuses them when `task_store.turn_claims().is_some()` (the signal E-F4 already uses).
   - **Caveat:** there is no SQLite lease store.
2. **NEW-e-1: medium.** A PostgreSQL outage or failover longer than the 15 s claim TTL lets the reapers, the owner's own included, end healthy quiet turns `failed`/transient when the database returns. They race the owners' renewals, which are stuck in sqlx connect backoff. Managed failovers usually take longer than 15 s, so in-flight model calls fail and get re-sent.
3. **E-F7: medium.** Renewals still share the default 10-connection task pool, with a 30 s acquire wait against the 15 s TTL. Nothing self-fences an executor cut off from the database. Since the reaper exists, a renewal stall of one TTL now ends a healthy turn automatically; before, it took a continuation.
4. **E-F8: medium, read-confirmed.** Positions are broadcast before they are appended (`in_memory.rs:696,738`), and a takeover numbers from the log end (`admission.rs:256-277`). A client resuming with Last-Event-ID, or a subscriber on the old pod, then silently skips the next turn's first events, with no `streamLagged`. Triggers: a partition past the TTL, a SIGKILL/OOM, or a drain with unappended events.
5. **E-F9: medium, read-confirmed, small fix.** Take a cross-replica cancel of an executor that returns `Ok` on its token. Neither pod pushes CANCELED: B uses a throwaway writer, and A only fires the token, so `supersede` never runs. The book says A delivers it. With round robin that is (N-1)/N of cancels. NEW-e-2 is the same path: no Canceled event reaches the event log or the audit chain.

E-F18 is next. It is a single optimistic audit chain for the fleet, with 8 attempts and no backoff. By a back-of-envelope estimate (not measured), failures appear at N=3 to 5 under load, not N≥9.

## Probes specified (none run)

- **E-F8:** two processes over toxiproxy. Cut pg_a, continue the task on N2 at 16 s, and compare the SSE `id:` payloads with `task_events`. A Last-Event-ID resume follows.
- **E-F9:** a scratch replica with `HttpPushSender::allow_private_urls()` and `PostgresPushConfigStore`. Run `hold` on A, cancel through B, and check that the webhook gets no CANCELED.
- **E-F11:** a `pg_locks` query with two runs. (A) An open INSERT plus a second start: no waiters. (B) A v0.15.0 database plus an open INSERT plus a new replica: the CREATE TRIGGER waits on ShareRowExclusive and every writer queues behind it.
- **E-F24:** a 300 ms latency toxic on B's link. Send the retry to A the moment the key row appears; it is answered as a replay, and the original is refused about 5 s later.
- **E-F7 and NEW-e-1:** pool starvation, a partition counting side effects, and both links cut for 25 s.

## What I did not do or could not settle

- I ran nothing, and no test was run.
- Two points rely on my knowledge of PostgreSQL, not a run:
  - the lock modes (CREATE TRIGGER is SHARE ROW EXCLUSIVE; `to_regclass` and regclass casts take no lock);
  - how READ COMMITTED treats E-F21's concurrent ctid DELETEs.
- The E-F18 thresholds and the E-F25 capacity cost need X17 and a 2,000-subscriber run.
- I did not trace `audit/retention.rs` purge, which is E-F18's `time_ms`-skew sub-claim.
- The SQLite variants of E-F8 and E-F9 follow from the same handler code. I did not open their SQL.
- The failure rates assume an L7 per-request balancer. Kubernetes' kube-proxy balances per connection, so clients holding keep-alive connections see fewer cross-replica turns, but still some.
