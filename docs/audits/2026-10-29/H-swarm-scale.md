# H — Swarm-scale audit: what blocks a frontier-lab operator running millions of agents

Audited at `8dc4331` (2026-10-29), from source only. Nothing was built or run.
Scope: `crates/a2a-protocol-{types,client,server,sdk}`, `bindings/`, `examples/swarm`,
`docs/swarm-*.md`, ADRs 0019–0027.

**The scope boundary to keep in mind.** `docs/handoff-history.md:3473` ("What not to chase",
amended 2026-10-02) rules out registries, schedulers, orchestration DSLs and a mesh in the
protocol crates. This audit respects that line. Where a gap falls outside it (discovery,
scheduling), the proposal is a **companion crate** or an **extension in the existing style**
(lease, budget, idempotency), not a change to the core crates.

---

## 1. What exists (verified in code)

| Area | What is there | Where |
|---|---|---|
| Delegation handle (G1-A) | `Delegation::{start,start_leased,start_budgeted}`: sends as a stream, learns the child's id, and cancels the child on parent cancel, on drop, or on stream loss. Adds an idempotency key automatically when the peer honours keys | `crates/a2a-protocol-client/src/delegation/mod.rs` |
| Lease (G1-B, ADR 0020) | Grant in `Message.metadata`. Renewal is a `GetTask` carrying the `A2A-Extensions` header. The server keeps a `LeaseStore` (in-memory or Postgres) and runs a per-task watcher that stops the task once the lease lapses | `types/src/lease.rs`, `server/src/handler/lease.rs`, `server/src/store/lease/{mod,postgres}.rs`, `client/src/delegation/renewal.rs` |
| Budget (G5, ADR 0023) | Relative deadline plus up to 16 named u64 meters. The server caps the executor timeout at `min(own, remaining)`. `BudgetHandle::grant` reserves part of the parent's budget, and the reservation is refunded from the child's reported spend. The tool loop charges `tokens` | `types/src/budget.rs`, `types/src/budget/handle.rs`, `server/src/handler/budget.rs`, `server/src/tool_loop/executor.rs:42-117` |
| Halts / kill switch (ADR 0019, 0024) | `RequestHandler::halt(HaltScope::{All,Tenant})`. A `HaltStore` (memory, SQLite, PG) is polled every 1 s. On PG, halts and cancels are also pushed with `LISTEN/NOTIFY` | `server/src/handler/halt.rs`, `handler/halt/sync.rs`, `store/halt/*` |
| Cross-replica single writer | Turn claims, fenced writes, one renewer per handler ("one statement per interval"). Pushed cancel via `publish_cancel`. **This closes swarm-scale-findings Finding 8.** The test is now `the_single_writer_refusal_crosses_replicas` | `server/src/handler/claims/mod.rs`, `store/claim/*`, `tests/multi_replica.rs:554` |
| Idempotency | Send keys honoured by the store (SQL included). Tool-call keys are HMAC-scoped (ADR 0026) | `types/src/idempotency.rs`, `server/src/handler/messaging/idempotency.rs`, `store/postgres_store/idempotency.rs`, `server/src/tool_loop/idempotency.rs` |
| Failure taxonomy | `FailureClass` on the failed status. The client's retry classification follows it | `types/src/failure.rs` |
| Admission caps | Global queue cap `DEFAULT_MAX_CONCURRENT_STREAMS = 1024` (one queue per running turn) → `Overloaded`. Optional per-tenant `max_concurrent_tasks` semaphore → `Overloaded`. Both use `try_acquire`: **refuse, never queue** | `server/src/builder.rs:39,1471`, `streaming/event_queue/manager.rs:185-320`, `handler/concurrency.rs`, `handler/messaging/admission.rs:226-237` |
| Rate limiting | Per-caller fixed window. Optional shared counter across replicas | `server/src/rate_limit/{mod,shared,config}.rs` |
| Tenancy | Fail-closed tenant isolation (G3 option 2): a non-isolating store refuses a tenant request. Tenant-aware SQLite and PG stores exist. Per-tenant limits and per-tenant halts | `handler/mod.rs:122,408`, `store/tenant*` |
| Identity across hops | RFC 8693 token exchange on the client. The server keeps the `act` chain (16 hops max) in `CallContext::actors()` and in audit records | ADR 0021, `server/src/auth/actors.rs` |
| Tracing | W3C traceparent is propagated by default (`ClientConfig::propagate_trace`). Delegation renewals and cancels run inside the originating span. `gen_ai.*` agent and tool spans, and tool-loop token counters | `client/src/trace_propagation.rs`, `client/src/delegation/origin.rs`, `server/src/rpc_span/{agent,tool,tool_metrics}.rs` |
| Audit | Hash-chained, signed checkpoints. Records carry `trace_id`, `actor`, `task_id`, `context_id` | `types/src/audit/record.rs`, `server/src/audit/*` |
| Durable stores | SQLite and Postgres task stores, event log with `seq` used as the SSE id, delta writes (`save_status_delta`, `save_artifact_delta`), retention via an explicit `purge_expired` | `store/{sqlite_store,postgres_store,retention}` |
| Stream resumption | `subscribe_to_task_from(id, last_event_id)`. `streamLagged` catch-up from the log | `client/src/methods/tasks.rs:222`, `server/src/handler/lifecycle/subscribe.rs` |
| Card caching | `CachingCardResolver`: RFC 9111 freshness and ETag/Last-Modified revalidation, **one URL per resolver** | `client/src/discovery.rs:339-505`, `discovery/freshness.rs` |
| Transports | JSON-RPC, REST, WebSocket (multiplexed, `DEFAULT_MAX_PENDING_REQUESTS=64`), gRPC (tonic channel), SLIMRPC custom binding (incl. multicast) | `client/src/transport/*`, `bindings/a2a-protocol-slimrpc` |
| Health | `/health` (constant), `/ready` (task store health only). Introspection: `active_queue_count`, `task_count`, `cancellation_token_count` | `dispatch/axum_adapter.rs:665`, `handler/introspection.rs` |
| Production profile | Refuses to build without an authenticating interceptor, a halt store, a lease store and a content signer | `server/src/profile.rs`, ADR 0025 |
| Interop | a2a-go v2.7 client against the Rust server, Python client against the Rust server, ITK, TCK | `itk/interop/*`, `tck/` |

---

## 2. Gaps, ranked by impact at millions-of-agents scale

Severity: **S0** blocks the use outright or causes silent wrong behaviour at scale. **S1** a serious
reliability or cost problem. **S2** an operability or ergonomics gap. Effort: S ≤ 3 days, M ≤ 3 weeks, L > 3 weeks.

### P0 — must fix before a "production-ready for swarms" claim

#### 1. Overload and halt look like permanent failures on JSON-RPC and WebSocket — S0, effort S
**Evidence.**
- `ServerError::Overloaded` → `A2aError::internal(msg)` with no `RetryInfo` (`server/src/error/mod.rs:334`).
- JSON-RPC errors go out as HTTP **200** unless they are auth errors (`dispatch/jsonrpc/response.rs:152-156`).
- The client treats `Protocol(e)` as retryable only when `e.retry_after().is_some()` (`client/src/retry.rs:172,290`).
- So on JSON-RPC, the default binding (and on WebSocket, `dispatch/websocket.rs:1382`), saturation at the 1024-stream cap, a per-tenant concurrency cap, or an idempotency store that is down (`handler/messaging/idempotency.rs:85`) reaches the caller as a **non-retryable `-32603`**.
- `Halted` is indistinguishable from an internal error as well (`error/mod.rs:337`).
- REST and gRPC are correct: 503 / `RESOURCE_EXHAUSTED`. But REST sends **no `Retry-After`** for `Overloaded`, because `retry_after()` is `None` for that variant (`error/mod.rs:225`).

In a swarm, the most common transient condition, a popular worker that is saturated, is reported as final. Orchestrators drop work or escalate falsely.

**Design.**
```rust
// server/src/error/mod.rs
Self::Overloaded(msg) => A2aError::internal(msg.clone())
    .with_retry_after(self.suggested_backoff())          // RetryInfo detail, all bindings
    .with_failure_class(FailureClass::Transient);        // a2a-rust failure extension
Self::Halted(msg) => A2aError::internal(format!("halted: {msg}"))
    .with_failure_class(FailureClass::Halted /* new, non-retryable */);
// REST: emit Retry-After for Overloaded (jittered 1–5 s by default, configurable)
pub struct HandlerLimits { pub overload_retry_after: Duration, … }
```
Add a TCK-style test for each binding: saturate → client sees `is_retryable() == true`.

#### 2. The HTTP client cannot use a private CA or mTLS, and every `A2aClient` owns a separate pool — S0, effort M
**Evidence.**
- `RestTransport::with_all_timeouts` and `JsonRpcTransport::with_all_timeouts` always call `crate::tls::default_tls_config()`, which is webpki roots with `with_no_client_auth()` (`client/src/transport/rest/mod.rs:165-169`, `tls.rs:60-66`).
- `ClientBuilder` has a TLS hook only for gRPC (`with_grpc_tls_config`, `builder/mod.rs:459`) and WebSocket. `tls_config_with_extra_roots` and `build_https_client_with_config` are public but no HTTP transport consumes them; only raw-hyper tests use them (`tests/tls_integration_tests.rs:168`).
- An internal swarm on a private PKI or SPIFFE mTLS, which the server supports for caller identity (ADR 0018), therefore cannot use the default HTTP client without writing a custom `Transport`.

Pooling:
- Every `build()` creates a fresh hyper `Client`, a fresh `RootCertStore` cloned from webpki, and a fresh TLS session cache (`tls.rs:97-101,122-137`). There is no connection or session reuse across peers on the same host and no TLS resumption across clients.
- There is no `pool_max_idle_per_host` and no per-host connection ceiling.
- Plaintext `http://` (typical behind a mesh sidecar) is **HTTP/1.1 only**: no h2c prior-knowledge option. Every open SSE stream (one per delegation) therefore pins one TCP connection. A supervisor with 10k leased children on one worker host holds 10k sockets.
- `swarm-scale-findings` Finding 9 shows a p95 above 1 s for 512 simultaneous card fetches, and attributes it to connection setup.

**Design.**
```rust
// client: one shared, cloneable connection layer for many agents
pub struct HttpPool { /* Arc<hyper Client<HttpsConnector>> */ }
impl HttpPool {
    pub fn builder() -> HttpPoolBuilder;          // tls(ClientConfig), client_cert(chain,key),
}                                                  // extra_roots, h2c_prior_knowledge(bool),
                                                   // max_conns_per_host(usize), idle_per_host,
                                                   // dns_resolver(Arc<dyn Resolve>), http2_keepalive
impl ClientBuilder {
    pub fn with_http_pool(self, pool: HttpPool) -> Self;   // REST + JSON-RPC + card fetch share it
    pub fn with_tls_config(self, cfg: rustls::ClientConfig) -> Self;
}
```
Default stays one pool per client, so nothing changes for existing users. Document "one `HttpPool` per process" for orchestrators. Add `rustls-native-certs` or `SSL_CERT_FILE` support behind a feature.

#### 3. Tasks orphaned by a crashed replica stay `working` forever, and their leases keep being renewed — S0, effort M
**Evidence.** ROADMAP.md:380: "No executor resumes after a restart … stays `Working` … forever … recorded, not scheduled."

- Turn claims are taken over only when a *new send* reaches the task (`store/claim/postgres.rs:46`). Nothing scans for expired claims on non-terminal tasks.
- The lease watcher is a `tokio::spawn` **in the process that ran the executor** (`handler/lease.rs:148-185`). When that process dies, no one watches the deadline.
- A parent's renewals (`GetTask` with the extension) still succeed and push the deadline forward, because renewal checks only that the read succeeded (ADR 0020). So the parent sees a live child that will never finish.
- A parent that follows by polling (`follow_task`) waits until its own timeout.

At scale, with hourly node churn, this produces a steady accumulation of zombie subtrees.

**Design.** Add a reconciler owned by the handler, using the claim table that already exists:
```rust
RequestHandlerBuilder::with_orphan_reconciler(ReconcilerConfig {
    scan_every: Duration::from_secs(10),
    grace: claim_ttl * 2,
    action: OrphanAction::Fail(FailureClass::Transient) /* or Requeue(Arc<dyn Resumer>) */,
})
// one statement per scan per replica:
// SELECT t.id FROM tasks t LEFT JOIN turn_claims c USING(tenant,id)
//  WHERE t.state IN ('submitted','working') AND (c.expires_at IS NULL OR c.expires_at < now()-grace)
//  FOR UPDATE SKIP LOCKED LIMIT 500
```
The reconciler writes `failed` with `FailureClass::Transient` and the message "executor lost". Parents then retry with the same idempotency key. Renewal must also **refuse** (`TaskNotRunning`) a lease on a task that has no live claim, so the parent learns this on its next renewal.

#### 4. Delegation cancels its child on any stream blip, and has no durable journal or reattach (G2 still open) — S0, effort M
**Evidence.**
- `Delegation::wait` → `next_event` → `Err` or `None` → `lost()` → `cancel_task_in` (`client/src/delegation/mod.rs:328-345, 390-396`).
- The client never resubscribes automatically, even though `EventStream::last_event_id()` exists (`streaming/event_stream.rs:447`) and the server supports `subscribe_to_task_from`.
- An L4 load-balancer idle reset, a proxy deploy, or `stream_idle_timeout` (5 min default) therefore **kills healthy long-running children**, and the kill cascades down the tree.
- `swarm-orchestration.md` G2 (record child before await, reattach on restart) is not implemented. `detach()` returns an id but there is no `Delegation::reattach`.

**Design.**
```rust
pub trait DelegationJournal: Send + Sync {            // in a2a-protocol-client
    fn record(&self, e: &ChildRecord) -> BoxFuture<'_, Result<()>>;   // before stream_message
    fn settle(&self, id: &ChildKey, o: &Outcome) -> BoxFuture<'_, Result<()>>;
    fn open(&self, parent: &str) -> BoxFuture<'_, Result<Vec<ChildRecord>>>;
}
pub struct ChildRecord { parent: String, agent_url: String, tenant: Option<String>,
                         idempotency_key: String, task_id: Option<String>,
                         last_event_id: Option<String>, lease_ttl: Option<Duration>, grant: Option<…> }
impl Delegation {
    pub fn with_resume(self, ResumePolicy { attempts: 5, backoff: …, within: Duration }) -> Self;
    pub async fn reattach(client: Arc<A2aClient>, rec: ChildRecord) -> ClientResult<Self>;
}
```
`lost()` resubscribes from `last_event_id` within the policy before it cancels. Ship `SqliteJournal` and `PostgresJournal` (or a `TaskStore`-backed one). Validate with the swarm run that kills a supervisor, which the swarm document lists as not done.

#### 5. Control-plane cost is O(children) round trips and store operations per interval — S1 (S0 at 10^6), effort M
**Evidence.**
- *Client renewal.* One spawned task per child, one `GetTask` every ttl/3. The `TaskQueryParams` has no `history_length`, so every renewal returns **the full task** (`delegation/renewal.rs:27-45`, `methods/tasks.rs:45`).
- *Server watcher.* One spawned task per leased turn, one `store.deadline()` read every `min(ttl/4, 1s)` (`handler/lease.rs:136-185`).
- *Idle subscribers.* Every `SubscribeToTask` on a task without a live queue polls the store every **250 ms** with no time bound (`subscribe_max_idle: None`) and no count bound. There is no per-task or global subscriber cap (`handler/limits/mod.rs:172-200,300`).
- With 1M leased children at a 30 s TTL, that is about 100k renew RPCs/s and about 250k–1M lease reads/s, before any real work. 10k parked `input-required` tasks with one subscriber each produce 40k store reads/s.

**Design.**
- **Lease groups.** The grant carries `{"ttlMs":…, "group":"<parent-task-or-uuid>"}`. A new extension call (`POST /extensions/lease/v1:renew` with `{"groups":[…]}` on REST; on JSON-RPC `a2a-rust.com/lease.renew`) renews every child of a group in one store statement. One renewer per `A2aClient` and peer, not one per child.
- **Server sweeper.** Replace the per-task watchers with one per-replica sweeper: `SELECT … WHERE deadline < now AND replica = me` every second, then fire tokens.
- Renewal sets `history_length = 0` until groups land. That change is S and ships immediately.
- **Subscribers.** `HandlerLimits::{max_subscribers_per_task, max_subscribers}`. Exceeding either returns `Overloaded` with Retry-After. Parked-task reattach becomes an event (a PG `LISTEN` on the task channel already used by ADR 0024) with exponential backoff instead of a fixed 250 ms poll.

#### 6. No retry budget, circuit breaker or client-side concurrency limit; retries amplify overload — S1, effort M
**Evidence.**
- `RetryPolicy` has `max_retries` and backoff only (`client/src/retry.rs:93-112`). No `breaker`, `hedge` or `retry budget` appears anywhere in the client.
- HTTP transports have no in-flight cap; only WebSocket does (`websocket.rs:157-168`).
- Retries do not respect an inherited budget deadline. The doc tells callers to wrap the call in `tokio::time::timeout` themselves (`retry.rs:60-81`).
- At the defaults, 1000 supervisors hitting one saturated worker produce up to 4× the load (initial attempt plus 3 retries) on the worker that is already refusing.

**Design.**
```rust
RetryPolicy::default()
    .with_budget(RetryBudget { ratio: 0.1, min_per_sec: 10 })   // token bucket shared per HttpPool host
    .with_deadline_from_budget(true);                            // stop when BudgetHandle::remaining_time() < next backoff
ClientBuilder::with_concurrency_limit(ConcurrencyLimit::per_host(256))   // returns TooManyPendingRequests (already retryable)
ClientBuilder::with_circuit_breaker(Breaker { failure_ratio: 0.5, window: 10s, open_for: 5s, half_open_probes: 3 })
```
The breaker state lives in the `HttpPool` (gap 2), keyed by authority, so all agents in a process share what they learn about a peer.

#### 7. Readiness ignores saturation and halt; no load signal for balancers — S1, effort S
**Evidence.**
- `/ready` checks only `task_store_health()` (`dispatch/axum_adapter.rs:665-677`). A replica at 1024/1024 queues, or halted, still reports ready, so a K8s Service or LB keeps sending it work that will be refused.
- `ServeConfig::max_connections` defaults to `None` (`serve/graceful/config.rs:139`), and the production profile does not require it.

**Design.**
- `/ready` returns 503 `{"status":"saturated"|"halted"}` above a configurable high-water mark: `HandlerLimits::ready_high_watermark = 0.9`.
- Add a response header `A2A-Load: queues=…;cap=…` (ORCA-style) so client LB (gap 12) can do least-loaded selection.
- The production profile warns when `max_connections` is unset.

#### 8. Card discovery cost at fleet cold start — S1, effort S
**Evidence.**
- `fetch_inner` builds a **new hyper client, TLS config and root store on every card fetch** (`client/src/discovery.rs:563-577`).
- `CachingCardResolver::resolve` has no single-flight: N concurrent callers with a stale entry make N fetches.
- It has no stale-if-error: a failed revalidation returns `Err` even when a usable card is held (`discovery.rs:437-480`).
- One resolver covers one URL; there is no multi-agent cache.

**Design.** `CardCache` (keyed by URL, bounded LRU) on the shared `HttpPool`, with a single-flight per key, `stale_if_error: Duration`, background refresh at 80% of freshness, and a `watch()` that emits card changes. Reuse it in `ClientBuilder::from_url_cached`.

### P1 — roadmap, needed for frontier-lab operation but not a blocker for a "production-ready" claim

#### 9. Admission control has refuse-only semantics: no queue, priority or weighted fairness — S1, effort M–L
**Evidence.**
- Both the global cap and the per-tenant cap use `try_acquire`, an explicit design choice to refuse rather than queue (`handler/concurrency.rs` header). Nothing anywhere carries a priority.
- The global 1024 cap is first-come: one tenant can take all of it unless the per-tenant caps are sized so they sum to no more than the global cap (`tenant_config/mod.rs:87`).
- Combined with gap 1, a burst in one tree becomes failures elsewhere.

**Design.** An optional `AdmissionQueue`, so refuse-by-default is kept:
```rust
RequestHandlerBuilder::with_admission(Admission::queued(QueueConfig {
    max_wait: Duration::from_millis(500),   // never longer than the caller's budget.remaining_time()
    max_queued: 4096,
    classes: PriorityFrom::BudgetMetadata("a2a-rust.com/priority"),  // 0..=3
    fairness: Fairness::WeightedPerTenant(tenant_weights),           // DRR across tenants
}))
```
A queued send whose budget would expire in the queue is refused immediately with `BudgetExhausted`.

#### 10. No task lineage, so there is no tree view and no "halt this job" scope — S1, effort M (extension) / L (upstream)
**Evidence.**
- A grep for `parent_task|lineage|root_task` across the crates finds nothing.
- `HaltScope` is `All | Tenant` (`handler/halt.rs:50-57`).
- A swarm-wide stop for a single job therefore means either halting whole servers or relying on every hop running the Rust lease. Trees can be reconstructed only offline, from `trace_id` in audit records.

**Design.** A lineage extension `a2a-rust.com/lineage/v1`: `{"root":"<job-id>","parent":"<task-id>","depth":n}`. `Delegation` attaches it automatically from the `RequestContext`. The server stores it in indexed columns (`root_id`, `parent_task_id`) and enforces `max_depth`. Add `HaltScope::Root(String)`, propagated through the shared `HaltStore` (one org-wide PG halt table is enough to stop a job across every agent service that shares it). Add `ListTasks` filters `rootId` and `parentTaskId`, which become a tree API for dashboards. Propose it upstream as `parentTaskId` (the swarm document's G1-C).

#### 11. Metrics have no tenant, agent or job dimension and no control-plane instruments — S1, effort S–M
**Evidence.** The `Metrics` trait hooks are `on_request/response/error/latency/queue_depth/pool/persistence/store_retry/push/rpc_call/executor_run` (`server/src/metrics/mod.rs:54-158`), none taking a tenant. `a2a.tenant` appears only on spans (`rpc_span/a2a.rs:77`). There is no instrument for lease lapses, halts applied, budget-exhausted endings, overload refusals by cause, claim takeovers, or spend by meter. Tool-loop token counters carry the model but not tenant, principal or root.

**Design.** Add `on_refusal(cause, tenant)`, `on_lease(event)`, `on_halt(scope)`, `on_budget_settle(tenant, meter, granted, spent)` and `on_claim_takeover()`. Add an opt-in bounded-cardinality tenant attribute (top-N plus `other`). Emit `a2a.server.budget.spent{meter,tenant}` so cost accounting comes from the budget the SDK already enforces.

#### 12. No client load balancing across replicas of a peer, no outlier ejection, no hedging — S2, effort M
**Evidence.** One `A2aClient` targets one URL (`builder/mod.rs:132`). The gRPC transport uses one `Channel` with no `balance_list`. DNS resolution goes through the default `HttpConnector` (getaddrinfo, no TTL-aware caching).

**Design.** `EndpointSet` (static list, DNS SRV/A with TTL, or a discovery source from gap 15), with P2C least-loaded selection using `A2A-Load` (gap 7) and outlier ejection fed by the breaker. Hedging only for idempotent reads (`GetTask`, `ListTasks`, card) after p95. **Sends that carry an idempotency key are the one safe place to hedge a write**, and should stay opt-in.

#### 13. Memory and payload scaling — S1, effort M
**Evidence.**
- Event queues are bounded by **count** (256), not bytes, with a 16 MiB max event (`streaming/event_queue/mod.rs:79-82`). History is capped at 1024 messages / 16 MiB per task (`messaging/decisions.rs:25,34`). There is no process-wide byte budget, so worst-case memory is roughly caps multiplied together, with nothing to stop it.
- Artifacts and file `raw` parts are stored base64 inside the task JSON/JSONB row (`store/postgres_store/mod.rs:33`). There is no blob offload or URI rewriting.
- SQL stores still lack a `save_appending_history` override, so a send costs O(history) twice (Finding 7 still holds: overrides exist only in-memory, encrypted, traced and audit).
- The SQL event log is trimmed only when a terminal task is purged, so a long-lived "channel" task (the coordination shape from swarm-scale-findings) grows without bound.

**Design.**
- `HandlerLimits::max_buffered_bytes` (process-wide semaphore over in-flight event bytes; `Overloaded` when exceeded).
- A `BlobStore` trait (`put(bytes) -> Url`, presigned GET) with `ArtifactOffload { threshold: 256 KiB }`, which rewrites `raw` parts to `url` parts before persistence and broadcast.
- PG/SQLite `save_appending_history` that appends a row instead of rewriting the whole document.
- `RetentionPolicy::max_events_per_live_task` with compaction to a snapshot event.

#### 14. Push delivery is not durable — S2, effort M
**Evidence.** `HttpPushSender` delivers in the background processor within `push_delivery_budget`. There is no outbox or dead-letter queue (no grep hits in `server/src/push`), so a crash loses notifications that were in flight. Fleets that use push to avoid polling silently miss terminal events.

**Design.** A `PushOutbox` table (task, config, seq, attempts, next_at) written in the same transaction as the status delta, drained by a per-replica worker using `SKIP LOCKED`. Delivery becomes at-least-once with `seq` for receiver deduplication (the webhook receiver already exists in `client/src/webhook`).

#### 15. Discovery: no registry, catalog, mDNS/DNS-SD or capability matching — S2 for the core crates, S1 for the operator; effort M (companion crate)
**Evidence.** A grep for `mdns|dns-sd|registry|catalog` in the crates finds nothing relevant. Out of scope by decision (`docs/handoff-history.md:3473`). Skills have tags (`AgentSkill::with_tags`) but there is no matcher.

**Design.** Keep it out of the protocol crates. Add a companion crate `a2a-protocol-discovery`:
```rust
pub trait AgentDirectory { fn lookup(&self, q: &CapabilityQuery) -> BoxStream<'_, AgentRef>;
                           fn watch(&self) -> BoxStream<'_, DirectoryEvent>; }
// impls: StaticDirectory, DnsSrvDirectory, MdnsDirectory (_a2a._tcp, TXT: card path, bindings),
//        HttpCatalogDirectory (GET /agents?skill=…&tag=…), and CardCache-backed health
pub struct CapabilityQuery { skills: Vec<String>, tags: Vec<String>, input_modes, output_modes,
                             required_extensions: Vec<String> /* lease, budget… */ }
```
Peer health comes from the breaker (gap 6) and `/ready` (gap 7).

#### 16. The rate limiter fails closed above 10k distinct callers by default — S2, effort S
**Evidence.** `DEFAULT_MAX_BUCKETS = 10_000`. With the map full and no stale buckets, new callers are rejected (`rate_limit/config.rs:45-55`). The default is 100 requests/min per caller. A shared service with more than 10k distinct agent identities in one window refuses new agents.

**Design.** Document this in the production chapter. Make the limit scale with memory (around 64 B per bucket, so 1M buckets is about 64 MiB). Prefer a sketch-based limiter (count-min or sliding-log approximation) with bounded memory and no fail-closed cliff.

#### 17. The tool loop has no shared inference concurrency limit or 429 backoff — S2, effort S–M
**Evidence.** HTTP 429 from the model is turned into an error (`tool_loop/model.rs:341-346`). There is no semaphore or retry in `openai.rs`. In the measured live run the model was the bottleneck by three orders of magnitude (`swarm-orchestration.md` G4).

**Design.** `OpenAiChatModel::with_concurrency(n)`, `with_retry(RetryPolicy)` honouring `Retry-After`, and charging the budget for the time spent waiting.

#### 18. Ergonomics: wiring a child into the parent's context is manual — S2, effort S
**Evidence.** `examples/swarm/src/supervisor.rs:64-117` threads cancel tokens by hand. The budget carve, the lease and RFC 8693 token exchange are each separate calls.

**Design.** In `a2a-protocol-sdk`, which depends on both crates:
```rust
ctx.delegate(&client, params)
   .share(0.25)              // budget fraction via BudgetHandle::grant
   .lease(Duration::from_secs(30))
   .on_behalf_of_caller()    // RFC 8693 exchange with ctx's token
   .start().await?           // wait() bound to ctx.cancellation_token automatically
```

### P2 — interop and bindings

#### 19. Swarm controls are Rust-only, so polyglot trees lose control at the first non-Rust hop — S1 for real fleets, effort L
**Evidence.** Lease, budget, idempotency, failure class, provenance and approval are all `https://a2a-rust.com/extensions/*` URIs (`types/src/lease.rs:47`, `budget.rs`, ADR 0023/0020). The interop harness checks the base protocol only (`itk/interop/go-sdk-client`, `python_client_vs_rust.py`). `bindings/` holds a *protocol* binding (SLIMRPC), not *language* bindings: there is no pyo3, napi or uniffi anywhere. A Python or Go agent in the tree ignores the lease and budget, so cascades stop at it, and so do deadlines.

**Design.**
1. Publish each extension as a stand-alone spec page with JSON Schema and conformance vectors. Most of the material already exists in the ADRs.
2. Write reference interceptors/middleware for a2a-python and a2a-go (about 200 lines each: read the metadata, cap the timeout, honour the lease renewal).
3. Propose lease, budget and lineage upstream as A2A extensions.
4. Optionally add a `pyo3` wheel of the client's `Delegation` and `CardCache`.

### Doc and claim hygiene (S2, effort S)
- `docs/swarm-scale-findings.md` Finding 8 says the single-writer refusal "does not cross replicas". The code now fences with turn claims, and the test was renamed `the_single_writer_refusal_crosses_replicas` (`tests/multi_replica.rs:554`). Mark Finding 8 as fixed.
- `docs/swarm-orchestration.md` "Next, in order" still lists G2 and G3 as open. G3 is done (fail-closed; `handler/mod.rs:408`). G2 is still open (gap 4).
- Finding 7's `save_appending_history` override for SQL is still open. The `save_artifact_delta` gap mentioned there is closed (`postgres_store/store_impl.rs:239`, `sqlite_store/mod.rs:796`).

---

## 3. Production-ready line

**Must ship before any public "production-ready for agent swarms / delegation at scale" claim:**

| # | Gap | Effort |
|---|---|---|
| 1 | Overload and halt classified correctly on JSON-RPC/WS, plus Retry-After on REST | S |
| 2 | HTTP TLS injection (private CA, mTLS) plus a shared `HttpPool` with per-host caps and h2c | M |
| 3 | Orphan reconciler, and renewal refused for a claimless task | M |
| 4 | Delegation resume on stream loss, plus a durable journal and `reattach` (G2) | M |
| 5a | Renewal with `history_length=0` (immediate) and subscriber caps plus backoff | S |
| 6 | Retry budget and deadline-aware retries (the breaker can follow) | M |
| 7 | `/ready` reflects saturation and halt | S |
| — | Doc corrections above | S |

Rationale: without 1, 3 and 4 the system reports wrong answers (transient errors shown as final, dead work shown as alive, live work killed). Without 2, the documented mTLS server feature has no matching client, and fleets hit FD and port limits. 6 and 7 stop overload from cascading.

**Roadmap (state it publicly as not yet provided):** 5b lease groups and the server sweeper (S0 only at 10^5–10^6 leased tasks per cluster), 8 card cache, 9 queued and priority admission, 10 lineage and job-scoped halt, 11 tenant and cost metrics, 12 client LB and hedging, 13 byte budgets and blob offload, 14 push outbox, 15 discovery companion crate, 16 rate limiter scaling, 17 inference limiter, 18 the `ctx.delegate` helper, 19 cross-SDK extension specs and shims.

**Unmeasured, and worth measuring before choosing the order within the roadmap:**
- store operations per second from lease renewal and watchers at 10^4 leased tasks per replica;
- idle-subscriber poll load at 10^4 parked tasks;
- sockets held by one supervisor with 10^4 plaintext SSE delegations;
- the supervisor-kill swarm run (G2), which the swarm document lists as not done.
