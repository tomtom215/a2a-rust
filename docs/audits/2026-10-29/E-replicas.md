# E: multi-replica audit (2..N replicas, round-robin LB, no affinity)

Tree: `/home/user/a2a-rust` at `8dc4331`. Read-only audit. Nothing was built or run.
Paths are relative to `crates/a2a-protocol-server/` unless they say otherwise.
**KNOWN** marks something `docs/handoff.md` ("Now", "Open engineering") or the book already states.
"Verified" means I read the code path end to end. "Conjecture" means the effect follows from the code but has not been observed.

## 1. Component table

| Component | Shared or per-process | What goes wrong with N replicas |
|---|---|---|
| Task store (PG/SQLite) | Shared (DB) | Works. The readiness probe is `SELECT COUNT(*) FROM tasks`, a full count (F5). |
| Event log (`task_events`) | Shared | Works, but a position can be reused after a takeover (F8). On a non-owner, the terminal frame can be synthesized without its metadata (F20). |
| Live event queues / SSE fan-out | Per-process | KNOWN: a non-owner polls every 250 ms. There is no idle bound by default, so each idle stream costs 8 queries/s, on the same pool that renews claims (F7, F25). |
| Turn claims | Shared (DB clock on PG) | Mostly correct. Renewal shares the 10-connection task pool, and sqlx's 30 s acquire timeout is longer than the 15 s TTL. There is no local self-fence (F7). Nothing ever finishes a dead owner's task (F1). |
| Cancel | Store write is shared; the executor signal arrives at the owner's next renewal | KNOWN: about 1 s. The Canceled webhook may never be sent (F9). |
| Push-config store | Shared only if configured. **Default is in-memory, even with a SQL task store** | A config registered on B is invisible to A, which runs the task, so no push is sent (F4). |
| Push delivery | Per-process, inline in the owner's processor, no outbox | No duplicates in steady state. Lost if the owner dies (F1). The claim is held through webhook delivery, so continuations on other replicas are refused (F3). |
| Push per-task cap | Per-process keyed lock | The cap can be overshot permanently (F14). |
| Idempotency keys | Shared (DB) | Works. One small replay window remains (F24). A retry after the owner dies replays a WORKING task forever (F1). |
| Rate limit | Per-process by default; shared with `PostgresRateLimitCounter` | KNOWN: N× without a shared counter, and it falls back to local counting when the counter is down. New: windows and sweeps use each replica's clock (F16). |
| Auth-failure throttle | Per-process, with no shared option | N× the brute-force budget (F15). |
| Tenant `max_concurrent_tasks` | Per-process semaphore | N× the limit, and the book does not say so (F12). |
| Tenant `max_total_tasks` | In-memory tenant store only | Not applicable to PG. |
| Approval quorum | In task metadata, counted under the claim | Fixed (KNOWN, `fa2785eb`). |
| Halt / kill switch | Per-process unless `with_halt_store(PostgresHaltStore)` | KNOWN, but `Profile::Production` accepts `InMemoryHaltStore` (F13). |
| Leases | Per-process unless `PostgresLeaseStore` | KNOWN. Deadlines use replica wall clocks (F17). The Production profile accepts in-memory (F13). |
| Budgets | Per-request | No cross-replica state. The claim wait counts against the deadline (fine). |
| Audit log | Shared store, optimistic append, 8 attempts | Every replica contends on one chain. N ≥ 9 replicas under load fails appends. `time_ms` comes from the local clock. The run registry is per-process (F18, F19). |
| Retention sweeper | User-scheduled, DB clock | Safe to run concurrently, but the batches overlap rather than being "disjoint", and `complete=true` can be reported early (F21). |
| Claim sweeper | Every renewer, every 600 renewals | Idempotent. Harmless. |
| Agent card ETag / Last-Modified | ETag is a content hash; Last-Modified is the process start time | Last-Modified differs per replica. ES256 card signatures are randomized, so a card signed at boot gets a different ETag per replica. Hot reload skews between replicas (F22). |
| Content-signing keys | User config, per replica | Configuration hazard only. |
| Encryption nonce budget | Per-process | **KNOWN** (handoff, Open engineering). |
| Metrics `service.instance.id` | Random per process unless set | Commented out in `deployment.yaml`, so every restart starts a new series. Low. |
| Schema/migrations | Advisory schema lock, `statement_timeout=0` | Startup DDL takes table locks (F11). Functions are `CREATE OR REPLACE`d on every start, so a rollback or old pod reverts fixes (F10). |
| Pools | 10 per store per replica | deploy-agent uses 20 per replica; a fully configured replica up to ~41. PG's default 97 usable connections run out at N=5 (deploy-agent) or N=3 (fully configured) (F6). |
| Shutdown | Per-process | In-flight turns are CANCELED after 5 s rather than handed off. deploy-agent keeps accepting, with readiness at 200, for about 15 s after SIGTERM (F2). |
| Per-context lock | Per-process | A send waiting up to 5 s for a remote claim holds the context lock (F23). |

## 2. Findings

### F1. High: a dead or stalled owner's task stays WORKING forever, and nothing reaps it
- **Where:** `src/handler/claims/mod.rs:41-43`; `src/store/claim/mod.rs:29-33`; `src/handler/event_processing/background/processor.rs:252-279` (`lose_claim`); `src/handler/messaging/idempotency.rs:90-93,120-138`; `tests/replica_processes.rs:262-279`.
- **Evidence:**
  - The module docs say: "once its claims expire, the next send for one of its tasks takes the claim over and continues the task."
  - `lose_claim`: "nothing is recorded or pushed, because the task is not finished and what happens to it next is the other replica's to record and push."
  - The kill test names the task `orphan`, and it only recovers it by sending `"finish"` again from the client.
  - Nothing scans for non-terminal tasks with no live claim (grep for reap/orphan/abandon finds nothing).
  - An idempotent retry gets `IdempotencyClaim::Replay(existing) => self.await_claimed_task(&existing)`, which returns the stored task as-is.
- **Failure scenarios** (each verified by reading):
  1. The owner is OOM-killed, its node is lost, or it gets SIGKILL during a turn. The task stays WORKING. GetTask through the LB shows WORKING forever. Subscribers on other replicas poll forever (`subscribe_max_idle: None`, `limits/mod.rs:301`). No terminal webhook is ever sent. A client retry with the same idempotency key gets the WORKING snapshot back and the original message is never re-executed.
  2. Shutdown runs past the grace period: `release_turn_claims` (`handler/shutdown/mod.rs:129,234`) drains claims while an executor that ignores its token is still running. Its later terminal write is fenced (ClaimLost) and `lose_claim` stores nothing, so the task is left WORKING.
  3. A claim row vanishes even though no one took over: a PG failover with async replication lag, or `sweep` removing a claim that expired more than an hour ago without a takeover. The owner's next write is fenced and its executor is cancelled, leaving a WORKING orphan.
- **Book claim this contradicts:** horizontal-scaling "A replica that dies mid-turn holds its tasks — For one claim time to live (15 s)". That is only true if a client sends again.
- **Fix:** add a reaper on every replica. Periodically, under the task advisory lock, select non-terminal tasks that have no live claim and whose `updated_at` is older than TTL plus margin. Either mark them `Failed` (failure class `interrupted`, logging a terminal event and pushing it) or re-enqueue them. In `lose_claim`, when `holds()` shows no other token exists, write `Failed` instead of nothing. Bound subscriber idle by default.
- **Status:** verified by reading. Experiments X1 and X2.

### F2. High: rolling update and scale-down cancel in-flight turns, and deploy-agent keeps taking (and cancelling) new sends for about 15 s after SIGTERM
- **Where:**
  - `examples/deploy-agent/src/main.rs:283-296`
  - `src/handler/shutdown/in_flight.rs:166-181`
  - `src/handler/messaging/terminal.rs:141-159`
  - `examples/deploy-agent/src/main.rs:196-206` (`/readyz`)
  - `src/handler/introspection.rs:84-88`
  - `examples/deploy-agent/README.md:122` ("A task still running after 5 s is cancelled")
- **Evidence:**
  - `axum::serve(listener, router).with_graceful_shutdown(async move { shutdown_signal().await; tasks.finish_in_flight(5s, 10s).await; ... })`. axum only stops accepting when this future resolves, so new connections are accepted for up to 15 s.
  - `finish_in_flight` docs: "Requests that arrive during the window, on connections still open, are admitted as usual and cancelled with the rest when it closes." Tokens of tasks admitted after shutdown begins are "born cancelled" (`in_flight.rs:18-24`).
  - `finish_run`: when `stopped && !self.terminal_written()`, it calls `cancel_with`, which writes Canceled.
  - `/readyz` only calls `task_store_health()`, so it stays 200 during the drain. There is no preStop hook in `deployment.yaml`.
- **Failure scenario:** on `kubectl rollout restart` or an HPA scale-down:
  - Every turn longer than 5 s on the terminating pod ends CANCELED (terminal), even though N-1 healthy replicas could run it.
  - New sends that the Service or HAProxy still routes to the pod during the 15 s window (readiness is still green, HTTP keep-alive connections are reused) are accepted and then CANCELED.
  - The SDK's own `serve_with_shutdown` does drop the listener first (`serve/graceful/mod.rs:373`), but readiness still does not flip, and keep-alive requests are still admitted.
  - With claims, the work could be handed over instead of being terminally cancelled.
- **Status:** the 5 s cancellation is documented in the deploy-agent README. Accepting and cancelling new sends, the green readiness, and the lack of handoff are not stated. Verified by reading.
- **Fix:**
  - On SIGTERM, flip readiness to 503 first and stop admitting `SendMessage`, returning a retryable 503 or `Overloaded` rather than admitting and cancelling.
  - Then wait the LB's propagation delay. In deploy-agent, drop the listener before `finish_in_flight`.
  - For long turns, offer "release, don't cancel": release the claim and leave the task non-terminal for another replica to resume. That needs F1's reaper or a resume protocol.
  - Add a `preStop` sleep to `deployment.yaml`.
- **Experiments:** X3, X4.

### F3. Medium-High: the owner holds the turn claim until its webhook deliveries finish, so continuations on other replicas are refused while the same send on the owner succeeds
- **Where:**
  - `src/handler/event_processing/background/mod.rs:91-92`: `// Held to this processor's last write`, then `let _claim = claim;`, for the processor's whole loop.
  - `state_machine.rs:214-225`: `on_persisted()` and then the inline `deliver_push_bg(...).await`.
  - `push_delivery/mod.rs:63-98`.
  - In-process path: `messaging/execute.rs:331-341` (`writer.settled().await; ... turn.finished.cancel();`) and `streaming/event_queue/in_memory.rs:280-296`. `settled` waits only for the events to be *stored*.
  - Claim wait bound: `executor_drain_timeout` = 5 s (`limits/mod.rs:294`).
- **Failure scenario:**
  - Turn 1 on A parks at input-required.
  - A's processor now delivers the input-required push. The default `HttpPushSender` schedule is up to 38 s, bounded by `push_delivery_timeout` = 30 s.
  - The client answers at once and the LB sends it to B. B waits 5 s for A's claim, then refuses with `UnsupportedOperation "already being processed"`.
  - On A itself, `wait_for_parked_turn` resumes as soon as the events are stored, so the same continuation would have been admitted.
  - With round-robin, about (N-1)/N of continuations hit this whenever a webhook is slower than 5 s, or down and timing out.
- **Fix:** release the claim at `settled` (the last event stored, including the terminal log append), not at the end of the processor. Move push delivery out of the claim's lifetime, or deliver from a durable outbox (which also fixes push loss in F1).
- **Status:** verified by reading. Experiment X5.

### F4. Medium (High for anyone using push): the default push-config store stays in-memory when the task store is SQL; build() is silent and the book snippet omits it
- **Where:** `src/builder.rs:1362-1373`; `builder.rs:143-145`; `book/src/deployment/horizontal-scaling/shared-stores.md` (first code block wires only `.with_task_store(store)`).
- **Evidence:** `self.push_config_store.unwrap_or_else(|| { if task_store.isolates_tenants() { Arc::new(TenantAwareInMemoryPushConfigStore::new()) } else { Arc::new(InMemoryPushConfigStore::new()) } })`. Nothing checks whether `task_store.turn_claims().is_some()`, which is a signal that the store is shared.
- **Failure scenario:** the client creates a push config through B (or inline in a send that lands on B). The turn runs on A, which reads its own empty store, so no webhook is sent. `GetTaskPushNotificationConfig` via A returns not found and `List` is inconsistent. deploy-agent wires `PostgresPushConfigStore` (main.rs:177-181), but it also has no push sender, so the path is never exercised there.
- **Fix:** when the task store reports turn claims (shared) and no push-config store is given, have `build()` refuse or warn when a push sender is set, or default to a SQL push-config store on the same pool. Fix the book snippet.
- **Status:** verified by reading. Experiment X6.

### F5. Medium-High: readiness runs a full COUNT(*), so all replicas go NotReady together, and a read-only database still reports ready
- **Where:**
  - `src/handler/introspection.rs:84-88`
  - `src/store/postgres_store/store_impl.rs:717-721`: `sqlx::query_as("SELECT COUNT(*) FROM tasks")`
  - Tenant variant: `tenant_postgres_store/store_impl.rs:475`
  - `book/src/deployment/health-checks.md:51-53`: "which every bundled store answers with a cheap query ... so a read-only replica ... still reports ready"
- **Failure scenario:**
  - The tasks table grows forever by default (sustained-load.md: 65k rows/minute in the soak; no purge unless scheduled).
  - In PostgreSQL, `COUNT(*)` is O(rows). Once it exceeds the probe's `timeoutSeconds` (k8s default 1 s, not set in `deployment.yaml`), every replica fails readiness at the same moment and the Service has no endpoints: a full outage caused by the probe. N replicas × 1 full scan per 5 s also adds steady load on the database.
  - Separately, a `DATABASE_URL` that resolves to a hot standby (or a demoted primary after failover) passes readiness while every write fails with `25006`.
- **Fix:** probe with `SELECT 1`, or `SELECT 1 FROM tasks LIMIT 1`, plus `SELECT pg_is_in_recovery()` / `SHOW transaction_read_only`, all bounded by a short timeout. Keep `count()` for metrics only.
- **Status:** the code path is verified by reading. The timing threshold is conjecture. Experiments X7, X8.

### F6. Medium: pool size × replicas runs out of PostgreSQL's default `max_connections`
- **Where:**
  - `store/postgres_store/pool.rs:31` (`max_connections(10)`)
  - `push/postgres_config_store.rs:58` (10)
  - `rate_limit/shared.rs:201-210` (sqlx default 10)
  - `pool.rs:22` audit (10)
  - The halt store listener holds one more connection (`store/halt/postgres.rs` docs)
  - `examples/deploy-agent/src/main.rs:177-181` opens two pools
- **Arithmetic:**
  - deploy-agent uses 20 per replica. With the default `max_connections=100` minus `superuser_reserved_connections=3`, 97 are usable, so N=5 already exceeds it.
  - A Deployment's default `maxSurge` of 25% adds a pod during rollout, so 4 replicas plus 1 surge pod is already 5 pods.
  - A replica with task, push, rate-limit and audit pools plus the halt listener needs about 41, so N=3 exceeds it.
- **Failure scenario:** under load, new connections fail with `53300 too_many_connections`. `pg_retry::connection_lost` does not treat 53300 as retryable, so statements fail. Claim renewals also fail, which can lead to F7 takeovers.
- **Fix:** document the per-replica connection budget. Let every PG store share one pool by default (`from_pool`), and make deploy-agent use one pool. Read pool size from the environment.
- **Status:** the numbers are verified. The failure is conjecture. Experiment X9.

### F7. Medium: claim renewal competes with all other traffic for a 10-connection pool, can wait 30 s for a connection against a 15 s TTL, and an executor cut off from the database keeps running unfenced
- **Where:**
  - `store/postgres_store/store_impl.rs:43-46`: the claims use `self.pool.clone()`, the task store's pool.
  - `store/pg_retry.rs:76-87`: `pool_options()` sets no `acquire_timeout`, so sqlx's 30 s default applies.
  - `handler/claims/mod.rs:349-378`: the renewal is awaited inline, and on error: `// An outage is not evidence that another replica took over: the fence decides that, write by write.`
- **Failure scenarios:**
  1. Pool starvation. Hundreds of idle subscribe streams cost 2 queries per 250 ms each (F25), plus sends at 8-11 statements each, all on 10 connections. The renewal waits in `acquire` longer than 15 s, the claim expires on the database clock, and a continuation on B takes it over. The healthy owner's executor is then fenced and cancelled, and its work is discarded.
  2. Partition. The owner keeps executing (model calls, tool side effects, outbound A2A delegations) for as long as it cannot reach the database, because only a DB write or a successful renewal stops it. Another replica takes over after 15 s and runs a new turn, so external side effects are duplicated. KNOWN in part: the handoff and book measured "fenced and stopped 2.5 s after its link returned". The duplicated external side effects are not stated.
  3. Runtime starvation. The renewer is a tokio task on the same runtime as executors, so a CPU-bound or blocking executor can starve it.
- **Fix:**
  - Give renewal its own small pool or a reserved connection, and set `acquire_timeout` below TTL/3.
  - Self-fence locally: if no successful renewal within TTL − margin (measured with a local monotonic clock from when the renewal was sent), cancel the executor and stop pushes.
  - Expose `renewal_age` as a metric.
- **Status:** verified by reading; the effect is conjecture. Experiment X10.

### F8. Medium (conjecture): after a takeover, an event position is reused, so subscribers that saw the old turn's live events silently miss the new turn's events
- **Where:**
  - `streaming/event_queue/in_memory.rs:694-744`: the seq is assigned, handed off, and broadcast to live readers before the processor logs it.
  - `processor.rs:156-158`: the log append comes later and is fenced.
  - `messaging/admission.rs:253-276`: the new turn numbers from `max(log end, stored position)`.
  - `subscribe.rs:141-147`: reattach only replays when `through > after`.
- **Failure scenario:**
  - A stalls past TTL. Its executor writes events P+1..P+k, which A's live SSE subscribers receive with `id: P+1..P+k`. The appends are refused (ClaimLost).
  - B takes over and numbers from P+1, which is a different event under the same id.
  - A's subscriber, now reattaching through the store with `after = P+k`, skips B's P+1..P+k.
  - A client resuming with `Last-Event-ID: P+k` also skips them. This violates §3.5.2 and the book's "no position names two events".
- **Fix:** on takeover, start numbering after `max(log end, stored position) + gap`, with the gap carried in the claim (for example, the owner stores its broadcast high-water mark on renewal), or skip a large epoch. Alternatively, end A's live streams with `streamLagged` on ClaimLost.
- **Status:** conjecture with a strong basis in the code. Experiment X11.

### F9. Medium (conjecture): a cross-replica cancel of an executor that exits quietly on its token never pushes CANCELED
- **Where:**
  - `handler/lifecycle/cancel_task.rs:125-129,161-170`: on B, `writer_for_cancel` returns a throwaway writer because B has no queue (`streaming/event_queue/manager.rs:372-385`), then B writes `save_status_delta`.
  - `claims/mod.rs:396-404`: A only fires the token.
  - `halt/sync.rs:194-205`: "The token only".
  - `terminal.rs:141,176-191`: an executor that returns `Ok` after its token fires takes `repark_if_silent`, which returns early because the token is cancelled.
  - `processor.rs:236-246`: CANCELED is pushed only from `supersede`, which needs another write.
- **Book claim this contradicts:** turn-claims.md: "A delivers `Canceled` to the task's webhooks, which nothing else would."
- **Failure scenario:** the common executor pattern `if ctx.cancellation_token.is_cancelled() { return Ok(()) }` leaves the webhook without a terminal event. `tests/cross_replica_cancel/` does not assert a push.
- **Fix:** at the end of the processor, if the stored state is terminal and `!pushed_terminal`, push the stored terminal status. Or have the cancelling replica push it.
- **Status:** conjecture. Experiment X12.

### F10. Medium: every replica re-runs CREATE OR REPLACE FUNCTION on every start, so the last replica to start sets the function bodies for the whole fleet (version skew)
- **Where:** `store/claim/upgrade.rs:99-101` ("Replaced on every start, as are the other functions, so a fix here reaches a database without a migration"); `store/postgres_store/mod.rs:205-210`; `store/claim/postgres.rs:72-85`.
- **Failure scenario:** v0.16.1 fixes `a2a_claim_turn`. During the rollout, or a rollback, or when a v0.16.0 pod is rescheduled or crash-loops, that old pod re-installs the 0.16.0 body. The fix silently disappears for every replica, including 0.16.1 pods, until one of them restarts. `PgMigrationRunner::with_migrations` (`postgres_store/mod.rs:112-123`) runs migration 9 once and never re-applies, so the two constructors also diverge.
- **Fix:** version the function name (`a2a_claim_turn_v2`), or store a body version in a table and only replace when it is higher, under the schema lock.
- **Status:** verified by reading. Experiment X13.

### F11. Medium (conjecture, verify with pg_locks): replica startup DDL takes SHARE locks on hot tables with `statement_timeout=0`, and can stall all writers fleet-wide
- **Where:** `store/pg_schema.rs:51-59,86-93` (`SET LOCAL statement_timeout = 0`, one transaction); `postgres_store/mod.rs:186-200` (four `CREATE INDEX IF NOT EXISTS ... ON tasks`, the event log DDL, and `CREATE INDEX IF NOT EXISTS idx_a2a_task_claims_owner`).
- **Evidence:** PostgreSQL's `CREATE INDEX` resolves and locks the relation (ShareLock) before the IF-NOT-EXISTS check in `index_create`. The authors measured that the `ADD COLUMN IF NOT EXISTS` lock problem exists (`pg_schema.rs:62-66`) and replaced it, but did not do the same for `CREATE INDEX`.
- **Failure scenario:**
  - A pod starts while any transaction holds RowExclusive on `tasks` for a while: a large retention DELETE batch, an analyst's open transaction, or a long `store_write_retry` on a lock.
  - The starting pod's ShareLock request queues behind it, and because `lock_timeout` is unset it waits forever.
  - Every other replica's INSERT/UPDATE on `tasks` (and claim INSERTs/renewals on `a2a_task_claims`) then queues behind the ShareLock request. Renewals stall, which leads to takeovers (F7).
- **Fix:** check `pg_indexes` or `to_regclass` in a `DO` block before each `CREATE INDEX`, as already done for `applied_seq`. Set `SET LOCAL lock_timeout` for the schema transaction and retry.
- **Status:** conjecture. Experiment X14.

### F12. Medium: tenant `max_concurrent_tasks` is per process, so the fleet admits N× the limit
- **Where:** `handler/concurrency.rs:6-9,51-68` ("One `Semaphore` per tenant ... `try_acquire_owned`"); `tenant_config/mod.rs:116-125` ("Enforced by a per-tenant semaphore"); `book/src/deployment/multi-tenancy.md:80` does not mention replicas.
- **Failure scenario:** a tenant with a limit of 10 runs 10×N concurrent turns. A send waiting up to 5 s on a remote claim also holds a slot.
- **Fix:** document it. Optionally count live claims per tenant in the database (`SELECT count(*) FROM a2a_task_claims WHERE scope LIKE tenant%`) under an advisory lock.
- **Status:** verified by reading. Experiment X15.

### F13. Medium: `Profile::Production` accepts in-memory halt and lease stores, so the kill switch is per-replica in a "production" fleet
- **Where:** `src/profile.rs:47-62`, which only requires that the store "is set". The profile's own test `a_complete_production_configuration_builds` (`profile.rs:111-137`) builds with `InMemoryHaltStore::new()` and `InMemoryLeaseStore::new()`.
- **Failure scenario:** the operator halts via replica A. Replicas B..N keep admitting and running work, and a restart forgets the halt. The profile also does not require a shared push-config store, rate-limit counter, or audit store even when the task store is shared.
- **Fix:** when the task store reports shared claims, have Production refuse in-memory halt, lease, push-config and audit stores.
- **Status:** verified by reading. Experiment X16.

### F14. Low: the per-task push-config cap uses a per-process lock, so it can be exceeded permanently across replicas
- **Where:** `handler/push_config.rs:110-135`. The code's own comment says: "Unlike the task store's transient overshoot this one is permanent: nothing re-checks or evicts a config once it is stored." The lock is `self.keyed_lock(...)`, which is per process.
- **Fix:** enforce the cap with an advisory lock or `INSERT ... WHERE (SELECT count) < cap` in the SQL store.
- **Status:** verified by reading.

### F15. Low-Medium: `AuthFailureThrottle` is per process with no shared option
- **Where:** `src/auth/throttle.rs:1-50`. There is no `with_shared_counter` for it.
- **Failure scenario:** with N replicas, an attacker gets N× `max_failures` per window. Round-robin also spreads failures, so each replica sees 1/N of them.
- **Fix:** accept a `RateLimitCounter`.
- **Status:** verified by reading.

### F16. Low: rate-limit windows and sweeps use each replica's wall clock
- **Where:** `rate_limit/window.rs:37-38,63` (`SystemTime::now()`); `rate_limit/shared.rs:139,245-256,273-277` (`DELETE FROM a2a_rate_limit WHERE window_no < $1`, with the replica's own current window).
- **Failure scenario:** a replica whose clock is ahead rolls into window W+1 first and deletes window W's rows while the other replicas are still counting in W. Their counts restart at 0, so the limit can be exceeded about 2× around boundaries.
- **Fix:** compute the window with `extract(epoch from now())` in SQL, and sweep `window_no < current - 1`.
- **Status:** verified by reading.

### F17. Low-Medium: lease deadlines use the replica's wall clock
- **Where:** `store/lease/postgres.rs:79-99` (`SET deadline_ms = $3 + ttl_ms`, where `now_ms` comes from the app); `store/lease/mod.rs:32`; `handler/lease.rs:140-144`.
- **Failure scenario:** a renewal on a replica whose clock is behind, checked by the owner whose clock is ahead, shortens the lease. Skew larger than the TTL kills delegated tasks.
- **Fix:** use `now()` in SQL for both grant/renew and lapse checks.
- **Status:** verified by reading.

### F18. Medium: the audit chain is one optimistic append point for the whole fleet
- **Where:** `audit/log.rs:19-22` (`MAX_APPEND_ATTEMPTS: usize = 8`) and `audit/log.rs:41-46,174-201`.
- **Evidence:** each attempt makes 2+ round trips. Replicas serialize locally, so up to N writers race on each `(chain, seq)`. With the default chain `""` (no tenant), every request in the fleet appends to one chain.
- **Failure scenario:**
  - Audited throughput is capped at about 1/(2·RTT) appends/s fleet-wide, whatever N is. Adding replicas raises contention, not throughput.
  - At N ≥ 9 under load, some appends lose 8 times in a row: "audit append lost the race". With `require_record(true)` (which the Auditable profile requires), the request is refused.
  - `time_ms` uses the local clock (`utc_now_iso8601()`), so timestamps are not monotonic in seq order, and `purge` (`audit/retention.rs:244-279`, `last_before(cutoff)` then `delete_through(seq)`) can delete records younger than the floor by up to the skew.
- **Fix:** append with `INSERT ... SELECT max(seq)+1 ... FOR UPDATE` under `pg_advisory_xact_lock(chain)` in one statement, or use a sequence per chain and backoff with jitter. Take the time from the database.
- **Status:** the mechanism is verified; the thresholds are conjecture. Experiment X17.

### F19. Low: the audit run registry is per process
- **Where:** `audit/log.rs:300-336`; `audit/mod.rs:111-112`; `audit/task_store.rs:53,95`.
- **Failure scenario:** turn 1 runs on A and turn 2 on B. A never sees the terminal state, so its `begin_run` entry lingers (bounded at 65,536) and is reused for anything A later writes for that task. A CancelTask handled by B for a turn on A records no `runSeq`.
- **Status:** verified by reading.

### F20. Low-Medium: a non-owner can synthesize the terminal frame without the real event's metadata
- **Where:**
  - `processor.rs:153-158`: "A terminal event is recorded *after* the store rules on it".
  - `subscribe.rs:110-112`: `Ok(Some(t)) if t.status.state.is_terminal() => return finished(...)`
  - `subscribe.rs:631-649`: `metadata: None`
- **Failure scenario:** the non-owner's poll lands between the owner's terminal `save_status_delta` and its terminal log append (one round trip). The subscriber gets a Failed or Completed frame without `metadata.error`, the failure class, or the budget metadata. `execute.rs` says `metadata.error` "is where streaming callers have always read it". Owner-attached subscribers get the real frame, so the two kinds of stream differ (§3.5.2).
- **Fix:** in `finished`, if `last_event_seq` is below the stored position, wait (bounded) for the log to catch up.
- **Status:** the race is verified by reading; the frequency is unknown. Experiment X18.

### F21. Low: concurrent retention sweeps are not "disjoint", and report `complete` early
- **Where:** `store/retention/postgres.rs:51-77`; `postgres_store/mod.rs:229-231` and sustained-load.md ("two sweeps racing delete disjoint sets").
- **Evidence:** without `FOR UPDATE SKIP LOCKED`, both subqueries pick the same ctids. The second waits, its rows are then gone, `rows_affected == 0`, and it reports `complete = true` while rows remain. No data harm.
- **Fix:** `SELECT ctid ... FOR UPDATE SKIP LOCKED`.
- **Status:** verified by reading.

### F22. Low: agent-card caching differs per replica
- **Where:** `agent_card/static_handler.rs:52` (`last_modified: format_http_date(SystemTime::now())`); `a2a-protocol-types/src/signing.rs:353` (ES256 `key_pair.sign(&rng, ...)`, randomized).
- **Failure scenario:**
  - `If-Modified-Since` misses on every other replica.
  - A card signed with `sign_card_es256` at each replica's boot has a different signature, and so a different ETag, per replica. Round-robin then makes `If-None-Match` miss about (N-1)/N of the time, and clients that pin signatures see them flip.
  - SIGHUP or ConfigMap hot reload reaches pods at different times.
- **Fix:** derive Last-Modified from card content or the build time. Document signing the card once, offline, or with Ed25519 (deterministic).
- **Status:** the ES256 randomness is verified; per-boot signing depends on the user's code (conjecture).

### F23. Low: a send waiting for a remote claim holds the per-context lock for up to 5 s
- **Where:** `messaging/commit.rs:90-93` (context lock taken) and then `:139` (`claimed_view` leads to `claim_turn`, a loop of up to 5 s).
- **Failure scenario:** all sends on this replica in the same context (including forks to new tasks) queue behind the wait. This is head-of-line blocking for swarm or multi-task contexts.
- **Status:** verified by reading.

### F24. Low (conjecture): a small idempotency replay window remains
- **Where:** `messaging/commit.rs:127-131` (key claimed) and `messaging/claimed.rs:110-115,141-145` (released only after `try_claim_turn` returns Busy).
- **Failure scenario:** a duplicate that lands on C between B's key claim and B's release gets `Replay` with the task's previous-turn state. B's send is then refused after its 5 s wait, so the client was told "replayed" for a message that never ran. This is a narrower form of the bug fixed in `fa2785eb`.
- **Fix:** try the turn claim before the key claim, or mark the key "pending" so a replayer waits.

### F25. Low-Medium (partly KNOWN): reattach polling is unbounded and shares the pool
- **Where:** `limits/mod.rs:300-301` (`subscribe_reattach_interval: 250ms`, `subscribe_max_idle: None`); `subscribe.rs:107-171`.
- **Evidence:** the 250 ms poll is KNOWN. Not stated: each idle stream costs `get` + `last_event_seq`, about 8 queries/s, forever (including on F1 orphans), on the 10-connection pool that also renews claims (F7).
- **Fix:** LISTEN/NOTIFY (KNOWN as unbuilt), a default idle bound, or a separate read pool.

### F26. Low: SQLite claims use the wall clock, and nothing guards against network filesystems
- **Where:** `store/claim/sqlite.rs:6-10,93-97` (`SystemTime::now()`).
- **Failure scenario:** an NTP step or a VM resume larger than the TTL makes live claims look expired. A ReadWriteMany volume shared by pods is warned against in the docs, but nothing detects it.
- **Status:** verified by reading.

### KNOWN (not counted above)
- Streaming on a non-owner is up to 250 ms late.
- A cancel reaches the owner at its next renewal (1 s).
- An outage longer than `store_write_retry` loses events and can leave the task unfinished.
- The AES-GCM nonce budget is per process.
- The rate limit is N× without a shared counter, and falls back to local counting when the counter is down.
- Halts and leases are per process without their stores.
- A pooler disables the halt store's LISTEN, which falls back to polling.
- Erasure is not serialized across replicas.
- `ALTER DATABASE ... SET` does not reach pooled backends that are already open.

### deploy-agent wiring (the question asked)
`examples/deploy-agent/src/main.rs:173-182` wires `PostgresTaskStore::new` and `PostgresPushConfigStore::new`. Turn claims come from the store by default (`builder.rs:270`, `TurnClaimsChoice::FromStore`), so claims **are** on. Everything else a production user would enable is absent:
- push sender
- halt store
- lease store
- rate limiter
- audit
- auth
- tenant configuration

So the live "6 of 6 GetTask" check exercised only the task store and claims. Its shutdown path (F2), readiness (F5) and pool count (F6) are deploy-agent defects in their own right.

## 3. Live experiments (2-3 replicas behind HAProxy `balance roundrobin`, PostgreSQL 17)

Common setup:
- Use `scripts/live/replica`, which has the `hold`/`park`/`finish` commands of `tests/replica_processes.rs`, or deploy-agent with a slow executor.
- HAProxy: `option httpchk GET /readyz`, `default-server inter 2s fall 2 rise 2`, keep-alive on.
- Log `pg_stat_activity` and `pg_locks` once a second.

- **X1 (F1, orphan).**
  1. Send `hold` through the LB with `return_immediately`, and note which replica owns it from `a2a_task_claims.owner`.
  2. `kill -9` the owner.
  3. For 5 minutes, poll GetTask through the LB, keep a SubscribeToTask open through the other replica, and replay the same send with the same `a2a-idempotency` key.
  - **Expect:** WORKING forever, the stream never ends, and the replay returns WORKING.
  - **Also:** check that `a2a_task_claims` is empty after 15 s while the task is still non-terminal.
- **X2 (F1, failover or claim loss).**
  1. During a `hold` turn, run `DELETE FROM a2a_task_claims WHERE task_id=...` (this simulates a claim lost to async-replica failover).
  - **Expect:** a ClaimLost log line on the owner, the executor cancelled, and the task WORKING forever.
- **X3 (F2, rolling restart).**
  1. Start a 30 s `hold` on A.
  2. `kill -TERM` A.
  - **Expect:** the task becomes CANCELED after about 5 s, not continued on B.
- **X4 (F2, sends during the drain).**
  1. Run `oha -c 4 -z 30s` sends (each about 6 s long) through HAProxy.
  2. SIGTERM A.
  - **Measure:** how many sends admitted by A after the SIGTERM end CANCELED, and that `/readyz` on A stays 200 for about 15 s. Repeat with the SDK's `serve_with_shutdown` replica.
- **X5 (F3).**
  1. Point a push config at a webhook that sleeps 10 s (`python3 -m http.server`-style handler with `time.sleep(10)`).
  2. Park a task on A.
  3. Immediately send the continuation directly to B, then repeat directly to A.
  - **Expect:** B refuses "already being processed" after 5 s, and A admits it.
- **X6 (F4).** Build a replica binary with `PostgresTaskStore` and `HttpPushSender` but no push-config store. Create the config via B, run the task via A, and check that the webhook receives nothing.
- **X7 (F5, slow count).**
  1. `INSERT INTO tasks SELECT ... FROM generate_series(1, 2e7)`.
  2. Time `curl -m1 /readyz` on every replica and `EXPLAIN ANALYZE SELECT COUNT(*) FROM tasks`.
  - **Expect:** all replicas fail together once the count takes more than 1 s.
- **X8 (F5, read-only).** Point one replica at a streaming standby or `default_transaction_read_only=on`. Expect `/readyz` 200 and sends to fail with 25006.
- **X9 (F6).**
  1. Set `max_connections=50` (to scale down the test).
  2. Start 3 deploy-agent replicas plus a load generator at about 200 concurrent sends.
  - **Watch for:** `53300` in the logs and `numbackends` in `pg_stat_database`.
- **X10 (F7).**
  1. On A, open 2,000 SubscribeToTask streams on parked tasks.
  2. Start a 60 s `hold` turn on A.
  3. Every 2 s, try a continuation via B.
  - **Watch for:** "turn claims not renewed" warnings, `pg_stat_statements` on `RENEW_SQL`, and a ClaimLost or takeover on A.
  - **Variant:** use toxiproxy to cut A's database link for 20 s while A's executor does an observable external side effect (a counter webhook). Count the side effects: expect duplicates.
- **X11 (F8).**
  1. Attach an SSE subscriber directly to A, logging `id:`.
  2. Have A stream artifacts.
  3. Cut A's database link for 20 s (toxiproxy) and send a continuation via B after 16 s.
  4. Compare the ids A's subscriber saw against the `task_events.seq` rows.
  5. Resubscribe with `Last-Event-ID` at A's highest id and check that B's events with lower ids are missing.
- **X12 (F9).** Use an executor that loops `sleep(100ms)` until the token is cancelled and then returns `Ok(())`. Register a webhook, start the task on A, and cancel via B. Expect no CANCELED POST.
- **X13 (F10).**
  1. `CREATE OR REPLACE FUNCTION a2a_claim_turn(...)` with a marker comment (this simulates a newer release).
  2. Restart one replica.
  3. Check `pg_get_functiondef('a2a_claim_turn'::regproc)`. Expect the marker to be gone.
- **X14 (F11).**
  1. In psql: `BEGIN; UPDATE tasks SET state=state WHERE id='<some id>';`, and leave it open.
  2. Start a third replica.
  3. Check `SELECT locktype, relation::regclass, mode, granted FROM pg_locks WHERE NOT granted`. Expect a ShareLock waiting on `tasks`.
  4. Send through the other replicas. Expect their writes to block (`wait_event_type = Lock`) and renewals to stall.
- **X15 (F12).** Set `PerTenantConfig` to `max_concurrent_tasks(2)` on 2 replicas, then start 4 `hold` turns through the LB. Expect all 4 admitted.
- **X16 (F13).** Use Production profile with `InMemoryHaltStore` on 2 replicas. Halt via A and send via B. Expect it admitted.
- **X17 (F18).** Use 3+ replicas (or 10 handlers in one process on separate pools) with `AuditLog` on `PostgresAuditStore` and `require_record(true)`. Load at 500 rps and count "lost the race" errors and audited rps as N goes 1, 2, 3.
- **X18 (F20).** 500 runs of an executor that fails, with a subscriber on the non-owner. Count terminal frames lacking `metadata.error`.
- **X19 (F16).** `faketime '+30s'` on one replica with `window_secs=60` and a shared counter. Send limit+X requests across the boundary and count admissions.
- **X20 (F21).** Run `purge_expired` from 3 replicas at once over 50k terminal rows with `batch=1000`. Expect some reports with `complete=true` and `tasks_deleted` well below 50k while rows remain.
