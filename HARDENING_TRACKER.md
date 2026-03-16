# Hardening Tracker

> **Working document** — delete once all items are resolved.
> Tracks bugs, gaps, and improvements found during the second dogfooding pass.

## Status Key

- [ ] Not started
- [~] In progress
- [x] Done

---

## Bugs

### Bug 4: `list_push_configs` REST response format mismatch
- **Severity:** Medium
- **Status:** [x] Done
- **Fix:** Wrapped handler result in `ListPushConfigsResponse` in both REST and JSON-RPC dispatchers.

### Bug 5: Push notifications received: 0 in Test 8
- **Severity:** Medium
- **Status:** [x] Done
- **Fix:** Rewrote Test 8 as push config CRUD lifecycle test (create→get→list→delete→verify).

### Bug 6: `on_error` metrics hook never fires
- **Severity:** Low
- **Status:** [x] Done
- **Fix:** Restructured all handler methods to use async block pattern with `on_error`/`on_response` match.

### Bug 7: `on_queue_depth_change` metrics hook never fires
- **Severity:** Low
- **Status:** [x] Done
- **Fix:** Added `Arc<dyn Metrics>` to `EventQueueManager`, wired in builder.

### Bug 8: `JsonRpcDispatcher` does not serve agent cards
- **Severity:** Medium
- **Status:** [x] Done
- **Fix:** Added `StaticAgentCardHandler` to `JsonRpcDispatcher`, serving `GET /.well-known/agent.json` before falling through to JSON-RPC body parsing.

### Bug 9: `SubscribeToTask` fails when another SSE stream is active
- **Severity:** High
- **Status:** [x] Done
- **Fix:** Redesigned `EventQueueManager` from `mpsc` to `tokio::sync::broadcast` channels. Added `subscribe()` method that creates additional readers for the same task's event stream.

---

## Gaps

### Gap 1: CancelTask not tested in agent-team
- **Status:** [x] Done
- **Fix:** Added Test 14: streaming cancel mid-stream on BuildMonitor.

### Gap 2: Client lib.rs doc example incorrect
- **Status:** [x] Done
- **Fix:** Changed `A2aClient::from_card(&card)?` to `ClientBuilder::from_card(&card).build()?`.

---

## Hardcoded Configs → Configurable

### High Priority: DispatchConfig (centralize duplicated constants)
- **Status:** [x] Done
- **Result:** `DispatchConfig` struct with `max_request_body_size`, `body_read_timeout`, `max_query_string_length`. Both dispatchers accept `with_config()`.

### High Priority: Push retry policy
- **Status:** [x] Done
- **Result:** `PushRetryPolicy` struct with `max_attempts`, `backoff`. Available via `HttpPushSender::with_retry_policy()`.

### High Priority: Handler limits
- **Status:** [x] Done
- **Result:** `HandlerLimits` struct with `max_id_length`, `max_metadata_size`, `max_cancellation_tokens`, `max_token_age`. Available via `RequestHandlerBuilder::with_handler_limits()`.

### Medium Priority: Store limits
- **Status:** [ ] Not started (deferred to next release)
- **Constants:** `EVICTION_INTERVAL` (64), `MAX_PAGE_SIZE` (1000), `MAX_PUSH_CONFIGS_PER_TASK` (100)

### Medium Priority: SSE / Event queue limits
- **Status:** [ ] Not started (deferred to next release)
- **Constants:** `DEFAULT_WRITE_TIMEOUT` (5s), `DEFAULT_KEEP_ALIVE` (30s), SSE channel capacity (64)

### Fix: Client/server max event size mismatch
- **Status:** [x] Done
- **Fix:** Aligned client `DEFAULT_MAX_EVENT_SIZE` from 4 MiB to 16 MiB to match server.

---

## Documentation Updates Needed

- [x] Update `book/src/reference/configuration.md` with new configurable options
- [x] Update `book/src/deployment/dogfooding.md` with Bug 4-7 findings
- [x] Fix client lib.rs doc example
- [x] Update `README.md` agent-team section (if needed)
- [x] Update book pages for broadcast channels, agent card on JSON-RPC, modular agent-team

---

## Commit Log

| Commit | Description |
|--------|-------------|
| `60f59be` | fix: wrap list_push_configs in ListPushConfigsResponse, rewrite Test 8 |
| `d943fde` | fix: wire on_error metrics hook in all handler error paths |
| `77cc1d0` | fix: wire on_queue_depth_change metrics via EventQueueManager |
| `7c0a174` | feat: add CancelTask E2E test (Test 14) to agent-team |
| `7d462af` | refactor: extract hardcoded constants into configurable structs |
| `fed52df` | fix: correct client doc example, align max event size defaults |
| `69bfd1c` | fix: agent card on JSON-RPC, broadcast event queues, modular agent-team |
