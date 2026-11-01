# Triage slice h-s0: audit rows H2, H3, H4 (S0)

Baseline: `4a8243c66df5ae07a54a69168ab419ac609b06e9` (checked with `git rev-parse HEAD`).
Everything here comes from reading the code and tests. Nothing was built or run, and `cargo` was not used.

## Counts by verdict

| Verdict | Rows |
|---|---|
| partly_fixed | 3 (H2, H3, H4) |
| confirmed, already_fixed, not_a_defect, needs_decision, cannot_check, duplicate | 0 |

New findings: 9. One is high (NEW-h-s0-1), four are medium (NEW-2, 3, 7, 8) and four are low.

## The rows

**H2: partly_fixed, now S1.**
- **Private CA and mTLS.** These now work end to end on:
  - JSON-RPC and HTTP+JSON, through `ClientBuilder::with_tls_config` (commit 57f034e).
  - WebSocket, through `WebSocketTransportConfig::with_tls_config`.
  - gRPC, through `with_grpc_tls_config`, which takes a tonic type and replaces the Mozilla roots.
  - SLIMRPC, through `slim_config`.
  - The card resolver and the client JWKS fetch, through `CardFetchOptions::with_tls_config` (commit c4404e8).
  - Push delivery and server-side OIDC/JWKS.
  - Delegation, which reuses its `Arc<A2aClient>` and so inherits that client's TLS.
- **Still missing on TLS:**
  - The token provider has no TLS option: environment roots only, and no client certificate.
  - gRPC ignores `SSL_CERT_FILE`.
  - The standalone `JsonRpcTransport::new` and `RestTransport::new` take no TLS configuration.
  - One TLS identity has to be configured up to four times.
- **Pooling.** Every `build()` still creates its own hyper pool. There is no per-host cap and no h2c, and the card fetch builds a new pool on every fetch. Nothing public lets several `A2aClient`s share a pool. ADR 0032 (`HttpPool`) is proposed and not built.
  - Two workarounds exist: share one `Arc<A2aClient>` per peer, or clone one `JsonRpcTransport` into `with_custom_transport` (default TLS only).
- **Refuted sub-claim.** The report says each build gets a new root store and TLS session cache. It does not: `default_tls_config` is now a process-wide static, and clones share the rustls session store.

**H3: partly_fixed, now medium.** The reaper (commit 57f034e) ends a task that a dead replica left `submitted` or `working`. It marks it `failed` with class `transient`, sends the webhook, writes the log entry, and (since commit e313835) releases the lease. This runs on the bundled SQLite and PostgreSQL stores, plain and tenant-aware, with turn claims, which is the default.
- **What the parent sees:** `Outcome::Failed{Transient}` about 15 to 30 s after the death, through the resumed stream on another replica. Its renewal loop then stops.
- **In-memory stores:** no claims and no reaper, but no orphan can survive a restart either.
- **No reaper at all:** with `without_turn_claims()`, with a custom claim store, or with `PostgresTurnClaims::new`. In those setups the task stays `working` forever and its lease keeps renewing.
- **Renewal is never refused** for a task with no live claim. The H-report's `TaskNotRunning` design was not built.
- **The reaper starts only on the first request** a replica serves.

**H4: partly_fixed, now high.**
- **Closed:**
  - Resume before cancel, for SSE bindings (commit 660196e). The test proves it: `delegation_resume/resume.rs:13` would fail if this were reverted.
  - `reattach`.
  - The `DelegationJournal` trait, an in-memory journal, and `PostgresDelegationJournal` (commit 49f5509).
- **Open:**
  - Nothing in the SDK calls `reattach`. Recovery is a hand-written loop in the book.
  - The journal has no owner claim or adoption, so a Kubernetes pod that comes back under a new name finds nothing.
  - The journal holds no message text.
  - There is no SQLite journal, and the types-crate docs claim there is one.
  - Lease and budget are not restored on reattach, and `start_journaled` cannot grant a lease.
  - On gRPC and WebSocket, streams carry no event ids, so the resume streak never resets. With no keep-alive on those streams and a 5-minute idle bound, a long quiet child is cancelled after about 6 idle cuts.
  - A graceful redeploy of a supervisor agent still cancels its whole subtree.
  - A terminal child cannot be resubscribed to, so its tail events are not replayed (the outcome is still right, via `GetTask`).

## Most important findings outside the rows

1. **NEW-h-s0-1 (high).** The reaper, combined with no local self-fence and a 30 s pool acquire against a 15 s claim TTL (E-F7), declares a healthy replica dead. This happens when its claim renewals stall or it is partitioned from the database. Its tasks are marked `failed` while their executors keep running, so parents resend and the work runs twice.
2. **NEW-h-s0-2 (medium).** Shutdown fires every executor token on purpose. With the documented `wait(ctx.cancellation_token)` pattern, that cascades `CancelTask` to every child and settles each journal record `canceled`. The journal and reattach therefore only help after a hard kill.
3. **NEW-h-s0-3 (medium).** Journal writes are awaited inline on each delegation's event path. A journal database outage stalls every delegation's stream.
4. **NEW-h-s0-7 (medium).** gRPC alone ignores `SSL_CERT_FILE`.
5. **NEW-h-s0-8 (medium, conjecture).** On tenant-aware PostgreSQL, no index serves the reaper's query, so it probably scans the whole `tenant_tasks` table every 15 s on every replica.

## Not done, and why

- **No live probes.** Each row's `probe` field gives the exact steps: a supervisor kill with a PostgreSQL journal, gRPC idle cuts behind an L7 balancer, a toxiproxy partition for NEW-h-s0-1, and `EXPLAIN` for NEW-h-s0-8.
- **SLIM's TLS internals** were not audited beyond the binding's tests.
- **Proxy tunnelling.** I did not confirm that tonic layers the configured TLS on top of `connect_with_connector` through the proxy tunnel.
- **Tests.** I read the assertions in the cited tests and judged whether each would fail without its fix. None was executed, including the PostgreSQL ones, which CI runs through `scripts/run_postgres_suites.py`.
