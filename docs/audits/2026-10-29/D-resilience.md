# D: Resilience and robustness audit, a2a-rust v0.15.0

Scope: client, server, push, stores and cancellation safety. The method was reading only: no cargo build, test or check was run. Every finding quotes code that was opened. Each one is marked **verified-by-reading**, **partly verified** or **conjecture**. Items that overlap `docs/handoff.md` "Open engineering" are marked **KNOWN**.

Paths below are relative to `/home/user/a2a-rust/crates/`.

Severity: **High** means a real deployment hits it under ordinary conditions. **Medium** needs a specific but plausible condition. **Low** is a hardening gap or edge case. **Info** records something checked and found sound.

---

## Summary

| ID | Sev | Title |
|---|---|---|
| D-01 | High | The default 30 s request timeout cuts off a blocking `SendMessage` to a slow (LLM) agent, and the task is orphaned (the gRPC deadline also reaches the server) |
| D-02 | High | The streaming path delivers push notifications inline in the per-task processor, so one hung webhook stalls persistence by up to 30 s per event and then holds the executor back |
| D-03 | Medium | After an executor panic the task is stored `Failed`, but no push is sent and a `SendStreamingMessage` stream ends with no terminal event |
| D-04 | Medium | A failed `PushConfigStore::list` drops the push silently, with no log and no metric (both paths) |
| D-05 | Medium | Push delivery is not durable: no outbox, a restart loses it, there is no delivery id for de-duplication, and a dead or `410` webhook is never disabled |
| D-06 | Medium | Blocking-path push jobs are spawned per request with no global bound and no per-task ordering, so events can arrive out of order across turns |
| D-07 | Medium | The server's WebSocket read loop awaits the sink lock and `send` without a timeout, so one stalled stream write pins the connection, defeats `idle_timeout` and ignores `closing` |
| D-08 | Medium | The client WebSocket transport sends no ping and sets no TCP keepalive, and a timeout does not mark the connection dead, so a half-open socket fails every call for about 15 minutes |
| D-09 | Low | WebSocket client connect timeout and TLS or handshake errors map to the non-retryable `Transport`, and the handshake's `Retry-After` is ignored |
| D-10 | Low | `Retry-After` is clamped to `max_backoff` (30 s) and applied without jitter, and the HTTP-date form is ignored |
| D-11 | Low | Retries are off by default, a stream cut mid-way is never resumed automatically, and `TaskFollow` does not resume |
| D-12 | Low | The client pool keeps idle connections 90 s against a server idle timeout of 75 s (a stale-connection race), with no connection max-lifetime (DNS changes) and no per-host cap |
| D-13 | Low | SSRF: `https://` webhooks are not pinned (rebinding gives a blind connect plus ClientHello to an internal IP), and several reserved ranges are missing |
| D-14 | Medium | The SQLite migration runner never issues `ROLLBACK` on error or cancellation after `BEGIN EXCLUSIVE`, so a pooled connection keeps the database exclusively locked |
| D-15 | Medium | A PostgreSQL failover to a demoted primary (`25006` read-only) is classed as a permanent internal error, and the poisoned connections stay pooled for up to 30 minutes |
| D-16 | Low | A store outage on a continuation's event-log read becomes a non-retryable `Internal` error, not `Unavailable`/503 |
| D-17 | Low | The PostgreSQL pool size is fixed at 10 per store per replica, so replicas × stores × 10 exhausts PostgreSQL `max_connections` |
| D-18 | Low | In-memory capacity eviction removes running or `input-required` tasks when the oldest 1024 are not terminal |
| D-19 | Low | `AuditLog.locks` (and the in-memory audit chains) grow by one entry per distinct tenant and are never evicted (conjecture on reachability) |
| D-20 | Low | gRPC dispatcher defaults: no keepalive, no connection cap, no handshake or idle bound. The WebSocket `idle_timeout` defaults to `None` |
| D-21 | Medium | Graceful shutdown cancels every in-flight task after 5 s, so a rolling deploy makes each long task terminally `Canceled` instead of handing it to a peer replica |
| D-22 | Low | Push DNS lookups run `getaddrinfo` on the blocking pool, and the 5 s timeout does not free the thread |
| D-23 | Low | `CleanupGuard::drop` and `DeferredExecutor::drop` call `tokio::spawn` from `Drop`, which panics when dropped outside a runtime context |
| D-24 | Info | Panics on request paths: the scan found none reachable from untrusted input (list included) |
| D-25 | Info | Checked and sound: server SSE slow-consumer handling, blocking-send client disconnect, body, frame and header limits, client SSE parser bounds, PostgreSQL migration locking, SQLite WAL and busy settings |
| K-1 | KNOWN | An outage longer than `store_write_retry` loses the event being written, and each queued event then waits the full retry again |
| K-2 | KNOWN | Cross-replica streaming polls the event log every 250 ms; a cancel reaches the other replica at the next 1 s renewal |
| K-3 | KNOWN | The executor's `Failed` write can be dropped when its retry gives up (`write_failure_event` logs and moves on) |

---

## Findings

### D-01 · High · The default timeout cuts off a blocking `SendMessage` to a slow agent, and the task is orphaned

**Where**
- `a2a-protocol-client/src/config.rs:298`, `:322`: `request_timeout: Duration::from_secs(30)`. The field doc says only "Per-request timeout for non-streaming calls. Defaults to 30 seconds."
- `a2a-protocol-client/src/transport/jsonrpc.rs:244`: `let deadline = tokio::time::Instant::now() + self.inner.request_timeout;` is applied to every method, `SendMessage` included. The REST transport is the same (`transport/rest/request.rs`).
- `a2a-protocol-client/src/transport/grpc.rs:436-440`:
  ```rust
  if with_deadline {
      req.set_timeout(self.inner.config.timeout);   // GrpcTransportConfig::timeout = 30 s
  }
  ```
  This sends `grpc-timeout: 30S` to the server. A Go, Java or Python server cancels the handler at the deadline.
- `a2a-protocol-client/src/methods/send_message.rs`: `send_message` applies `return_immediately` only when `ClientConfig::return_immediately` is set (default `false`). Nothing lengthens the bound for a blocking call.
- `a2a-protocol-client/src/retry.rs:283-290`: a `Timeout` is retried for `SendMessage` only when an idempotency key is present *and* `peer_honours_idempotency` is set. Neither is the default.

**Failure scenario.** An agent built on a hosted LLM takes 45–120 s per turn (one model call plus a tool round trip is enough). With default settings, `client.send_message(...)` returns `ClientError::Timeout("request timed out")` at 30 s. The server keeps running the task, spending tokens. The client never learned the task id: a blocking send returns the `Task` only at the end. Unless the caller supplied its own `contextId`, it has no handle with which to `GetTask`, `SubscribeToTask` or `CancelTask`. Over gRPC to a non-Rust server, the propagated deadline makes the *server* cancel the call at 30 s. The From-impl table (`error/mod.rs`) classes the timeout as `Transient`, so a delegating executor reports "transient" and an orchestrator may resend, starting a *second* task.

**Suggested fix.** Choose one or more:
- Give `SendMessage` its own default bound (minutes, or none when not `return_immediately`), separate from `request_timeout`.
- Or default to `return_immediately = true` plus `follow_task` for agents that declare streaming or push.
- Always attach an idempotency key to `SendMessage`, so a timed-out send can be retried or looked up.
- Put the task id in the timeout error when it is known (streaming), and document the pattern.
- Do not send `grpc-timeout` on `SendMessage`.

**Status:** verified-by-reading.

---

### D-02 · High · A hung webhook stalls the streaming persistence pipeline (push runs inline per event)

**Where**
- `a2a-protocol-server/src/handler/event_processing/background/state_machine.rs:217`, `:320`, `:370`, `:418`, `:654`: after each status, artifact or message event is persisted, the processor awaits push delivery inline:
  ```rust
  deliver_push_bg(task_id, stream_resp, push_config_store, push_sender, limits, metrics).await;
  Outcome::Persisted
  ```
- `.../background/push_delivery/mod.rs:74-100`: every event does `push_config_store.list(...)` (a database round trip on the SQL stores) and then `deliver_to_every_config(...)`, bounded by `push_delivery_timeout` (default 30 s, `handler/limits/mod.rs:292`).
- `.../background/mod.rs:166-200`: one processor loop per task handles events strictly in sequence: `Some(event) => run.handle(...).await`.
- `streaming/event_queue/mod.rs:283`: `mpsc::channel(capacity.saturating_mul(16).max(1024))` is the persistence channel. Once it is full, the executor's `write` waits, by design ("a slow collector hold[s] the executor back", `handler/messaging/respond.rs:210`).
- `push/sender.rs`: at the defaults a blackholed webhook uses the whole 30 s (3 × 10 s attempts + 1 s + 2 s backoff + 5 s DNS = 38 s, truncated to 30 s).

**Failure scenario.** A client registers a push config whose URL is blackholed: a firewall drops SYNs, the receiver is down, or the URL has a typo on a routable IP. The agent streams 300 artifact chunks (token streaming). Each chunk is persisted, then waits up to 30 s on the webhook before the *next* chunk is persisted. The results:
- `GetTask` and the event log lag by hours (300 × 30 s = 2.5 h).
- Subscribers on another replica, which read the log, see nothing.
- After 1024 buffered events the executor itself blocks.
- The terminal state reaches the store only after every earlier event's push has timed out.
- Cross-replica cancel through the store is delayed the same way.
- `executor_timeout` (1 h) can then fire on an executor that was merely back-pressured.

The blocking path was moved off the request path in an earlier release (CHANGELOG "Non-streaming `SendMessage` delivered webhooks on the request path"), but the streaming background path still awaits inline. Nothing in the book's "Delivery and retries" page says so.

**Suggested fix.** Decouple push from persistence. After persisting, hand the event to a per-task push worker: a bounded queue that preserves order and drops or coalesces oldest-first with a metric. Cache the config list per task, invalidated on config writes. Add a per-URL circuit breaker so a webhook that failed its last N deliveries is skipped quickly.

**Status:** verified-by-reading. The stall arithmetic is derived, not measured.

---

### D-03 · Medium · An executor panic gives no `Failed` push and no terminal SSE frame

**Where**
- `.../background/processor.rs:101-131` (`executor_panicked`): sets `TaskState::Failed` and `task_store.save(task)`. It makes **no** `deliver_push_bg` call and writes nothing to the event log or the queue.
- `.../sync_collector.rs:489-509` (`on_executor_finished`): the same; it only saves.
- `handler/messaging/execute.rs:62-82` (`CleanupGuard::drop`): on panic it only destroys the queue and removes the token, so readers see `Closed`.
- `handler/messaging/respond.rs:178-182`: a `SendStreamingMessage` reader gets `with_catch_up` and `with_final_snapshot`, but **no** `with_reattach`. `streaming/event_queue/in_memory.rs:1604-1610` therefore ends the stream: `if self.saw_terminal || !self.reattach().await { return None; }`. `final_task_snapshot` defaults to `false` (`handler/limits/mod.rs:304`).

**Failure scenario.** An executor panics, for example on an `unwrap` of a model response. A webhook-only consumer is never told the task failed and waits forever. A streaming client gets `IncompleteStream` and must resubscribe to learn `Failed`. There is also a race: if it resubscribes before `executor_panicked` has saved, it sees `Working` and attaches to a task nobody will ever finish. The resubscribe's reattach hook then polls the store until `Failed` appears or `subscribe_max_idle` expires, which defaults to `None`.

**Suggested fix.** In `executor_panicked` and `on_executor_finished`, emit a synthetic `TaskStatusUpdateEvent(Failed)`: record it in the log at the next position, push it through `deliver_push_bg`, and broadcast it on the queue *before* `CleanupGuard` destroys the queue. A `catch_unwind` around `run_executor` that turns a panic into a written terminal event would be simplest.

**Status:** verified-by-reading for push and the store. The SSE behaviour is verified for the default config (no reattach hook and no final snapshot). The resubscribe race is conjecture.

---

### D-04 · Medium · A config-list failure drops the push silently

**Where**
- `.../background/push_delivery/mod.rs:74-76`: `let Ok(configs) = push_config_store.list(task_id.as_ref()).await else { return; };`
- `.../sync_collector.rs:887-889`: `let Ok(configs) = self.store.list(self.task_id.as_ref()).await else { return; };`

Neither path logs, counts or retries. `Metrics::on_push_delivery` is not called, so `a2a.server.push_deliveries` stays flat.

**Failure scenario.** The push-config database blips (pool timeout, failover). Every event in the window, including terminal states, is dropped with no log line even when tracing is on, and no metric. Operators see "webhook never called" with nothing to correlate. Combined with D-05, there is no recovery.

**Suggested fix.** Retry through `persisting(store_write_retry, ...)` as the task writes do. On a final failure, log with `trace_error!` and count a distinct outcome such as `config_unavailable`.

**Status:** verified-by-reading.

---

### D-05 · Medium · Push is at-most-a-few-attempts, not durable, and has no dedupe id or dead-webhook handling

**Where**
- `push/sender.rs` (`PushRetryPolicy::default`): `max_attempts: 3`, `backoff: [1s, 2s]`. The whole delivery is capped by `push_delivery_timeout` = 30 s. After that the event is dropped, apart from a metric.
- Nothing persists pending deliveries; both delivery paths are in-memory futures on the handler's tracker.
- Request headers: `content-type`, `authorization`, `x-a2a-notification-token`/`a2a-notification-token` and `traceparent` (`push/sender.rs:658-700`). There is **no** per-event or per-delivery id, so a receiver cannot de-duplicate a retry whose first attempt reached it but timed out on the response.
- A `4xx` (for example `404` or `410 Gone`) is `"non-retryable"`, but the config is kept. Every later event of every task using it pays a round trip again, or the full timeout for a blackholed host (D-02).

**Failure scenario.** The receiver is deployed with 60 s of downtime, and every task finishing in that window loses its terminal webhook for good. Or a process restart drops every push in flight. Or a slow receiver acknowledges at 10.5 s: the sender times out at 10 s, retries, and the receiver processes the event twice.

**Suggested fix.**
- Add an optional durable outbox: a table written in the same transaction as the event-log append, with a delivery worker that retries with exponential backoff and jitter over hours.
- Send a stable `A2A-Delivery-Id` header (`taskId:seq`).
- Auto-disable, or back off, configs that answered `404`/`410` or failed N times in a row.

**Status:** verified-by-reading. This is a design limit, not listed in handoff.

---

### D-06 · Medium · Blocking-path push jobs: unbounded spawn and no per-task ordering

**Where.** `.../sync_collector.rs:835-868`. Each blocking request calls `self.in_flight.background().spawn(... SyncPushJob.run() ...)`. There is no semaphore, no per-task queue and no serialization against the streaming background processor of a later turn.

**Failure scenario.**
1. Ordering. A task parks at `input-required` (job 1 is still retrying a slow webhook, up to 30 s per event). The client continues the task, and turn 2's job, or the streaming processor if turn 2 is a streaming send, delivers turn 2's events first. The webhook sees `working` (turn 2) before `input-required` (turn 1), contradicting the code's own comment "events in order so a webhook never sees a later one first".
2. Unbounded work. A webhook down under heavy load gives one live job per blocking request, each holding its `Vec<StreamResponse>` for up to `events × 30 s`. There is no back-pressure and no cap, and shutdown waits on all of them.

**Suggested fix.** Route both paths through the same per-task ordered push worker (see D-02), with a global concurrency limit.

**Status:** verified-by-reading. The ordering race is derived.

---

### D-07 · Medium · The server WebSocket read loop blocks on an untimed write, defeating the idle timeout

**Where.** `a2a-protocol-server/src/dispatch/websocket.rs`:
- `:1319-1325`
  ```rust
  async fn send_json<T: serde::Serialize + Sync>(writer: &WsSink, value: &T) {
      let json = serde_json::to_string(value).unwrap_or_default();
      let mut w = writer.sink.lock().await;
      let _ = w.send(WsMessage::Text(json.into())).await;
  ```
  There is no timeout on either the lock or the send.
- `:715` (`send_json(writer, &busy).await;`, the reply when 64 requests are in flight) and `:747` (the binary-frame reply) are awaited **inside `read_loop`**, outside the `tokio::select!` that watches `closing` and `next_frame` (whose idle logic is at `:770-830`).
- `websocket/stream.rs:55-57`: stream frames take `writer.sink.lock().await` and `send().await`, also untimed.
- `:177`: `idle_timeout: None` by default.

The `next_frame` comments show the authors guarded the *ping* path against a held lock (N30): "Never waits: not for the lock, which a stream send blocked on a peer that stopped reading can hold indefinitely". The busy and binary replies in the read loop were not given the same guard.

**Failure scenario.** A client subscribes to a chatty task and stops reading its socket. The send buffer fills, and a stream task blocks in `w.send()` while holding the sink mutex. The client, or an attacker, then sends one binary frame, or a 65th text request. The read loop then:
- blocks in `send_json` on the mutex for as long as the peer keeps the TCP window at zero, which is indefinitely;
- never runs `next_frame` again, so the idle budget is never evaluated, even when configured;
- never sees `closing.cancelled()`, so shutdown waits out the whole drain timeout for the connection.

With `max_connections` unbounded by default, this is a cheap way to pin sockets and tasks.

**Suggested fix.** Wrap every sink lock and send in `tokio::time::timeout`, and close the connection on expiry. Or move all writes to one writer task fed by a bounded channel and use `try_send` from the read loop: if the channel is full, drop the busy reply and close.

**Status:** verified-by-reading. Not reproduced.

---

### D-08 · Medium · A half-open client WebSocket stays wedged for about 15 minutes

**Where.** `a2a-protocol-client/src/transport/websocket.rs`:
- `:636-660`: connects with `tokio_tungstenite::connect_async_*`. Unlike `connector::http_connector`, no `TcpStream` keepalive is set, and the client never sends a ping. A grep for `ping`/`Ping`/`keepalive` in the file finds no sender.
- `:769-774`: a unary timeout returns `ClientError::Timeout` but does **not** mark the connection closed. `closed` is set only by `fail_all_pending` (`:981-982`), which runs when the reader task exits or a write fails.
- The docs at `:409-414` promise "A connection that drops is replaced on the next call". That holds only when the drop is *observed*.

**Failure scenario.** A NAT or load balancer silently drops the flow (a mobile network, an idle-timeout middlebox, a server host kernel panic). Writes succeed into the local send buffer, and the reader blocks forever. Each call waits 30 s and fails with `Timeout`. The retry layer retries on the *same* dead connection, because reconnect happens only when `closed` is set. The process recovers only when the kernel's retransmission timeout kills the socket (`tcp_retries2` ≈ 15 min). Open streams hang until the 5 min idle timeout.

**Suggested fix.** Send WebSocket pings, for example at `request_timeout / 2`, and treat a missing pong as dead. Set TCP keepalive on the dialled socket by connecting the `TcpStream` yourself and calling `client_async_with_config`. On a unary timeout, mark the connection suspect so the next call reconnects.

**Status:** verified-by-reading.

---

### D-09 · Low · WebSocket client connect errors are classed non-retryable

**Where.** `a2a-protocol-client/src/transport/websocket.rs:651-657`: a connect timeout becomes `ClientError::Transport(format!("WebSocket connect to {endpoint} did not complete within {:?}", ...))`. At `:1025-1040`, `connect_error` maps everything except `Http` and `Io` (TLS failures, a reset during handshake, protocol errors) to `Transport`. `retry.rs`'s `is_retryable` returns `false` for `Transport`. An `Http(resp)` 503 sets `retry_after: None` without reading `resp.headers()`.

**Failure scenario.** A server restarting behind a TLS-terminating load balancer accepts TCP and then resets during TLS, or answers the upgrade late. The HTTP bindings would retry these (`HttpClient`, retryable), but the WebSocket binding fails at once even with a `RetryPolicy`. A `503` with `Retry-After: 20` is retried on generic backoff instead.

**Suggested fix.** Map the connect timeout to `ClientError::Timeout` and TLS or handshake I/O failures to `HttpClient`. Parse `Retry-After` from the handshake response.

**Status:** verified-by-reading.

---

### D-10 · Low · `Retry-After` is clamped to `max_backoff` and not jittered

**Where.** `a2a-protocol-client/src/retry.rs:297-305`:
```rust
last_err.and_then(ClientError::retry_after)
    .map_or_else(|| jittered(backoff), |after| after.min(max_backoff))
```
`max_backoff` defaults to 30 s. `error/mod.rs:301-306` parses only delta-seconds; the HTTP-date form returns `None`.

**Failure scenario.** A rate limiter answers `429 Retry-After: 120`. The client retries at 30 s, 30 s and 30 s, all inside the penalty window, so it uses up `max_retries` and may extend the penalty at a server that resets the window on each early request. When many clients receive the same `Retry-After`, they all retry at exactly the same instant (no jitter).

**Suggested fix.** Honour `Retry-After` up to its own ceiling (the parser already clamps to 1 h), or fail fast when it exceeds the remaining budget. Add small additive jitter, and parse the HTTP-date form.

**Status:** verified-by-reading.

---

### D-11 · Low · No retry by default, and no automatic stream resume

**Where**
- `builder/mod.rs:139`, `:256`: `retry_policy: None`.
- `streaming/event_stream.rs:459-462`: "After an error the stream is finished". `IncompleteStream { last_event_id }` is surfaced and left to the caller (`methods/tasks.rs:177-227` documents a manual resubscribe loop).
- `methods/follow.rs`: in the `How::Subscribed(stream) => stream.next().await` branch of `TaskFollow`, a cut subscription is returned as an error. There is no resubscribe from `last_event_id` and no fallback to polling.
- `RetryTransport` retries only stream *connect*. With the defaults (0.5 s, 1 s, 2 s plus jitter, about 3.5 s in total) a server restart taking more than about 4 s defeats it even for `SubscribeToTask`.

**Failure scenario.** A server rolls during a 10-minute stream. The caller gets `IncompleteStream` and, unless it hand-wrote the resume loop, loses the task. The SDK's own follow helper does the same.

**Suggested fix.** Add an opt-in `EventStream::resuming(client)`, or make `TaskFollow` resubscribe automatically with `Last-Event-ID` and bounded backoff.

**Status:** verified-by-reading. These are design choices, but they are exactly the "server restart mid-stream" case.

---

### D-12 · Low · Client pool hygiene: idle race, no max-lifetime, no per-host cap, no HTTP/2 keepalive

**Where**
- `a2a-protocol-client/src/tls.rs:135-137` and `transport/jsonrpc.rs:134-136`: `Client::builder(TokioExecutor::new()).pool_idle_timeout(Duration::from_secs(90))`. Nothing else is set: no `pool_max_idle_per_host`, no `http2_keep_alive_interval`, no connection lifetime (hyper-util's legacy client has none).
- `a2a-protocol-server/src/serve/graceful/config.rs:44`: `DEFAULT_IDLE_TIMEOUT = 75s`, and the idle timer closes keep-alive connections.

**Failure scenario.**
- **Idle race.** When an idle connection reaches 75 s, the server closes it. A client that picks the same connection from its pool at that instant writes `SendMessage` and gets "connection closed before message completed" (`ClientError::Http`, retryable). `SendMessage` is non-idempotent, so it is not retried, and the user sees a sporadic failure under low traffic. The usual rule is client idle < server idle.
- **DNS.** In a blue/green or Kubernetes Service IP change, busy pooled connections to the old IPs are reused for as long as traffic keeps them warm. The client never re-resolves until the old endpoint actively closes.
- **Fan-out.** A delegating agent has no per-host limit, so a burst opens unbounded sockets to one peer.

**Suggested fix.** Default `pool_idle_timeout` below 60 s (for example 50 s), and expose `pool_max_idle_per_host` and a concurrency limit. Optionally recycle connections after N minutes, and set an HTTP/2 keepalive for h2 peers.

**Status:** verified-by-reading. The race window is inferred from the constants.

---

### D-13 · Low · Push SSRF: `https` is not pinned, and some reserved ranges are missing

**Where**
- `a2a-protocol-server/src/push/pinning.rs:76-84`: `pin_target` returns `None` for `https`, so hyper's `HttpConnector` resolves the name a **second** time after `url_guard::resolve` checked it. The rationale in the doc comment is that TLS validation defeats a rebind.
- `a2a-protocol-server/src/url_guard.rs:66-72` (`is_private_v4`): loopback, RFC 1918, link-local, unspecified and CGNAT. Missing: `0.0.0.0/8` (other than `.0`), `192.0.0.0/24`, `198.18.0.0/15` (benchmark ranges, often routed internally), `240.0.0.0/4`, `255.255.255.255` and multicast `224.0.0.0/4`.
- `:171-187` (`is_private_ip`, IPv6): missing site-local `fec0::/10`, multicast `ff00::/8`, local-use NAT64 `64:ff9b:1::/48` (RFC 8215, not normalized by `embedded_ipv4`) and 6to4 `2002::/16`, which embeds an IPv4 address.

**Failure scenario.** A rebinding resolver answers the public IP to the guard, then `10.0.0.5` to the connector. The sender opens TCP to `10.0.0.5:<port>` and sends a TLS ClientHello carrying an attacker-chosen SNI. The handshake fails, but the attacker learns open versus closed ports from timing and outcome metrics: a blind internal port scan. An internal service with a wildcard certificate covering the attacker's chosen name is unlikely. A deployment using a local NAT64 prefix exposes `64:ff9b:1::a9fe:a9fe` (169.254.169.254).

**Suggested fix.** For `https`, pin too: use a custom resolver (`hyper_util` `HttpConnector::new_with_resolver`) that returns only the validated `SocketAddr`, so SNI and certificate validation still use the hostname. Extend the range lists, ideally with `ip_network`-style "is_global" semantics.

**Status:** verified-by-reading. The impact is limited to blind probing.

---

### D-14 · Medium · The SQLite migration runner leaves an exclusive transaction open on error or cancel

**Where.** `a2a-protocol-server/src/store/migration/mod.rs:258-320`:
```rust
let mut conn = self.pool.acquire().await?;
sqlx::query("BEGIN EXCLUSIVE").execute(&mut *conn).await?;
...
sqlx::query(trimmed).execute(&mut *conn).await?;   // any error: `?` returns, no ROLLBACK
...
sqlx::query("COMMIT").execute(&mut *conn).await?;
```
The doc at `:247-251` claims "If a migration fails, the transaction is rolled back". This uses raw `BEGIN`, not `pool.begin()`, so sqlx does not track a transaction. `sqlx-core-0.9.0/src/pool/connection.rs:275-327` (`return_to_pool`) only `ping()`s and then `release()`s, so the connection returns to the pool **inside** `BEGIN EXCLUSIVE`.

**Failure scenario.** A migration statement fails (for example `SQLITE_FULL`, a constraint, or a busy timeout on a later statement), or the startup future is dropped (a `tokio::time::timeout` around initialization). If the pool outlives the error, every other connection gets `SQLITE_BUSY` after `busy_timeout` (5 s) on each write. Writes on the poisoned connection are never committed, and sqlx's `begin()` on it fails with "cannot start a transaction within a transaction". The pool outlives the error when a caller shares it (`from_pool` and its own `run_pending`), or after a retry loop around startup.

Through `SqliteTaskStore::with_migrations(url)` the pool is dropped on error, which limits the impact to a failed startup. The constructor is at `sqlite_store/mod.rs:125`.

**Suggested fix.** Use `let mut tx = conn.begin_with("BEGIN EXCLUSIVE")`, or keep the raw `BEGIN` but guard it with a drop guard or a `match` that issues `ROLLBACK` on every error path. If the rollback fails, `close_hard` the connection with `conn.detach()`.

**Status:** verified-by-reading. The sqlx release behaviour was read from the vendored crate.

---

### D-15 · Medium · A PostgreSQL failover is classed as permanent, and poisoned connections stay pooled

**Where**
- `a2a-protocol-server/src/sql_error.rs:66-88` (`transient_code`, PostgreSQL): `08*`, `40001`, `40P01`, `55P03`, `53000`, `53200`, `53300`, `57P01`, `57P02`, `57P03`, `57P05`, `57014`. **Not** `25006` (`read_only_sql_transaction`).
- `store/pg_retry.rs:93-100` (`connection_lost`): `Io`, `08*` and `57P01/02/05` only.
- `store/pg_retry.rs:76-87` (`pool_options`): a ping only when a connection was idle 1 s or more. The sqlx default `max_lifetime` is 30 min (`sqlx-core-0.9.0/src/pool/options.rs:162`).

**Failure scenario.** In a Patroni, RDS or Cloud SQL failover with a DNS or VIP swap, connections still open to the old primary, now a read-only standby, answer every `INSERT`/`UPDATE` with `25006`. The ping succeeds, so the connections are never discarded. The error is `A2aError::internal`: not retried by `store_write_retry`, and surfaced to clients as a permanent 500. That lasts until each connection ages out (up to 30 min) or idles past 10 min, which a busy service never does.

**Suggested fix.** Treat `25006` (and `57P04` during maintenance) as `connection_lost`: discard the connection, for example by returning `Ok(false)` from an `after_release` that inspects the last error, or by closing the pool's connections on first sight. Then classify the error transient so the retry reconnects. Consider a shorter default `max_lifetime` (for example 5 min).

**Status:** verified-by-reading. The failover behaviour is standard PostgreSQL semantics, not reproduced here.

---

### D-16 · Low · A store outage during a continuation is reported as `Internal`, not `Unavailable`

**Where.** `a2a-protocol-server/src/handler/messaging/admission.rs:200-218`. When `resume_after` fails, which includes an unavailable store, the code returns `ServerError::Internal(format!("task {task_id}: could not read the event log's last position ..."))`, discarding `e.is_unavailable()`.

**Failure scenario.** During a database blip, every continuation (`input-required` → send) fails with an internal error (500, JSON-RPC `-32603`). The client classes it as non-retryable (`Protocol(e)` without `retry_after`). New tasks in the same blip get `Overloaded`/503 (`idempotency.rs:85` does map unavailable errors), so behaviour is inconsistent.

**Suggested fix.** `if e.is_unavailable() { ServerError::Overloaded(...) }`, with a `Retry-After`.

**Status:** verified-by-reading.

---

### D-17 · Low · A fixed per-store PostgreSQL pool of 10 exhausts `max_connections` at modest scale

**Where.** `store/postgres_store/pool.rs:22`, `:31`, `push/postgres_config_store.rs:58`, `push/tenant_postgres_config_store.rs:63` and `store/tenant_postgres_store/mod.rs:92`: `.max_connections(10)`, with no setter on the URL constructors. The audit, halt and claim stores have their own pools, unless the deployer uses `from_pool`.

**Failure scenario.** Twelve replicas × (task 10 + push 10) = 240 connections, more than PostgreSQL's default `max_connections=100`. New replicas then fail with `53300`. Because `53300` is classed transient, startup or `store_write_retry` spins rather than failing clearly. Each send also makes 8–11 statements (handoff), so a single pool of 10 caps throughput at roughly 10 concurrent turns per replica before the 30 s acquire waits begin.

**Suggested fix.** Expose a pool size on the URL constructors, document one shared pool across stores (`from_pool`), and log the effective pool size at startup.

**Status:** verified-by-reading.

---

### D-18 · Low · In-memory capacity eviction can evict live or parked tasks

**Where.** `store/task_store/in_memory/eviction/capacity.rs:17-28`: `EVICTION_SCAN_WINDOW = 1_024`. "If the oldest `EVICTION_SCAN_WINDOW` tasks are all still running, the sweep evicts the oldest of them". The default `max_capacity` is 10 000 (`task_store/config.rs:264`). The TTL pass removes only terminal tasks.

**Failure scenario.** A human-in-the-loop agent parks over 1024 tasks in `input-required`. These are long-lived and non-terminal, so they are the oldest. Once the store fills, every new write evicts the oldest parked task. The user's reply then gets "task not found", and its executor state is lost.

**Suggested fix.** Never evict non-terminal tasks; refuse new tasks with `Overloaded` instead. At least evict `input-required` last, and emit a metric for each non-terminal eviction.

**Status:** verified-by-reading.

---

### D-19 · Low · Audit per-tenant locks are never evicted

**Where.** `audit/log.rs:51` (`locks: StdMutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>`) and `:147-153` (`chain_lock`: `locks.entry(chain.to_owned()).or_default()`, with no removal anywhere). `audit/mod.rs:125,140`: `r.chain = TenantContext::current();`. The in-memory `audit/store.rs:239-242` `chains` map grows the same way, unlike `keyed_lock` (`handler/locks.rs:45-51`), which sweeps at `max_context_locks`.

**Failure scenario.** If the tenant resolver accepts arbitrary tenant names (path `/{tenant}/...` or a header) and audit is enabled, each distinct name adds a map entry forever, a slow memory leak driven by the client.

**Suggested fix.** Use the same `strong_count > 1` sweep as `keyed_lock`, and bound or validate tenant names before they reach audit.

**Status:** conjecture on reachability. Whether a default resolver admits unbounded tenants was not traced. The unbounded map is verified.

---

### D-20 · Low · gRPC and WebSocket listeners have no liveness bound by default

**Where**
- `dispatch/grpc/dispatcher.rs:47-49`: `keepalive: None`, `max_connections: None`. The docs at `:128-134` record "400 TCP connections opened against this dispatcher and left silent were all accepted and all still alive twelve seconds later".
- `dispatch/websocket.rs:177`: `idle_timeout: None`.
- The `Server` HTTP listener, by contrast, has a header timeout and a 75 s idle timer by default.

**Failure scenario.** Silent or half-open peers accumulate until the file-descriptor limit is reached, after which `accept` fails for everyone. A half-open gRPC streaming subscriber holds its task's broadcast receiver and SSE-equivalent task forever.

**Suggested fix.** Default to HTTP/2 keepalive of (60 s, 20 s) on gRPC and a WebSocket idle timeout of about 120 s, matching the HTTP listener's "safe by default" stance. Leave opting out to the deployer.

**Status:** verified-by-reading. These are documented defaults.

---

### D-21 · Medium · A rolling deploy terminally cancels long tasks

**Where.** `serve/graceful/config.rs:46-71`:
- `DEFAULT_COMPLETION_GRACE = 5s` ("Zero cancels at once").
- `DEFAULT_TASK_GRACE = 10s` (cancellation).
- `DEFAULT_DRAIN_TIMEOUT = 15s`.

`Server::serve_with_shutdown` calls `finish_in_flight`, which cancels executors so "every open stream ends with a terminal event" (`serve/mod.rs` doc on `request_handler`).

**Failure scenario.** In a multi-replica deployment with turn claims (`handler::claims`, built for crash takeover), a routine rolling deploy turns every task running longer than 5 s into terminal `Canceled`, irreversibly. A *crash* would instead let another replica take the claim over after its time to live. So a graceful restart is worse for long LLM tasks than a hard kill. Clients see `Canceled` and may resend, duplicating work.

**Suggested fix.** When a claim store is configured, offer a "hand off" shutdown mode: stop the executor without writing a terminal state, release the claim (or let its time to live lapse), and let another replica resume. At least document that the defaults cancel long tasks on every deploy.

**Status:** verified-by-reading. This is a design issue.

---

### D-22 · Low · Push DNS lookups occupy blocking-pool threads past their timeout

**Where.** `url_guard.rs` (`resolve`): `tokio::net::lookup_host(addr)` under `tokio::time::timeout(dns_timeout, ...)` (5 s). `lookup_host` runs `getaddrinfo` via `spawn_blocking`, and a timeout does not cancel the thread. The `HttpConnector` (GaiResolver) used for `https` and unpinned deliveries does the same.

**Failure scenario.** With a slow or blackholed resolver and many concurrent deliveries (D-06 spawns them without bound), blocking threads pile up, each stuck for the resolver's own timeout (about 5 s × attempts per `resolv.conf`). That starves other `spawn_blocking` users, including file I/O in the process, up to the 512-thread default.

**Suggested fix.** Use an async resolver (`hickory-resolver`) for both validation and connect, which also fixes D-13's pinning, or bound push concurrency globally.

**Status:** conjecture on impact. The mechanism is tokio's documented behaviour.

---

### D-23 · Low · `tokio::spawn` from `Drop` impls

**Where.** `handler/messaging/execute.rs:69-82` (`CleanupGuard::drop` calls `tokio::task::spawn(async move { qmgr.destroy(&tid).await; ... })`) and `:116-122` (`DeferredExecutor::drop` calls `self.tracker.spawn(run)`).

**Failure scenario.** If the future holding these is dropped on a thread with no runtime context, `tokio::spawn` panics inside `Drop`, which aborts the process if it is already unwinding. That can happen when an embedding application drops a `RequestHandler`'s work after `Runtime::shutdown_background`, or drops a boxed future from a non-Tokio thread. During normal runtime shutdown, tokio drops tasks inside its context, so this is an edge case.

**Suggested fix.** Use `tokio::runtime::Handle::try_current()` and fall back to synchronous cleanup, or skip cleanup, when there is no runtime.

**Status:** conjecture on reachability.

---

### D-24 · Info · Panic surface on request paths

A script [the reviewer's local, unpublished one] scanned non-test sources of `a2a-protocol-server/src` for `.unwrap()`, `.expect(`, `panic!`, `unreachable!` and narrowing `as` casts, stopping at `#[cfg(test)] mod`. Of 200 hits, the production code that remains after filtering test files is:

| Site | Reachable from untrusted input? |
|---|---|
| `push/sender.rs:52`, `auth/jwt.rs:989`: `.expect("ring provider supports ...")` | No. A build-time constant configuration. |
| `auth/jwt.rs:1130`: `.expect("len >= 0x80 has a non-zero big-endian byte")` | No. The guard is the line above (DER encoding of the server's own key). |
| `rpc_span/call.rs:55`, `:60`: `.expect("a call polled after it finished")` | No. It fires only if a future is polled after `Ready`, which is a caller bug. |
| `otel/telemetry/console.rs:48`: `buf.lock().expect("lock")` | No. A test or console buffer, and poisoning only. |
| `store/encrypted/cipher.rs:241`: `id.len() as u8` | No. Its comment states that `expand` refuses ids over 255; operator configuration. |
| `store/migration/mod.rs:278`, `pg_migration.rs:282`: `current as u32` from `MAX(version)` | No. A database value written by the SDK. |
| `push_config_page.rs:70`, `in_memory/listing.rs:19`, `helpers.rs:279`, `in_memory/mod.rs:1202`, `:1308`: `u32 ↔ usize` on page size and history length | No truncation on 64-bit; values are clamped first. |
| `dispatch/{rest,jsonrpc}/response.rs`: `max_size as u64` | Safe widening. |
| `store/mod.rs:96`: `&iso[..10]`, `&iso[11..23]` on `unix_millis_to_iso8601` output | Reachable from a client timestamp only through `parse_iso8601_to_unix_millis`. The output is ASCII and at least 24 characters for parseable years. Low risk, not proven for extreme years. |
| String truncation helpers (`rpc_span/agent.rs:402`, `rpc_span/mod.rs:476`, `tool_loop/openai.rs:346`, client `transport/mod.rs:206`) | All walk to `is_char_boundary` first. Safe. |
| `dispatch/rest/query.rs:40-49`, `:147`: slices at `find('/')` and after a matched `%XX` | Safe on ASCII delimiters. |

Executor panics are caught: a spawned executor is observed through `JoinHandle` (`background/mod.rs:188-198`), and an inline one through `catch_unwind` (`execute.rs:151`). The task is set `Failed`, not left `Working`, but see D-03. The executor timeout defaults to 1 h (`builder.rs:241`).

**Status:** verified-by-reading. The grep is heuristic, and macro-generated or indexing `[i]` panics were sampled, not exhaustively proven.

---

### D-25 · Info · Areas checked and found sound

- **Server SSE slow consumer:** the per-stream `mpsc` is bounded (`sse.rs:397`). `tx.closed()` is raced so an abandoned stream ends at once (`:431`). The socket idle timer counts writes as activity (`serve/graceful/idle/mod.rs:19`, `:196`). Broadcast lag goes through the catch-up hook.
- **Blocking send when the client disconnects:** collection runs on its own tracked task (`respond.rs:219-255`, N27), so events are persisted regardless.
- **Request size limits:** JSON-RPC and REST bodies are `Limited` with a read timeout (`jsonrpc/response.rs:217-242`, `rest/response.rs:114-140`). The header-read timeout really applies (a timer is installed, `serve/graceful/connection.rs:115-120`). WebSocket messages and frames are limited to 4 MiB (`websocket.rs:579-581`). gRPC encode and decode are limited to 4 MiB. WebSocket allows 64 requests per connection.
- **Client response and event limits:** `collect_response_limited` (32 MiB plus a deadline); `SseParser` bounds both the line buffer and the event size (`parser.rs:170-201`, `:328`).
- **Client request deadline:** one deadline spans headers and body (`jsonrpc.rs:240-266`).
- **Client stream silence bounds:** first-event and idle bounds of 5 min each, and they abort the reader (`event_stream.rs:566-603`).
- **Retry classification:** non-idempotent methods are retried only on 429 or 503, or a protocol error carrying `retry_after`. `CancelTask` and `SubscribeToTask` count as idempotent. Backoff uses "equal jitter" (0.5–1.0×).
- **PostgreSQL migrations across replicas:** a transaction-scoped advisory lock plus `LOCK TABLE schema_versions IN EXCLUSIVE MODE` with a re-check (`pg_migration.rs:262-283`, `pg_schema.rs:55`).
- **SQLite:** WAL, `synchronous=NORMAL` and `busy_timeout=5000` (`sqlite_pool.rs:53-55`). `SQLITE_BUSY`/`LOCKED` (5/6, extended codes too) are classed transient; `SQLITE_FULL` (13) is permanent.
- **Bounded maps:** context and tenant-slot maps (sweep at `max_context_locks`), cancellation tokens (`max_cancellation_tokens` with live-queue checks), the rate limiter (`max_buckets`), the auth throttle (`max_peers`), idempotency keys (time to live plus a cap; PostgreSQL `EXPIRE_SQL`), and the tenant store (`max_tenants`). `grep unbounded_channel` finds nothing in non-test code.
- **Concurrent streams:** `DEFAULT_MAX_CONCURRENT_STREAMS` caps queues (`builder.rs:1471`) and refuses with `Overloaded`.

---

### KNOWN items (from `docs/handoff.md`, "Open engineering")

- **K-1:** an outage longer than `store_write_retry` loses the event being written, and each later event spends the whole retry again. A queue of N events drains in N × 30 s.
- **K-2:** cross-replica streaming is fed by polling the event log every `subscribe_reattach_interval` (250 ms); `LISTEN`/`NOTIFY` is unbuilt. A cross-replica cancel lands at the owner's next renewal (1 s).
- **K-3:** the executor's `Failed` status can be dropped if its own write gave up, leaving the task unfinished with nothing to finish it. D-03 is related but different: the panic path writes `Failed` without notifying anyone.
