<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> **Frozen audit input, 2026-11-25, of the tree at `2058058d`.** Not maintained. Written by a delegated
> read-only audit for [`docs/program/README.md`](../README.md); its VERIFIED and INFERRED labels are the auditor's own.
> Re-check an anchor before acting on it. `UPSTREAM/` is a local clone of the named upstream repository.
>
> **Correction, 2026-11-25, from a mechanical check of every citation in these seven files.** This file cites
> `drop.rs` at lines 91 to 143; `crates/a2a-protocol-client/src/delegation/drop.rs` has 61 lines. What the
> citations describe is there (the `Drop` impl starts at line 13 and spawns the cancel on the current
> runtime near its end); only the numbers are wrong. Of the in-repository `path:line` citations across the
> seven files, these were the only ones beyond the end of their file; a citation by bare file name that
> matches several files was counted as resolving when any of them is long enough, which is weaker.

# 03 — Swarm / fleet / orchestration audit of a2a-rust 0.16.0

Read-only audit at `2058058d` (HEAD of `/home/user/a2a-rust`). Nothing was built or run; every claim comes from reading files. Paths are relative to `/home/user/a2a-rust`.

Labels: **VERIFIED** = I opened the file and read the lines cited. **INFERRED** = a conclusion drawn from verified code that I did not trace end to end. **ADR-ONLY** = stated by a design record; I did not open the implementing code.

Short path prefixes used below: `client/` = `crates/a2a-protocol-client/src/`, `server/` = `crates/a2a-protocol-server/src/`, `types/` = `crates/a2a-protocol-types/src/`.

---

## 0. Headline findings

1. **There is no swarm abstraction above one child.** The only orchestration type is `a2a_protocol_client::delegation::Delegation` (one parent, one child). `DelegationGroup` (ADR 0033) is "Proposed. Nothing here is built or run" (`docs/adr/0033-group-delegation.md:7`) and no such type exists (grep in §3, G2). Fan-out, join rules, budget split, retries, placement, result collection and recovery are all application code.
2. **The primitives are strong and mostly real**: cascade cancel through a handle, lease, budget, lineage with root halt, journal with resume/reattach/hand-off, steering, admission gate, load signal, client balancing/ejection/hedging, card cache. All compile unconditionally (no feature flag) except the SQL stores.
3. **Three of the five delegating examples do not use `Delegation` at all** (`examples/agent-team`, `examples/incident-response`, `examples/multi-lang-team` call `send_message` directly), so a cancel of their coordinator task does not reach the children. Only `examples/swarm` and two `scripts/live` binaries use it. No example or live script uses `RequestContext::child_lineage`, `with_halt_store`, `with_admission`, `with_balancing` or `CardCache` (grep in §2).
4. **Design records disagree with the code in at least seven places** (§7).
5. **Five correctness risks traced in code with no covering test** (§4): the hand-off continuation re-sends the original relative budget; a halt refusal is retryable and ejects the replica on the HTTP+JSON path while the SDK's own client has no `HALTED` handling; `Canceled` does not say whether it was a caller's cancel, a halt or a lease lapse; the journal has no ownership claim, so two supervisor replicas with one owner name both reattach; idempotency keys are not added for a client built with `ClientBuilder::new(url)`, which is how every example builds one.
6. **The standing project decision is against a runtime**: "no registries, schedulers, orchestration DSLs or mesh" (`docs/swarm-orchestration.md:290-296`; ADR 0029 Context; ADR 0033 "What stays out"). Any first-class swarm layer must be argued against that decision, or scoped to fit it. §6 proposes a scope that fits.

---

## 1. Capability inventory

Feature flags are from the four `Cargo.toml` `[features]` tables (VERIFIED; dumped with `awk '/^\[features\]/{p=1;next} /^\[/{p=0} p' crates/a2a-protocol-<c>/Cargo.toml`). "none" means the module is compiled in every build: `client/lib.rs:126-145` declares `pub mod balance; pub mod delegation; pub mod discovery; pub mod retry;` with no `cfg`; `types/lib.rs:120-150` declares `approval, budget, delegation_journal, failure, idempotency, lease, lineage, steering` with no `cfg`; `server/lib.rs:116-139` declares `admission, approval, rate_limit, steering` with no `cfg`.

Bindings column: JSON-RPC (J), HTTP+JSON (R), gRPC (G), WebSocket (W), SLIMRPC (S). Every extension rides in `Message.metadata` / `Message.extensions`, so it is carried by any binding that carries a message; the column records what the design records say was tested.

| # | Primitive | Public surface (exact paths) | Feature | Stores | Bindings tested | Extension URI | ADR | Tests | Documented limits |
|---|---|---|---|---|---|---|---|---|---|
| 1 | **Delegation handle** (cascade cancel on cancel / drop / lost stream) | `a2a_protocol_client::delegation::{Delegation, Delegated, Outcome, DEFAULT_ID_WAIT}`; `Delegation::start(client: Arc<A2aClient>, params: MessageSendParams) -> ClientResult<Self>` (`client/delegation/mod.rs:184`); `next_event(&mut self) -> Option<ClientResult<StreamResponse>>` (:261); `wait<F: Future<Output=()>>(self, cancel: F) -> Delegated` (:340); `cancel(self) -> Delegated` (:403); `detach(self) -> Option<String>` (:435); `task_id`, `is_settled`, `idempotency_key`, `with_id_wait` | none | n/a | J (tests run JSON-RPC); gRPC idle resume in `scripts/live/delegation/src/bin/grpc_idle.rs` | none | swarm-orchestration G1-A | `crates/a2a-protocol-client/tests/delegation_tests.rs`, `delegation_outcome_tests.rs`, `delegation_spans.rs` | Does not cover a crashed parent (`mod.rs:29`); a drop outside a Tokio runtime cancels nothing (`drop.rs:99-105`); a child never named is `Outcome::Unreachable` (`mod.rs:416-418`). `Delegated` carries only `task_id` and `Outcome` (`outcome.rs:77-82`): **no artifacts, no final `Task`**. |
| 2 | **DelegationOptions** (compose lease + budget + journal + lineage + resume + hand-off) | `delegation::DelegationOptions::{new, with_lease(Duration), with_budget(ChildGrant), with_journal(Arc<dyn DelegationJournal>, owner, agent), with_journal_record(journal, record), with_lineage(Lineage), with_resume(ResumePolicy), with_id_wait(Duration), with_hand_off(HandOffPolicy)}` (`options.rs:74-166`); `Delegation::start_with(client, params, options)` (:182); `reattach_with(client, tenant, task_id, last_event_id, options)` (:241); `start_leased` (:294); `start_budgeted` (:322) | none | n/a | J | — | 0030 amendment 2026-11-03 | `tests/delegation_options_tests.rs`, `delegation_resume/options.rs` | `reattach_with` refuses a budget and a new journal record (`options.rs:248-262`). |
| 3 | **Lease** (child stops when parent stops renewing) | types: `a2a_protocol_types::lease::{Lease, LEASE_EXTENSION_URI, LEASE_RENEWAL_ONLY_URI, LEASE_METADATA_KEY}` (`types/lease.rs:57-114`). client: `Delegation::start_leased`, `A2aClient::renew_lease`, `renew_lease_status` (used at `client/delegation/renewal.rs:66`). server: `RequestHandlerBuilder::with_lease_store` (`server/builder.rs:585`), `with_max_lease_ttl` (:681), `handler::lease::{MIN_LEASE_TTL = 1 s, DEFAULT_MAX_LEASE_TTL = 3600 s}` (`server/handler/lease.rs:38-40`), `store::{LeaseStore, InMemoryLeaseStore, PostgresLeaseStore}` | none (`postgres` for the shared store) | memory, PostgreSQL. **No SQLite lease store** (VERIFIED: `ls server/store/lease` = `mod.rs`, `postgres.rs`) | J, R; WebSocket renews only on a connection that asked for the extension at upgrade (ADR 0020 Consequences) | `https://a2a-rust.com/extensions/lease/v1` and `.../lease/v1/renewal-only` | 0020, 0044 | `server/handler/lease/{tests,watch_tests,cost_tests,renewal_only_tests}.rs`; `client/tests/delegation_lease_tests.rs`; `client/src/delegation/renewal_tests.rs` | One `GetTask` per child per ttl/3 (ADR 0044 "What is unchanged"). A store that cannot be read counts as a live parent (`server/handler/lease.rs:20-23`). Restart window: 15 s leases, a restart at 9.5 s kept 50/50, at 12 s kept 0 (ADR 0030 amendment 2026-11-04). `PostgresLeaseStore` was not in the live proof (same). A peer that ignores the extension ignores the lease. |
| 4 | **Budget** (relative deadline + named meters, reserved per child, refunded on settle) | `a2a_protocol_types::budget::{Budget, BudgetStatus, BudgetHandle, ChildGrant, Exhausted, BUDGET_EXTENSION_URI, BUDGET_METADATA_KEY, MAX_METERS = 16, MAX_METER_NAME_LEN = 64, TOKENS_METER, MIN_GRANT_RESERVE, MAX_GRANT_RESERVE}`; `BudgetHandle::grant(&self, request: &Budget) -> Result<ChildGrant, Exhausted>` (`types/budget/handle.rs:229`); `charge(&self, meter, amount) -> Result<Option<u64>, Exhausted>` (:156); `ChildGrant::{budget, release, settle(Option<&BudgetStatus>)}` (:334-366). server: `RequestContext::budget() -> Option<&BudgetHandle>` (`server/request_context.rs:220`); always honoured, no switch (`server/handler/budget.rs:8-9`). client: `Delegation::start_budgeted` | none | **per process, per turn, no store** (ADR 0023 "Scope") | J, R; Python interop sends doubles (ADR 0049) | `https://a2a-rust.com/extensions/budget/v1` | 0023 | `server/handler/budget/tests.rs`; `types/budget/handle/tests`; `client/tests/delegation_budget_tests.rs`; `crates/a2a-protocol-sdk/tests/injection_containment` (`delegation::fan_out_stops_when_the_parents_budget_is_reserved`) | Does not cross replicas. `GetTask` pollers do not see spend (status-update metadata only). A child that under-reports gets a larger refund. A reattach cannot restore a grant. Only the `tokens` meter is charged automatically, by `ToolLoop`. |
| 5 | **Lineage + root halt** | `a2a_protocol_types::lineage::{Lineage, LineageNode, tree, LINEAGE_EXTENSION_URI, LINEAGE_METADATA_KEY, MAX_LINEAGE_DEPTH = 16, MAX_LINEAGE_ID_BYTES = 1024}`; `Lineage::{origin, child(&self, parent_task_id) -> A2aResult<Self>, validate, attach, read, of_task, record_on}` (`types/lineage.rs:100-183`); `tree(root: &str, tasks) -> Vec<LineageNode>` (`types/lineage/tree.rs:42`). server: `RequestContext::{lineage, child_lineage}` (`request_context.rs:238,253`); `HaltScope::Root { tenant, root }` (`server/handler/halt.rs:63-68`); `RequestHandler::running_in_root(&self, tenant, root) -> Vec<(TaskId, Lineage)>` (:354). client: `DelegationOptions::with_lineage` | none | lineage recorded in `Task.metadata`; root halts go through the halt store | J, R; Python and Go interop clients carry it (ADR 0034 "What was checked") | `https://a2a-rust.com/extensions/lineage/v1` | 0034, 0049 | `server/handler/halt/root_tests.rs`; `server/handler/lineage/tests`; `types/lineage/{tests,vectors_tests}.rs`; `client/tests/delegation_lineage_tests.rs` | No stored root index and no `rootId` filter on `ListTasks` (ADR 0034 "Not built"). A claim, never an authorization. Not on metric labels. A halt does not stop parked tasks. A two-replica PostgreSQL run through the handler "is not written"; the swarm example with a killed parent "is not run" (ADR 0034). **The child lineage is not attached automatically**: the executor must pass `ctx.child_lineage()?` to `with_lineage`. |
| 6 | **Halt (kill switch)** | `RequestHandler::halt(&self, scope: HaltScope, by: &str, reason: &str) -> HaltReport` (`server/handler/halt.rs:201`); `resume(&self, scope, by) -> ServerResult<bool>` (:251); `HaltScope::{All, Tenant(String), Root{..}}`; `HaltReport { stopped: Vec<String>, persisted: Option<bool> }`; `is_halted_everywhere`, `halt_reason`, `halt_reason_for_root`; `RequestHandlerBuilder::{with_halt_store, with_halt_poll_interval}` (`builder.rs:558,568`); `store::{HaltStore, HaltSignal, StoredHalt, InMemoryHaltStore, SqliteHaltStore, PostgresHaltStore}`; `server::{HALTED_REASON, A2A_RUST_ERROR_DOMAIN}`; `conformance::halt::{check_store, check_handlers}` | none (`sqlite` / `postgres` for those stores; `conformance` for the grader) | memory, SQLite (poll only), PostgreSQL (`LISTEN/NOTIFY` + poll) | refusal is machine-readable on every binding (ADR 0034 amendment 12) | none; the refusal carries `google.rpc.ErrorInfo{reason:"HALTED", domain:"a2a-rust.com"}` | 0019, 0024, 0034, 0049 | `server/handler/halt/{tests,store_tests,push_tests,root_tests,window_tests}.rs`; `server/tests/halt_e2e.rs`, `kill_switch_conformance.rs`, `cross_replica_cancel/` | **A Rust method only: no wire or admin route** (grep `'"/halt\|/resume"\|"/admin'` in `server/` returns nothing; ADR 0049 "What is not built": "A halt sent over the wire to another agent"). Poll default 1 s. An executor that ignores its token keeps running. A mixed fleet: "do not give a root halt until every replica sharing the halt store runs 0.16" (ADR 0034 amendment 8). Halts never expire (no `ttl`/`expire` in `server/store/halt/mod.rs` or `handler/halt/sync.rs`). |
| 7 | **Journal, resume, reattach, supervisor hand-off** | `a2a_protocol_types::delegation_journal::{DelegationJournal, DelegationRecord, InMemoryDelegationJournal}`; trait methods `begin, progress, settle, open(owner), purge_settled` (`types/delegation_journal.rs:153-195`); `delegation::{ResumePolicy, HandOffPolicy, HAND_OFF_METADATA_KEY}`; `Delegation::{start_journaled, with_journal, journal_id, reattach, last_event_id, with_resume, wait_or_hand_off<C,H>(self, cancel: C, hand_off: H) -> Delegated}`; `Outcome::HandedOff`. SQL: `a2a_protocol_server::store::{PostgresDelegationJournal, SqliteDelegationJournal}` (`server/store/delegation_journal/{postgres,sqlite}.rs`) | none (`postgres` / `sqlite`) | memory (types crate), SQLite, PostgreSQL | J; gRPC resume live-proved | none (the hand-off marker `a2a-rust.com/hand-off` has no URI) | 0030 | `client/tests/delegation_journal/{lifecycle,records,progress,hand_off}.rs`; `client/tests/delegation_resume/{resume,reattach,options}.rs`; `server/store/delegation_journal/tests.rs`; live: `scripts/live/delegation/` | **No recovery driver** (ADR 0030: "Not built, and why: a recovery driver"). The record holds no message text, no lease ttl, no lineage, no group id (`types/delegation_journal.rs:60-91`). `owner` is not enforced (ADR 0030 Consequences). Position lags up to 2 s (`client/delegation/journal.rs:25`). gRPC and WebSocket streams carry no event ids (`client/delegation/resume.rs:47-48`). |
| 8 | **Turn hand-off on server shutdown** | `AgentExecutor::resumes_after_hand_off(&self) -> bool` (`server/executor.rs:164`, default false); `HandlerLimits::hand_off_window` (default 600 s); client side continues automatically under `HandOffPolicy` (default 3 continuations, 1/2/4 s; `client/delegation/continuation.rs:53-62`) | none | needs a store with turn claims (SQLite, PostgreSQL) | every binding (ADR 0054) | marker only | 0054 | `server/handler/messaging/hand_off_tests.rs`; `client/src/delegation/continuation_tests.rs`; `scripts/live/delegation/handoff_live.py` | Not a crash; not a reattached delegation; not handlers without claims (ADR 0054; ROADMAP item 9). `ToolLoopExecutor` does not opt in (only the trait default at `executor.rs:164` exists in `server/src`, grep `resumes_after_hand_off`). |
| 9 | **Steering** | types: `a2a_protocol_types::steering::{STEERING_EXTENSION_URI, STEERING_METADATA_KEY, Delivery, Receipt, declare, declares_extension}`. server: `a2a_protocol_server::{Inbox, Steered, SteeringConfig}`, `RequestHandlerBuilder::with_steering` (`server/builder/steering.rs:39`), `RequestContext::inbox()`; `ToolLoopExecutor::with_steering`. client: `delegation::{Steering, SteerHandle, ChildSteering, steer_all}`, `Delegation::{steer, steer_handle}` (`client/delegation/steer.rs`) | none | cross-replica through `TurnClaimStore::{hand_steering, peek_steering, ack_steering, steering_signals}`: memory, SQLite, PostgreSQL | tests drive J and R only (ADR 0036 "Not built") | `https://a2a-rust.com/extensions/steering/v1` | 0036 | `server/handler/messaging/steer_{tests,replica_tests,bounds_tests,duplicate_tests,tenant_tests}.rs`; `server/tests/steering/`, `steering_tool_loop.rs`; `client/tests/delegation_steering_tests.rs`; `scripts/live/steering/` | Inbox 16 messages, 64 KiB each. `handed-off` includes a dead replica for up to the claim's ttl (ADR 0036 "Not built"; ROADMAP item 8). Not in `Task.history`. **No schema/vectors folder** (`ls crates/a2a-protocol-types/extensions/` has no `steering-v1`). |
| 10 | **Admission gate** | `a2a_protocol_server::admission::{Admission, TenantShare, AdmissionLoad}`; `Admission::new(capacity).with_max_queued(n).with_max_wait(d).with_default_share(s).with_tenant(name, share)` (`server/admission/mod.rs:134-171`); `RequestHandlerBuilder::with_admission` (`builder.rs:529`); `RequestHandler::admission_load()` | none | process-local | all (server side) | — | 0045 | `server/handler/admission_tests.rs`; `server/admission/` unit tests | Not built: caller-set priority, preemption, **fleet-wide admission** (ADR 0045 "Not built"). |
| 11 | **Load signal and readiness** | `RequestHandlerBuilder::with_load_reporting` (`builder.rs:540`); `handler::load::{LoadReport, LoadReporting, LOAD_HEADER = "endpoint-load-metrics"}` (`server/handler/load.rs:22-36`); `RequestHandler::load()` (:119); `GET /load`; `HandlerLimits::with_saturation_readiness`; `begin_draining`, `is_draining`, `readiness` (`server/handler/introspection.rs:110-137`) | none | process-local | HTTP dispatchers and axum; **not gRPC** | — (ORCA header format) | 0046 | `server/handler/load_tests.rs`; `server/tests/load_signal.rs`, `axum_load_signal.rs` | Per replica and instantaneous. **Nothing in this repository reads the header** (grep `endpoint-load-metrics\|application_utilization` outside tests matches only `server/handler/load.rs`). |
| 12 | **Client balancing, ejection, hedging** | `a2a_protocol_client::balance::{Endpoint, EndpointSource, StaticEndpoints, DnsEndpoints, Ejection, Hedge, BalancePolicy, DEFAULT_DNS_REFRESH}` (`client/balance.rs:52-417`); `ClientBuilder::with_balancing(source, policy)`; `HttpPool`, `HttpPoolBuilder::{with_ejection, with_max_in_flight_per_origin, with_retry_budget, ...}` (`client/pool/mod.rs:261-399`); `HttpPool::origins() -> Vec<OriginSnapshot>`; `ClientError::CircuitOpen` | `tls-rustls` gates `pool` and pinned endpoints (`client/lib.rs:115-116`) | process-local | J and R only; `build_grpc` and custom transports refuse it (ADR 0050 decision 7) | — | 0032, 0050 | `client/tests/balancing/{distribution,ejection,hedging,pinning,pool,premise}.rs`; `client/src/balance/tests*` | Selection is fewest-in-flight by the client's own count. Not built: active health checks, DNS SRV, load from the server, OTel instruments (ADR 0050 "Not built"). |
| 13 | **Card cache at fleet scale** | `a2a_protocol_client::discovery::{CardCache, CachingCardResolver, CardFetchOptions}`; `CardCache::{new, with_pool, with_stale_if_error, with_refresh_jitter, with_max_entries, resolver, resolve}` (`client/discovery/cache.rs:56-271`); `single_flight::SingleFlight` (crate-private module) | none (`with_pool` needs `tls-rustls`) | process-local | n/a | — | 0051 | `client/tests/card_freshness.rs`, `card_revalidation.rs`, `discovery_tests.rs` | No background refresh or `watch()`. The 512-fetch swarm arm "not run here" (ADR 0051). |
| 14 | **Local discovery (mDNS)** | crate `a2a-protocol-discovery`: `{Advertisement, Announcer, Scheme, Browser, DiscoveryEvent, discover(window), DiscoveredAgent, DiscoveryError, txt}` (`crates/a2a-protocol-discovery/src/lib.rs:62-86`) | own crate, **`publish = false`** (`Cargo.toml:12`); not re-exported by the SDK | n/a | n/a | — | 0029 | `crates/a2a-protocol-discovery/tests/mdns_round_trip.rs` | One link only: "no discovery across subnets, no catalogue, no capability search, no health" (ADR 0029). ADR 0031 (`AgentSource`, `StaticSource`, `DnsSdSource`, `KubernetesSource`, `Merged`) is "Proposed. Nothing here is built" and absent from the code. |
| 15 | **Failure class** (retry by class) | `a2a_protocol_types::failure::{FailureClass, set_class, class_of, set_error_class, error_class, FAILURE_EXTENSION_URI}`; `FailureClass::{is_retryable, needs_human}` (`types/failure.rs:137,143`); `delegation::Outcome::Failed { status, class }` | none | n/a | all | `https://a2a-rust.com/extensions/failure/v1` | — | `server/tests/failure_class_tests.rs`; `types/failure/tests.rs` | The SDK does not retry a failed child; the caller does. |
| 16 | **Idempotency across a hop** | `a2a_protocol_types::idempotency::{key_of, set_key, validate_key, IDEMPOTENCY_EXTENSION_URI, MIN_KEY_LEN = 16, MAX_KEY_LEN = 255}`; `A2aClient::honours_idempotency()` (`client/client.rs:178`); `ClientBuilder::with_peer_honouring_idempotency(bool)` (`client/builder/mod.rs:960`); tool-call keys in `tool_loop/idempotency.rs` | none (`tool-loop` for tool keys) | task stores: memory, SQLite, PostgreSQL, tenant-aware forms | all | `https://a2a-rust.com/extensions/idempotency/v1` | 0026 | `client/tests/delegation_idempotency_tests.rs`; `server/handler/messaging/idempotency_tests/`; `server/tests/idempotency_index_cap.rs` | A key is added only when the client "knows" the peer honours it: true from `ClientBuilder::from_card` when the card declares the URI (`builder/mod.rs:298-301`), **false for `ClientBuilder::new`** (`builder/mod.rs:194`). |
| 17 | **Originating principal** (token exchange, actor chain) | client: `token_provider::exchange::{TokenExchange, ExchangeRequest, ExchangedToken}`, `TokenExchange::exchange(&self, &ExchangeRequest) -> ClientResult<ExchangedToken>` (`client/token_provider/exchange.rs:316`). server: `JwtAuthInterceptor` reads `act`; `CallContext::actors()` (ADR-ONLY for the server half) | server half needs `auth-jwt` | n/a | all | — (RFC 8693) | 0021 | `client/src/token_provider/exchange/tests.rs` | The exchanged token is per principal, so it needs a client per principal; `Delegation` takes one `Arc<A2aClient>` and has no per-call credential. Chain capped at 16 hops. |
| 18 | **Approval gate** (human in the loop, quorum of approvers) | `a2a_protocol_types::approval::{ApprovalRequest, ApprovalDecision, Decision, action_digest, APPROVAL_EXTENSION_URI}`; `a2a_protocol_server::approval::{ApprovalGate, VerifiedApproval}`; `ApprovalGate::{new, with_review_grants, with_quorum(n), with_approvers, allowing_self_approval}` (`server/approval.rs:125-178`); `RequestHandlerBuilder::with_approval_gate` (`builder.rs:510`); `RequestContext::approval()` | none | task store | J, R | `https://a2a-rust.com/extensions/approval/v1` | (not in the list given) | `server/tests/approval_review_grants.rs`; `server/handler/review_tests.rs`; injection harness `approval::*` | One task on one server. Across a tree the child parks `input-required`, the parent's `Delegation` reports `Outcome::Interrupted` and the handle is consumed; relaying the question upward is application code. |
| 19 | **Tool loop, skills** | `a2a_protocol_server::tool_loop::{ToolLoop, ToolLoopExecutor, Tool, ToolModel, ToolSet, OpenAiChatModel}`; `server::skills` | `tool-loop`, `skills` | optional journal (`tool_loop/journal`) | n/a | — | 0022, 0026, 0035, 0048 | `server/tests/tool_loop/`, `tool_loop_*`; injection harness | A single-agent loop. It reaches other agents only through tools the adopter writes. `InferenceLimit` is per process (`tool_loop/limit.rs`). |
| 20 | **Turn claims** (single writer across replicas, reaper) | `RequestHandlerBuilder::{with_turn_claims, without_turn_claims, with_turn_claim_timing, with_replica_name}` (`builder.rs:624-670`); `store::claim::{TurnClaimStore, ClaimLost, current_fence}` | `sqlite` / `postgres` | memory (tests), SQLite, PostgreSQL | all | — | (0054 refers) | `server/tests/turn_claims/`, `claim_lapse.rs`, `multi_replica.rs`, `replica_processes.rs`, `soak_multi_replica.rs` | A cancel reaches another replica at the owner's next renewal (1 s) unless a halt store pushes it (`docs/handoff.md:657-658`). A dead owner's finished task takes three claim lifetimes (ROADMAP item 7). |
| 21 | **Shared rate limit** | `a2a_protocol_server::rate_limit::{RateLimitInterceptor, RateLimitConfig, RateLimitCounter}`, `RateLimitInterceptor::with_shared_counter(Arc<dyn RateLimitCounter>)` (`server/rate_limit/mod.rs:319`), `PostgresRateLimitCounter` | `postgres` for the bundled counter | memory, PostgreSQL | all | — | — | `server/tests/multi_replica.rs`, `rate_limit_wire.rs` | A request rate per caller per server deployment. No per-swarm quota; the fleet-wide inference quota (S3) "needs a decision first" (`ROADMAP.md:95`). |
| 22 | **Push outbox, webhook receiver** | `RequestHandlerBuilder::with_push_outbox` (`builder.rs:442`); `a2a_protocol_client::webhook::{WebhookReceiver, WebhookOutcome, WebhookRejection}` | `webhook` (client, default) | outbox: SQL stores (ADR 0037, ADR-ONLY) | n/a | — | 0037, 0043 | `server/tests/push_outbox.rs`; `client/tests/webhook_receiver*.rs` | ROADMAP item 11 lists the rest. `Delegation` does not use push; it holds a stream per child. |
| 23 | **SLIMRPC multicast** (one message, many agents) | `bindings/a2a-protocol-slimrpc`: `multicast::{SlimRpcMulticast, MemberOutcome<T>, MulticastOutcome<T>}`; `SlimRpcMulticast::{from_app, from_app_with_connection, members, send_message -> ClientResult<MulticastOutcome<SendMessageResponse>>, stream_message}` (`src/multicast/mod.rs:72-282`); `MulticastOutcome::{all, succeeded, failed, is_unanimous}` (`outcome.rs:39-59`) | binding crate (`otlp` default) | n/a | S only | — | spec `spec/v1/slimrpc-multicast.md` | `bindings/a2a-protocol-slimrpc/tests/{multicast,multicast_backpressure}.rs` | Only `SendMessage` and `SendStreamingMessage`. It waits for every invited member. **It is a second, unrelated fan-out surface**: it shares nothing with `Delegation` (no cancel on drop, no lease, no budget split, no journal). |
| 24 | **Testing tools** | `a2a_protocol_client::testing::{ScriptedPeer, RunningPeer, Binding, Script}` (`client/testing/mod.rs:56-208`); `a2a_protocol_server::conformance::{check, halt::{check_store, check_handlers}}`; `examples/harness` | client `testing`; server `conformance`. **Neither is forwarded by `a2a-protocol-sdk`** (its `[features]` table has no such entry) | n/a | per binding | — | — | `client/tests/scripted_peer_tests.rs`; `server/tests/handler_conformance/`, `kill_switch_conformance.rs` | One hostile peer, or one executor graded alone. No multi-agent harness (§3, G23). |

Extension schemas and vectors: `crates/a2a-protocol-types/extensions/{approval-v1, budget-v1, failure-v1, hand-off, idempotency-v1, lease-v1, lineage-v1, provenance-v1}/` (VERIFIED by `ls`). The wire contract page is `book/src/reference/swarm-extensions.md`.

---

## 2. The glue audit

Commands: `wc -l` over each example's files (output in the session); line ranges are from reading the files with `cat -n`.

Which examples use which primitive (command: `grep -rnE 'Delegation::|child_lineage|\.grant\(|with_lease_store|with_halt_store|with_admission|with_steering|with_balancing|CardCache|with_turn_claims|HaltScope|steer_all|with_load_reporting|with_approval_gate|start_with\(|reattach_with\(|DelegationJournal' --include=*.rs examples scripts/live`):

- `Delegation::start`: `examples/swarm/src/supervisor.rs:74`; `scripts/live/delegation/src/bin/grpc_idle.rs:106`.
- `Delegation::start_budgeted`: `examples/live-model/src/agents.rs:205`; `scripts/live/campaign/live/agent/src/coordinator.rs:62`.
- `Delegation::start_with` / `reattach_with`: only `scripts/live/delegation/src/bin/supervisor.rs:156,223,263`.
- **Zero matches** in `examples/` and `scripts/live/` for `child_lineage`, `with_halt_store`, `with_admission`, `with_balancing`, `CardCache`, `with_turn_claims`, `steer_all`, `with_load_reporting`.
- `examples/agent-team`, `examples/incident-response`, `examples/multi-lang-team`: no `Delegation` at all.

### 2.1 `examples/swarm` (750 lines of Rust: `wc -l examples/swarm/src/*.rs`)

| Concern | File:lines | Lines | Notes |
|---|---|---|---|
| Per-child send, follow, cancel wiring | `supervisor.rs:64-107` | 44 | 23 of them (85-107) are the detach control arm |
| Outcome mapping (10 `delegation::Outcome` variants to 4) | `supervisor.rs:46-60`, `108-117` | 25 | every orchestrator redoes this `match` |
| Retry of children by failure class, on a different worker | `supervisor.rs:120-150` | 31 | one retry, no backoff, no retry budget |
| Placement (round-robin counter over a `Vec<Arc<A2aClient>>`) | `supervisor.rs:38-40`, `128`, `134`, `176` | 6 | no load, no skill, no health |
| Fan-out and backpressure (`Semaphore` + `JoinSet`) | `supervisor.rs:171-185` | 15 | exactly ADR 0033 items 1 and 2 |
| Fan-in and aggregation (tally, latencies, summary artifact) | `supervisor.rs:186-203`, `208-217` | 28 | child artifacts are discarded; only counts survive |
| Cancellation cascade wiring | `supervisor.rs:67`, `84`, `172`, `204-207` | 7 | relies on passing `ctx.cancellation_token` to `wait` |
| Coordinator fan-out to supervisors and merge of their summaries | `main.rs:104-152` | 49 | a second, different fan-out, over blocking `send_message`, with no cancel |
| Topology construction (two tiers, pools) | `main.rs:51-89` | 39 | |
| Standing an agent up, card, client | `topology.rs:17-48` | 32 | discovery is "the URL I just bound" |
| Partial-failure policy | implicit in `supervisor.rs:191-203` | — | "report everything, never fail the batch" |
| Supervisor restart/resume, budget, lineage, lease, tracing wiring, progress roll-up | absent | 0 | the example does none of them |

Orchestration plumbing: about 276 of 750 lines (37%); in `supervisor.rs` alone about 152 of 220 (69%). What a group abstraction would remove outright: `supervisor.rs:46-150` and `171-203`.

### 2.2 `scripts/live/delegation/src/bin/supervisor.rs` (372 lines): the only code that composes journal + lease + reattach + hand-off

| Concern | Lines | Count |
|---|---|---|
| One child: options, start, wait until named, wait or hand off | `144-185` | 42 |
| Fan-out and fan-in with a `JoinSet` and a tally | `187-208` | 22 |
| Recovery loop, serial form (the one that lost 49 of 50 leased children) | `210-237` | 28 |
| Recovery loop, correct form (reattach all, then wait; keep going on an error) | `248-289` | 42 |
| Shutdown-signal plumbing into `hand_off` | `126-142` | 17 |
| Executor wiring of token as `cancel` and shutdown as `hand_off` | `294-313` | 20 |
| Outcome labels | `75-95` | about 20 |

About 191 of 372 lines are supervisor plumbing. ADR 0030 records three defects that live in exactly this glue (serial reattach, late first renewal, `?` dropping held handles). Two were in the documented application loop, not in the SDK. That is the evidence that this loop belongs in the SDK. Also VERIFIED at `:117`: the client is `ClientBuilder::new(agent)`, so no idempotency key is added, and at `:229-233` a record with no task id is logged and **not** resent, so the documented "resend under the record's key" path has no live proof.

### 2.3 `examples/agent-team` (coordinator 214 lines; whole example about 9,000)

| Concern | File:lines | Lines |
|---|---|---|
| Placement and client construction per agent and binding | `executors/coordinator.rs:39-65` | 27 |
| Sequential fan-out and result aggregation (artifact text extraction) | `executors/coordinator.rs:122-214` | 93 |
| Health probing of peers by card fetch plus `ListTasks` | `executors/health_monitor.rs:131-164` | 34 |
| Roster allow-list and fan-out cap (`MAX_CHECKS = 16`) | `executors/health_monitor.rs:23`, `49-62` | about 15 |

No cancellation cascade (blocking `client.send_message` at `:136,168,198`), no retry, no deadline, no lineage. About 120 of 214 coordinator lines are plumbing.

### 2.4 `examples/incident-response` (`agents.rs` 291 lines)

| Concern | File:lines | Lines |
|---|---|---|
| Delegate one question: build a client, timeout, first-artifact extraction, degrade to a string | `agents.rs:157-181` | 25 |
| Pipeline topology (logs, then runbook, then synthesis) with progress roll-up | `agents.rs:244-261` | 18 |
| Human-in-the-loop park and resume, with state kept in a `Mutex<HashMap>` | `agents.rs:146-150`, `189-198`, `208-216`, `225-242` | about 40 |
| Client construction across four bindings | `serving.rs:249-295` | 47 |

A client is rebuilt per call (`agents.rs:158`); no cascade (`send_message` at `:167`); partial failure is "inline text".

### 2.5 `examples/multi-lang-team` (`main.rs` 453 lines)

| Concern | Lines | Count |
|---|---|---|
| Static roster as discovery | `73-100` | 28 |
| Health probe at startup | `316-331` | 16 |
| Per-worker call: timeout and result extraction | `125-147` | 23 |
| Fan-out with bare `tokio::spawn`, fan-in in input order, partial-failure policy | `219-245` | 27 |

The spawned calls are detached tasks: a cancel of the coordinator's task neither cancels nor aborts them.

### 2.6 `scripts/live/campaign/live/agent/src/coordinator.rs` (123 lines)

Budget grant `44-56` (13 lines); outcome mapping `68-92` (25); **a second `get_task` to fetch the child's artifacts** because `Delegated` has none, `93-108` (16).

### 2.7 What a first-class API should absorb, by frequency in the code read

1. Fan-out with a cap, and fan-in in input order (swarm, live supervisor, multi-lang-team, agent-team): 4 of 5.
2. Outcome normalisation and result collection, including the child's artifacts: 5 of 5.
3. Cancel cascade wiring from `ctx.cancellation_token`: needed in 5, present in 2.
4. Retry of a retryable child on another target: 1 of 5 writes it, the others silently lack it.
5. Recovery (reattach all, then wait; tolerate one bad record): 1, and it is the place the recorded defects were.
6. Lineage, budget and trace carried from the `RequestContext` to the child: carried by hand in 2 (budget only), never for lineage.
7. Placement and health: three different hand-rolled forms.

---

## 3. Gap list

Each gap has a status. Greps were run with `grep -rnE '<pattern>' --include=*.rs crates/*/src bindings/*/src | grep -v test` unless another command is given.

| # | Capability | Status | Evidence |
|---|---|---|---|
| G1 | Declarative topology / workflow API (tree, mesh, pipeline, DAG) | **VERIFIED-ABSENT** | Pattern `Workflow\|Pipeline\|struct Dag\|Topology`: 0 matches. Excluded on purpose: ADR 0033 "What stays out: DAGs and planning"; `docs/swarm-orchestration.md:290-296`. |
| G2 | Group of delegations (cap, join rule, budget split, one cancel) | **VERIFIED-ABSENT** (designed) | Pattern `DelegationGroup\|DelegationSet\|JoinSet<.*Delegat`: 0 matches in `crates bindings examples scripts`. ADR 0033 status "Proposed. Nothing here is built or run". Only `steer_all` exists (`client/delegation/steer.rs:191`). |
| G3 | Supervisor abstraction with restart strategies (one-for-one and so on) | **VERIFIED-ABSENT** | Pattern `(?i)supervis(or\|ion)(Strategy\|_strategy)\|OneForOne\|one_for_one\|RestartStrategy\|restart_policy`: 0. The building blocks exist (journal, `reattach_with`); the driver does not (ADR 0030 "Not built: a recovery driver"). |
| G4 | Scatter-gather with quorum / first-N / deadline | **VERIFIED-ABSENT** on the client | Pattern `scatter\|ScatterGather\|first_ok\|FirstOk\|FailFast\|join_rule`: 2 matches, both unrelated comments. `Quorum` appears 65 times, all in approval (`server/approval/quorum.rs`, `server/handler/messaging/quorum.rs`). `SlimRpcMulticast` waits for all members and has no quorum. |
| G5 | Hedging of delegated work | **PARTIAL** | `balance::Hedge` hedges idempotent unary calls and a keyed `SendMessage` across replicas of one agent (ADR 0050 decision 5); "Streams are not hedged", and `Delegation` is a stream (`mod.rs:199`). So no delegation is ever hedged. |
| G6 | Map-reduce over artifacts | **VERIFIED-ABSENT** | Pattern `(?i)map_reduce\|MapReduce\|fn reduce`: 0. `Delegated` has no artifacts (`client/delegation/outcome.rs:77-82`). |
| G7 | Shared blackboard / context store | **VERIFIED-ABSENT** as an API; **PARTIAL** as a measured pattern | Pattern `(?i)blackboard\|SharedContext\|context_store\|ContextStore`: 0. `docs/swarm-scale-findings.md` measures "a task's event log as a channel" and its limits (peaks at four writers; no live tail when turns are short; shard by context). No helper wraps it. |
| G8 | Agent registry / directory with health and capability query | **VERIFIED-ABSENT** | Pattern `AgentRegistry\|AgentDirectory\|struct Registry\|find_by_skill\|with_skill_filter\|by_skill`: 1 match, an unrelated private struct (`server/store/holds/mod.rs:79`). mDNS is one link (ADR 0029). ADR 0031 sources are unbuilt (pattern `AgentSource\|StaticSource\|DnsSdSource\|KubernetesSource\|struct Merged` in `crates`: 0). Excluded by the "no registries" decision. |
| G9 | Placement by load signal and by skill | **PARTIAL** | The server emits load (`server/handler/load.rs:22,86`). The client selects by its own in-flight count (ADR 0050 decision 2) and does not read the header: "Load from the server ... no server sends it yet" (ADR 0050 "Not built"), which ADR 0046 made untrue on the server side. No selection by `AgentSkill`. ROADMAP item 12 lists ORCA. |
| G10 | Work queues, pull-based workers, work stealing | **VERIFIED-ABSENT** | Pattern `work_steal\|WorkQueue\|work_queue\|pull_task\|dequeue_task\|claim_next`: 0. A2A is push-only; the closest thing is the admission queue (per replica, ADR 0045). |
| G11 | Fleet-wide rate limit and quota (S3) | **PARTIAL** | A shared request counter exists (`server/rate_limit/shared.rs`, `PostgresRateLimitCounter`). Fleet-wide admission is "Not built" (ADR 0045). The fleet inference quota S3 "needs a decision first" (`ROADMAP.md:94-96`; `docs/handoff.md:214`). `InferenceLimit` is per process. |
| G12 | Global kill switch: semantics and propagation latency | **EXISTS within one halt store; PARTIAL across agents** | Within a store: at once locally, about one `NOTIFY` round trip on PostgreSQL, one poll (1 s) on SQLite or memory (ADR 0019, 0024). Across agents with no shared store the halt is not on the wire; it travels as the parent's cancels, one hop at a time (ADR 0049 "What is not built"), which needs every hop's executor to have wired the cascade. No admin route (§1 row 6). Measured only at two hops on one host: 65 ms for 256 and 84 ms for 512 executions (`docs/swarm-orchestration.md:34`). Nothing above 256 workers, deeper than two hops, or across hosts was measured (ADR 0033 Context). |
| G13 | Cascade cancel correctness ("cancellation does not cascade") | **PARTIAL; the doc heading is stale** | Still true of the protocol and of the server: the handler knows nothing about a task's children. It cascades when the executor uses `Delegation` and either passes its token to `wait` or is dropped (`client/delegation/mod.rs:396`, `drop.rs:91-143`). With plain `send_message` nothing cascades (three examples, §2). The heading `docs/swarm-orchestration.md:44` and the R1 table row (`:34`, "Missing in the protocol and the SDK") were not updated after G1-A shipped (same file `:56,304`). |
| G14 | Orphan detection | **PARTIAL** | Covered: a leased child of a dead parent (ADR 0020); a task whose owning replica died (the claim reaper, `server/handler/claims/reaper/`); a parked task past an opt-in expiry (ADR 0056). Not covered: an unleased child of a crashed parent, which runs to its executor timeout (1 h default, `docs/swarm-orchestration.md:40`); a child whose start was ambiguous and whose journal record has no task id and no key (§4, R5); any query "which running tasks have no live parent" (no stored root index, ADR 0034). |
| G15 | Exactly-once / at-least-once semantics of delegated work | **PARTIAL** | At-most-once per key when the peer honours the extension and the client knows it (§1 row 16). Otherwise an ambiguous start is unknowable: `refused()` (`client/delegation/mod.rs:480-488`) separates "the agent answered and refused" from everything else. A handed-back turn runs again from its input (ADR 0054 "Progress inside the turn"). No end-to-end statement of the guarantee exists in one place. |
| G16 | Idempotency keys across hops | **PARTIAL** | Per hop only. Each `Delegation::start` draws a fresh random key (`mod.rs:114-120,188-192`); nothing derives a child key from the parent's key or from the lineage, so a parent turn that is re-run (hand-off, or a retried send without a key) starts new children under new keys. Tool-call keys are scoped to (task id, message id) (`server/tool_loop/executor.rs:137`). |
| G17 | Deadline and budget propagation and enforcement across hops | **EXISTS, opt-in per call; PARTIAL in its edges** | Enforced at every receiving server with no switch (`server/handler/budget.rs:8-9`). Propagation needs the executor to call `ctx.budget().grant(..)` and `start_budgeted`. A plain `Delegation::start` that forwards the inbound message "passes the original `remainingMs` on unchanged" (ADR 0023 Consequences). Not across replicas. See §4 R1 for the hand-off case. |
| G18 | Lineage depth limit | **EXISTS** | `MAX_LINEAGE_DEPTH = 16` (`types/lineage.rs:74`), refused at the parent (`Lineage::child`, `:116-124`) and at the server before work (`server/handler/lineage.rs`). Tests: `a_bad_lineage_is_refused_before_the_executor_runs`. Only when the executor attaches the lineage. |
| G19 | Cycle detection in the tree view | **EXISTS** | `lineage::tree` attaches a child only to a parent exactly one level up (`types/lineage/tree.rs:60-64`), so no set of tasks makes a cycle. |
| G20 | Loop detection between agents (A to B to A) | **VERIFIED-ABSENT** | Pattern `cycle\|loop.detect\|visited` in `types/lineage.rs`, `client/delegation/*.rs`, `server/handler/lineage.rs`: 0. Lineage carries `root`, `depth`, `parentTaskId`, with no agent path (`types/lineage.rs:84-94`). A loop is bounded only by depth 16, and only if every hop carries the lineage. |
| G21 | Fan-out limits and backpressure | **PARTIAL** | Client: `HttpPoolBuilder::with_max_in_flight_per_origin`, `ClientBuilder::with_max_in_flight`, `RetryBudget`. Server: admission gate, `max_subscribers_per_task` 256, `max_subscribers` 10,000 (ADR 0044). No limit on children per task or per job (pattern `max_children\|max_fan_?out\|fanout_limit\|max_delegations`: 0). Depth 16 with unbounded width bounds nothing. |
| G22 | Fleet-level observability | **PARTIAL** | Spans: `a2a.lineage.root` and `a2a.lineage.depth` on the executor span (`server/rpc_span/agent/lineage.rs:17-18`); delegation calls run in the parent's trace (`client/delegation/origin.rs`; test `client/tests/delegation_spans.rs`). No delegation span and no client-side delegation metric (no `span!` in `client/delegation/*.rs`; pattern `a2a\.(swarm\|job\|delegation)\.`: 0). Server metrics: `a2a.server.{halts, halt.stopped_tasks, lease.events, claim.events, steering, refusals}` (`server/otel/`). No per-job metric by design (cardinality, ADR 0034 decision 4). The tree view is a pure function over tasks the caller must gather from every agent (`lineage::tree`). No budget metric. Balancer state is a snapshot, not an instrument (ADR 0050). |
| G23 | Testing tools for swarms | **PARTIAL** | Exists: `ScriptedPeer` (one hostile peer), executor conformance, halt-store conformance, `tests/swarm_scale` (2,385 lines, `wc -l`), the injection harness (multi-agent on loopback, private to the SDK's tests). Absent: an in-process transport and a deterministic simulator (pattern `InProcess\|in_process\|InMemoryTransport\|LocalTransport\|DirectTransport\|turmoil\|madsim\|loom\|FaultInjector\|chaos` in `crates bindings`: only the SLIMRPC example name). Every multi-agent test binds real loopback ports (`examples/swarm/src/topology.rs:18-40`). The fault injectors in `examples/resilient-agent/src/support/injectors.rs` are example code. `testing` and `conformance` are not reachable through the SDK crate (§1 row 24). |
| G24 | Fleet operations: drain | **EXISTS** | `begin_draining` / `is_draining` (`server/handler/introspection.rs:130-137`); `"draining"` in `/load` (ADR 0046). |
| G25 | Rolling upgrade with mixed extension versions | **PARTIAL** | Tolerant halt-row parsing fails closed; the root-halt rule is left to the operator (ADR 0034 amendment 8). `server/store/claim/upgrade.rs` and `scripts/live/upgrade_live.py` exist (not read). ROADMAP item 4: a rollback from 0.16 to 0.15 and 60 replicas at once are not qualified. A breaking extension change is a new URI (`book/src/reference/swarm-extensions.md:35`); no negotiation of versions between hops exists. |
| G26 | Hand-off on shutdown | **EXISTS, with stated holes** | ADR 0054; ROADMAP item 9 (a crashed replica, a handler without claims, a reattached delegation). |
| G27 | Human-in-the-loop approval across a swarm | **PARTIAL** | Per task per server (§1 row 18). No relay of `input-required` up a tree; no `Delegation` method to answer an interrupted child (the handle is consumed by `wait`; the caller starts a new delegation whose message names the task). |
| G28 | Security between members: mTLS identity | **EXISTS** | `auth::MutualTlsAuthInterceptor` (`tls-rustls`, ADR 0018); SPIFFE in the SLIMRPC binding (`bindings/a2a-protocol-slimrpc/tests/spiffe*.rs`). Not read in depth. |
| G29 | Token-exchange actor chains, per-hop authorization | **PARTIAL** | The exchange call exists (§1 row 17). Nothing connects it to `Delegation`: one `Arc<A2aClient>` per handle, so a client per (agent, principal) is the adopter's to build and cache. Lineage is explicitly not an authorization (ADR 0034 decision 4). |
| G30 | Prompt-injection containment | **EXISTS as a tested harness, not as a swarm policy** | `docs/injection-containment.md`: 17 scenarios, 5 "not contained" asserted. Controls are per agent (budget, approval, content keys, caller isolation, halt). |
| G31 | Cost attribution per tenant per swarm | **PARTIAL** | Meters exist per turn and are refunded up one hop (`ChildGrant::settle`). There is no roll-up across a tree: the report is on status-update metadata only, is not stored on the task (ADR 0023 "Reporting spend"), and has no metric. No per-root or per-tenant aggregate. |
| G32 | Multi-tenancy isolation in shared stores | **EXISTS, fail-closed** | A tenant-bearing request is refused unless the stores isolate (`docs/swarm-orchestration.md:83-88`). Lease and halt keys include the tenant (`server/handler/lease/watch.rs:47`; ADR 0034 amendment 7). The journal has `owner` and `tenant` columns, with the owner not enforced. |
| G33 | Multi-region | **VERIFIED-ABSENT** | Pattern `(?i)\bregion\b\|multi_region\|MultiRegion`: 0. ROADMAP item 4: "more than one host, so a network between replicas" is unqualified. Lease deadlines use the database clock on PostgreSQL (`server/store/lease/postgres.rs:9-12`), which helps; the in-memory store uses the process clock. |
| G34 | Mixed-language peers | **PARTIAL** | The wire contract, schemas and vectors exist; Python and Go shims run in interop. No middleware for other SDKs (ADR 0049 "What is not built"; ROADMAP item 14). A hop in another language must itself cascade cancels and stop on `HALTED`. |
| G35 | Progress roll-up from children to the parent's stream | **VERIFIED-ABSENT** | `Delegation::next_event` hands events to the caller; nothing forwards or rewrites them into the parent's queue. The adopter audit's S5 names it: "Forwarding a downstream stream into the upstream queue, rewriting ids ... and merging fan-out streams all have to be hand-written: 110 of the 230 lines" (`docs/adopter-audit-2026-09-22.md:1058`). `Delegation` closed the cancel part of S5 only. |
| G36 | One-step "full swarm mode" configuration | **VERIFIED-ABSENT** | See §5. |

---

## 4. Edge cases and correctness risks traced in code

Only paths I read end to end. "Test" names the covering test, or says none was found (by listing the test function names of the files named in §1).

**R1. A handed-back turn is re-sent with the original relative budget, so the child's deadline and meters restart.** VERIFIED.
- `Delegation::start_with` attaches the budget before the send (`client/delegation/options.rs:200`, `prepare` at `:379-395`), and `start` stores `params.message.clone()` (`client/delegation/mod.rs:198,218`). The stored message therefore carries `a2a-rust.com/budget` with the `remainingMs` computed at grant time.
- On a hand-off mark, `continuation()` clones that message, changes only the message id, task id, context id and idempotency key (`client/delegation/continuation.rs:133-144`), and sends it.
- The server starts a new account from the message at receipt: `BudgetHandle::start_at(&budget, received)` (`server/handler/budget.rs:38-48`); "A continuing message brings its own budget" (ADR 0023 "Scope").
- Effect: after each of up to three continuations (default `HandOffPolicy`) the child has the full original time again, measured from the new receipt, and full meters. The parent's `ChildGrant` is settled once, from the last turn's report (`mod.rs:324-328,468-470`), so what the handed-off turn spent is never charged. A child can outlive its parent's deadline by up to 3 x `remainingMs` plus the backoffs.
- Test: none. `client/src/delegation/continuation_tests.rs` has six tests, none mentions a budget; `client/tests/delegation_budget_tests.rs` has no hand-off case.

**R2. A halt refusal is treated as a retryable replica fault by this SDK's own client on the 503 path.** VERIFIED for the classification; INFERRED for which `ClientError` variant each binding produces.
- ADR 0049 decision 4 requires of a hop: "stop on a `HALTED` refusal: a child's refusal is not a fault to retry".
- The client has no reference to it: `grep -rn 'HALTED' crates/a2a-protocol-client/src` returns nothing.
- `ClientError::UnexpectedStatus { status: 503 }` is retryable (`client/retry.rs:205-217`), is "safe to retry" for a non-idempotent send (`retry.rs:328-340`), and counts as an origin failure toward ejection (`client/origin.rs:65-76`, `502..=504`).
- Effect, where a halt arrives as an HTTP 503: a client with a retry policy re-sends into the halt; a balanced or pooled client ejects every halted replica and then reports `CircuitOpen`. On JSON-RPC the halt is `-32603` with no retry delay, so `Protocol(e)` is not retryable and counts as success. The behaviour differs by binding.
- Test: none found for the client side (`server/tests/http_json_error_parity.rs` covers the server's bytes).

**R3. `Canceled` does not say who stopped the child.** VERIFIED.
- A lease lapse stops the turn with `halt::stop` (`server/handler/lease/watch.rs:159`), the same call a halt uses (`server/handler/halt.rs:451-454`). Both set `turn.halted`, and `finish_run` then writes `Canceled` through the cancel hook (`server/handler/messaging/terminal.rs:232-247`), as `CancelTask` does.
- The client maps the state to `Outcome::Canceled(status)`, "whoever asked for it" (`client/delegation/outcome.rs:26-27`).
- Effect: a parent cannot tell "my lease lapsed during my own pause, retry is reasonable" from "an operator halted the job, do not retry" from "a third party cancelled it". Any retry policy in a group layer needs this distinction.
- Test: `a_lapse_is_counted` (`server/handler/lease/tests.rs:462`) covers the metric only.

**R4. The journal has no ownership claim: two supervisors with one `owner` both reattach every child.** VERIFIED.
- `DelegationJournal::open(owner)` lists; no method claims or fences a record (`types/delegation_journal.rs:141-195`). ADR 0030: the owner column exists "but does not enforce".
- A reattached handle is armed (`client/delegation/resume.rs:83`). If one of two holders drops or gives up, it cancels the child the other is still following (`drop.rs:95-143`; `mod.rs:456-463`).
- This is the normal shape of a supervisor that is itself a replicated agent behind a load balancer.
- Test: none. ADR 0030 "What was and was not run": not run "under concurrent replicas".

**R5. With `ClientBuilder::new(url)` a delegation has no idempotency key, so an ambiguous start cannot be recovered.** VERIFIED.
- `peer_honours_idempotency: false` in `ClientBuilder::new` (`client/builder/mod.rs:194`); set from the card only in `from_card` (`:298-307`) or by `with_peer_honouring_idempotency` (`:960`).
- Every delegating example and live binary builds its client with `ClientBuilder::new` (`examples/swarm/src/topology.rs:45`; `scripts/live/delegation/src/bin/supervisor.rs:117`).
- `start_with` leaves the journal record open when the start failed without a refusal (`client/delegation/options.rs:204-215`): an open record with no task id and no key. The book's recovery for it is "send the same message again under the record's idempotency key" (`client/delegation/journal.rs:137-141`), which does not exist here, and a resend without a key starts a second child. The journal also stores no message text, so the supervisor must have kept the message itself.
- Bound on the damage: with a lease, an unrenewed child lapses within its ttl (the renewal loop never learns the id, `renewal.rs:60`). Without a lease it runs to the executor timeout.
- Tests: `an_unkeyed_message_to_an_agent_not_known_to_honour_keys_is_recorded_without_one` and `a_send_that_never_reaches_the_agent_leaves_its_record_open` (`client/tests/delegation_journal/records.rs:18,88`) pin the state; nothing tests a recovery from it, and the live proof skips it (`supervisor.rs:229-233`).

**R6. Lease lapse against completion: traced, no defect.** VERIFIED.
- The checker fires the token and marks the turn halted (`watch.rs:150-160`). `finish_run` writes `Canceled` only when no terminal state was written (`terminal.rs:232`), so an executor that completed first stays `Completed`. The checker re-checks that the watched turn is still the same `Arc` before acting (`watch.rs:143-147`).
- Tests: `an_answer_does_not_reach_the_turn_that_took_the_key_since`, `a_lease_the_store_dropped_is_not_released_at_the_turns_end` (`server/handler/lease/watch_tests.rs:186,203`).

**R7. Halt against a send being admitted: traced, no defect.** VERIFIED.
- The flag is set before the token walk (`halt.rs:213-229`); admission checks before and after registering the token (`halt.rs:33-38`, `stop_if_halted` at `:402-415`).
- Tests: `a_job_halted_between_admission_and_registration_is_stopped_by_its_own_check` (`server/handler/halt/window_tests.rs:107`); `a_poll_read_before_a_resume_does_not_halt_again` (`store_tests.rs:293`).

**R8. A parked child of a dead parent is not stopped by its lease.** VERIFIED.
- When a turn ends parked, its watch entry is removed and the lease is kept (`watch.rs:91-104`); nothing checks the deadline of a task with no running turn.
- The task stays `input-required` until ADR 0056's opt-in expiry, or forever. Root halts skip parked tasks too (`halt.rs:26-27`).
- Test: `a_terminal_turn_releases_its_lease_and_a_parked_one_keeps_it` (`server/handler/lease/tests.rs:320`) pins the keeping; nothing covers the orphan.

**R9. A drop-time cancel is a detached task.** INFERRED.
- `Drop` spawns the cancel on the current runtime (`client/delegation/drop.rs:133`). A process that exits, or a runtime that shuts down, right after dropping handles may never send it. The live supervisor adds `LINGER_SECS` for this (`scripts/live/delegation/src/bin/supervisor.rs:360-363`).
- Tests: `dropping_an_unsettled_handle_cancels_the_child`, `a_handle_dropped_outside_a_runtime_cancels_nothing` (`client/tests/delegation_tests.rs:281,403`). Neither covers runtime shutdown.

**R10. Steering to a dead replica.** ADR-ONLY plus test names.
- `handed-off` can mean "stored for a replica that has died", for up to the claim's ttl (ADR 0036 "Not built"; ROADMAP item 8).
- Tests: `with_the_claim_lapsed_the_message_is_refused_not_lost`, `a_halted_replica_hands_nothing_over` (`server/handler/messaging/steer_replica_tests.rs:211,293`). The dead-replica window itself has none.

**R11. A re-run turn re-runs its side effects under new keys.** INFERRED from two verified facts.
- The continuation gets a new message id (`client/delegation/continuation.rs:135`); tool-call keys are scoped to (task id, message id) (`server/tool_loop/executor.rs:137`).
- An executor that opts in to hand-off and derives keys the same way gives a backend no way to recognise the repeat. `ToolLoopExecutor` does not opt in today, so this is latent. ADR 0054 leaves it to the executor ("only it knows whether doing so is safe").

**R12. `steer_all` partial failure: traced, as designed.** VERIFIED.
- One spawned send per child, awaited in input order; a failure or panic of one is reported per child (`client/delegation/steer.rs:191-220`).
- Test: `steer_all_reports_each_child_in_order` (`client/tests/delegation_steering_tests.rs:360`).

**Not traced** (so not reported as findings): the admission gate's fairness arithmetic, the push outbox, claim takeover ordering, the SLIMRPC multicast internals.

---

## 5. API coherence

**Verdict: scattered.** VERIFIED from the module trees and the prelude.

- **Three crates, plus a fourth that is unpublished.** Wire types in `a2a_protocol_types::{lineage, lease, budget, delegation_journal, steering, failure, approval, idempotency}`; the handle in `a2a_protocol_client::delegation`; fleet client pieces in `a2a_protocol_client::{balance, discovery, retry}` and `HttpPool`; server pieces across `a2a_protocol_server::{admission, approval, steering, rate_limit, store, handler::{halt, lease, load}}`; SQL journals for a client-side concept live in the server crate (ADR 0030 says why); discovery in `a2a-protocol-discovery` (`publish = false`).
- **No prelude entry.** `a2a_protocol_sdk::prelude` (`crates/a2a-protocol-sdk/src/lib.rs:100-154`) exports no `Delegation`, `DelegationOptions`, `Outcome`, `Lineage`, `Budget`, `BudgetHandle`, `FailureClass`, `HaltScope`, `Admission` or `CardCache`. The adopter audit's K2 raised the same point for `FailureClass` (`docs/adopter-audit-2026-09-22.md:1123`).
- **No single builder.** A delegation is configured in `DelegationOptions`; the receiving side in about ten `RequestHandlerBuilder` calls; the operator controls are methods on `RequestHandler` with no route.
- **No single book part.** The material is in `book/src/client/delegation.md` (446 lines), `book/src/building-agents/steering.md`, `book/src/client/load-balancers.md`, `book/src/deployment/horizontal-scaling/*.md`, `book/src/deployment/oversight.md`, `book/src/reference/swarm-extensions.md` (358 lines), `book/src/examples/swarm.md`. There is no "running a swarm" chapter that orders them.
- **Two unrelated fan-out surfaces**: `delegation::steer_all` and `SlimRpcMulticast`.
- **Automatic context does not flow.** Trace context is propagated by default (`client/builder/mod.rs:355-362`, "default: yes"). Lineage, budget and the principal are not: the executor must call `ctx.child_lineage()`, `ctx.budget().grant()`, and build a per-principal client.

### What an adopter must do today for "full swarm mode" (from the code)

Cargo features on `a2a-protocol-sdk`:
- `postgres` (shared task, claim, halt, lease, journal and rate-limit stores), or `sqlite` for one host (no SQLite lease store).
- `auth-jwt` for verified callers and actor chains; `signing` for content keys.
- `tool-loop` (and `skills`) if the agents are model loops.
- `grpc` / `websocket` as wanted. Note that balancing is HTTP-only and that gRPC and WebSocket streams carry no event ids for resume.
- Nothing for delegation, lease, budget, lineage, halt, steering, admission or load: they are always compiled.
- Add `a2a-protocol-client` with `testing` and `a2a-protocol-server` with `conformance` as direct dev-dependencies; the SDK forwards neither.
- `a2a-protocol-discovery` as a path or git dependency.

Server builder calls (`server/builder.rs` line numbers):
`with_task_store` (318), `with_push_config_store` (362), `with_turn_claims` (624) or the store's own claims, `with_replica_name` (670), `with_halt_store` (558), `with_halt_poll_interval` (568), `with_lease_store` (585), `with_max_lease_ttl` (681), `with_steering(SteeringConfig)` (`builder/steering.rs:39`), `with_admission(Admission)` (529), `with_load_reporting()` (540), `with_handler_limits(HandlerLimits ... with_saturation_readiness)` (841), `with_approval_gate` (510), `with_tenant_resolver` (894) or `with_authorized_tenant_resolver`, `with_interceptor(JwtAuthInterceptor)`, `with_interceptor(RateLimitInterceptor ... with_shared_counter)` (688), `with_push_outbox` (442), `with_profile` (468). The agent card must come from this builder so the extensions are declared.

Executor code, per delegating turn: read `ctx.cancellation_token`; `ctx.child_lineage()?`; `ctx.budget().map(|b| b.grant(&request))`; build `DelegationOptions`; `Delegation::start_with`; `wait_or_hand_off(token.cancelled(), shutdown)`; map `Outcome`; fetch artifacts with a second `get_task`; override `resumes_after_hand_off` if safe.

Client calls: `ClientBuilder::from_card(&card)` (or `with_peer_honouring_idempotency(true)`), `with_retry_policy(RetryPolicy ... with_budget)`, `with_pool(&HttpPool::builder().with_ejection(..).with_max_in_flight_per_origin(..).with_retry_budget(..).build()?)`, `with_balancing(DnsEndpoints::new(url)?, BalancePolicy::new().with_hedge(..))`, a `CardCache` shared by all clients, and a token exchange per principal.

Operator code the adopter writes: an authenticated admin route that calls `handler.halt(HaltScope::root(tenant, root), by, reason)` and `resume`; the recovery loop over `DelegationJournal::open`; `purge_settled` on a schedule; gathering tasks from every agent for `lineage::tree`.

That is roughly 15 builder calls, 6 executor steps, 5 client calls and 4 pieces of operator code, with no type that checks they agree (for example, a lease granted to a server built without a lease store is silently ignored: `server/handler/lease.rs:62-64`).

---

## 6. Proposed direction for a first-class swarm layer

### 6.1 The constraint to respect

The recorded decision is "no registries, schedulers, orchestration DSLs or mesh" and "`examples/swarm` is where an orchestrator lives" (`docs/swarm-orchestration.md:290-296`), restated in ADR 0029 and in ADR 0033 "What stays out". ADR 0033 is the one piece of group machinery the maintainer asked to have designed. The proposal below stays inside that line: it is a client-side layer over existing methods, with no wire change, that owns handles and policies and chooses nothing.

### 6.2 Three layers, each a thin wrapper of what exists

**Layer A: `TurnScope` (name open). Make one child correct by default.**
A value built from the executor's `RequestContext` that carries what today is passed by hand, and a `delegate` method on it.

- Wraps: `RequestContext::{cancellation_token, child_lineage, budget}` (`server/request_context.rs:65,220,253`), `DelegationOptions`, `Delegation::start_with`, `wait_or_hand_off`.
- Does: attach the child lineage always; grant from the turn's budget when there is one; pass the turn's token as `cancel`; take an optional shutdown future as `hand_off`; return a result that includes the child's final `Task` or artifacts (closing the second `get_task` in §2.6).
- Placement problem: the client crate cannot see `RequestContext` and the server crate does not depend on the client (ADR 0030 "Why the SQL lives in the server crate"). The only crate that sees both is `a2a-protocol-sdk`, which today has one file. Alternative: a small trait in the types crate that `RequestContext` implements (`BudgetHandle` already lives in types for this reason, ADR 0023).
- This layer alone would have made the three non-cascading examples correct and removes concern 6 of §2.7.

**Layer B: `DelegationGroup`, as ADR 0033 specifies. Make N children correct.**
- Wraps: `Delegation` (unchanged), `BudgetHandle::grant` and `ChildGrant` for the split, `steer_all`, `FailureClass::is_retryable` for the "which members may be retried" helper.
- Does: required in-flight cap; `Join::{All, FirstOk, Quorum(k), FailFast}`; budget share taken at member start; one cancel, also on drop of the `run` future; results in input order; staggered lease renewals.
- Removes `examples/swarm/src/supervisor.rs:46-150,171-203` and concerns 1 to 4 of §2.7.
- Additions I would make to ADR 0033 from this audit: (a) members carry a target that may be a balanced client, so "retry on another replica" is the existing balancer and not group logic; (b) the member result carries artifacts; (c) R3 must be fixed first, because `FailFast` and the retry helper need to tell a lease lapse from a halt.

**Layer C: `Supervisor` recovery driver. Make a restart correct.**
- Wraps: `DelegationJournal::open`, `Delegation::reattach_with`, `DelegationOptions::{with_journal_record, with_lease}`, `wait_or_hand_off`.
- Does exactly what `book/src/client/delegation.md:196-237` tells the reader to write: reattach every open record before waiting on any; never drop held handles on one failure; renew leases at once; settle or report records with no task id.
- Needs from the journal: the lease ttl and the agent URL per record (the URL is there; the ttl is not), a group id (ADR 0033 "Open"), and an ownership claim with a fence (R4).
- This is "a recovery driver", which ADR 0030 declined as "eight lines". The live proof then found three defects in those lines (ADR 0030 amendment 2026-11-04). That is the argument for reopening the decision.

**Not proposed**: a registry, a scheduler, a DAG or workflow DSL, a blackboard, work queues, or a `Fleet` type that owns agents. Placement stays `EndpointSource` plus `BalancePolicy`; the one extension worth weighing is a load-aware policy that reads `endpoint-load-metrics` (G9), which is a client reading a header, not a scheduler.

### 6.3 Prerequisite fixes, in dependency order

1. R1: do not re-send a stale budget on a hand-off continuation (recompute from the live `ChildGrant`, or strip it and say so).
2. R3: give a lapse and a halt a machine-readable cause on the `Canceled` status, in the style of `FailureClass`.
3. R2: teach the client `HALTED` (not retryable, neutral for ejection), as ADR 0049 asks of other languages.
4. R5: decide the default for idempotency keys when the peer is this SDK's server with a store that honours them.
5. R4: a claim on journal records before any durable group.
6. Coherence: a `swarm` (or `delegation`) section in the prelude, the `testing` and `conformance` features forwarded by the SDK, and one book part that orders the existing chapters.

### 6.4 Open design questions for the maintainer

1. Does a group or a recovery driver cross the "not a runtime" line? ADR 0033 says a group does not. Layer C reverses a recorded "not built".
2. Where does the layer live: `a2a_protocol_client::delegation` (cannot see `RequestContext`), `a2a-protocol-sdk` (becomes more than a facade), or a sixth crate (a cost ADR 0030 already weighed against)?
3. `Join` as an enum or a closure (ADR 0033 "Open"); what `Quorum(k)` reports when k is no longer reachable.
4. Does the group retry? ADR 0033 says no (a helper selects the retryable members). At scale every adopter will write the same loop around it.
5. Durable groups: a `group` column on `a2a_delegations` or a separate group record (ADR 0033 "Open"). Both change the journal contract and the PostgreSQL schema.
6. Should lineage be attached automatically by any delegation made inside a turn? It is always honoured on the server but never sent unless asked.
7. Should a child idempotency key be derived from (parent task, parent message, member index) so a re-run parent reaches the same children (G16)? ADR 0026 chose random keys on purpose, because a derived key is computable by anyone who sees its inputs; a keyed HMAC as the tool loop uses would answer that.
8. A halt endpoint: stay a Rust method (ADR 0049's reason is that a wire halt is something any caller could send), or ship an opt-in authenticated admin route so every adopter does not write one?
9. Loop detection: carry an agent path in lineage (leaks agent identities to every hop, the concern ADR 0034 raised about locations), or keep depth 16 as the only bound?
10. Is `SlimRpcMulticast` a member transport of the group, or does it stay a separate surface?
11. A fan-out width limit per task or per job (G21): server-enforced or client-side only?

### 6.5 Over-engineering risks

- **Building Layer B before fixing R1 to R5.** A group multiplies each of those defects by N.
- **A supervisor with restart strategies in the Erlang sense.** There are no child processes here, only remote tasks; "restart" means "send again", which is a retry policy plus idempotency. One-for-all has no meaning across independent agents. A strategy enum would be machinery with no caller.
- **A generic result reducer or map-reduce.** Returning the artifacts is enough; reduction is the application's.
- **A registry or scheduler added "because scale".** The code has no measurement above 256 workers, two hops and one host (ADR 0033 Context; ROADMAP items 4 and 5). Designing placement for thousands of agents before that measurement repeats what `docs/swarm-scale-findings.md` warns about.
- **A second budget or deadline mechanism for groups.** `BudgetHandle::grant` already reserves atomically; the group only needs the share arithmetic.
- **A new extension.** Everything in 6.2 is client-side. Only R3 needs a wire addition, and it can ride the existing failure-class metadata.
- **Durable groups in the first cut.** ADR 0033 already defers them; the journal schema change is the expensive part.

---

## 7. Where the design records and the code disagree

| # | Record | What it says | What the code or a later record says |
|---|---|---|---|
| D1 | `docs/swarm-orchestration.md:70-71` | "A SQLite journal is not built." | `SqliteDelegationJournal` exists: `server/store/delegation_journal/sqlite.rs` (179 lines), exported from `server/store/mod.rs`, documented at `book/src/client/delegation.md:233-235`. |
| D2 | `docs/swarm-orchestration.md:34,36,40` (table "What is measured today") | R1 "Missing in the protocol and the SDK", R3 "the orchestrator side does not", R7 "Absent ... There is no budget field or grant." | The same file records G1-A done (`:56,304`), G2 built (`:63-71`) and G5 done (`:317-321`). The table and the heading `:44` were never updated. |
| D3 | `docs/swarm-scale-findings.md:601-627` (Finding 8) | The single-writer refusal does not cross replicas; test `the_single_writer_refusal_does_not_cross_replicas` "asserts today's behaviour". | That test no longer exists. `crates/a2a-protocol-server/tests/multi_replica.rs:579` is `the_single_writer_refusal_crosses_replicas` (turn claims). The finding has no "fixed" note, unlike Findings 2, 6 and 7. |
| D4 | ADR 0050 "Not built" | "Load from the server (`A2A-Load` ...): no server sends it yet". | ADR 0046, accepted in the same release, sends `endpoint-load-metrics` (`server/handler/load.rs:22`). The client still does not read it, so the gap is real but its stated reason is wrong. |
| D5 | ADR 0029 Decision | "`mdns-sd` 0.21.6, pinned exactly". | `crates/a2a-protocol-discovery/Cargo.toml:28` pins `=0.21.7`. |
| D6 | ADR 0049 decision 4 | A hop must "stop on a `HALTED` refusal: a child's refusal is not a fault to retry". | This SDK's client has no `HALTED` handling and classifies a 503 as retryable and as an ejection failure (R2). |
| D7 | ADR 0049 amendment 2026-11-05 | "A schema and vectors for every extension this SDK defines on the wire". | There is none for steering (`crates/a2a-protocol-types/extensions/` has no `steering-v1`); steering has only `tests/golden/uri_contracts/steering.json` (ADR 0036 "Built"). |
| D8 | ADR 0020 Consequences | "G5 (deadline and budget propagation) is not built." | Built by ADR 0023. Superseded text, not flagged in ADR 0020. |
| D9 | ADR 0030 Decision 4 and its Status line | Decision 4 names both SQL journals; the status paragraph and the "What was and was not run" section speak only of PostgreSQL. | Both exist (D1). |
| D10 | `examples/README` / book example pages (INFERRED from the code, not from reading those pages) | The examples are presented as multi-agent patterns. | Three of them delegate without `Delegation`, so they demonstrate the gap that `docs/swarm-orchestration.md` G1 measured. |

Roadmap and handoff items that bear on swarms, for cross-reference: `ROADMAP.md:63-101` items 4, 5, 7, 8, 9, 10, 11, 12 (with S3), 14; `docs/handoff.md:654-703` (cross-replica streaming still polls every 250 ms; a cancel reaches another replica at the owner's next renewal unless a halt store pushes it; a store outage longer than the retry can leave a task unfinished).

---

## 8. What this audit did not do

- No build, no test run, no measurement. Every number quoted is from a design record and is cited as such.
- Not read: `client/balance/*` bodies, `client/pool/*` bodies, the admission gate's body, the push outbox, the claim stores' SQL, the SLIMRPC multicast body, `examples/resilient-agent`, `scripts/live/*.py`, the tool loop beyond its executor scope line, ADR 0022, 0035, 0037 and 0056 beyond their status lines and headings, `docs/tool-calling-study.md` beyond its headings.
- §1's "Tests" column lists files by name; I read the test function names of the delegation, lease, halt, steering and budget files, not the bodies.
- R2's mapping from a REST 503 to `ClientError::UnexpectedStatus` was not traced through `client/transport/rest`.
