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
- **Status:** [ ] Not started
- **Location:** `crates/a2a-server/src/dispatch/rest.rs` (`handle_list_push_configs`)
- **Problem:** REST server serializes raw `Vec<TaskPushNotificationConfig>` (JSON array), but the client deserializes into `ListPushConfigsResponse { configs, next_page_token }` (JSON object). Error: `invalid type: map, expected a sequence`.
- **Fix:** Wrap the handler result in `ListPushConfigsResponse` in the REST dispatch, matching the JSON-RPC behavior.

### Bug 5: Push notifications received: 0 in Test 8
- **Severity:** Medium
- **Status:** [ ] Not started
- **Location:** `examples/agent-team/src/main.rs` Test 8
- **Problem:** Push config is registered on Task A, but the second `send_message` creates Task B with a new UUID. `deliver_push` looks up configs by task_id — no config exists for Task B.
- **Fix:** Restructure Test 8 so the push config is exercised on a task that actually generates events *after* registration. Use `return_immediately` to register the config before execution completes, or register on the same task via `context_id`.

### Bug 6: `on_error` metrics hook never fires
- **Severity:** Low
- **Status:** [ ] Not started
- **Location:** `crates/a2a-server/src/handler.rs` — all handler methods
- **Problem:** `on_error` is defined on `Metrics` trait but never called. Error paths use `?` to propagate but never invoke `self.metrics.on_error()`.
- **Fix:** Add `on_error` calls before returning errors in handler methods. Use a helper to avoid boilerplate.

### Bug 7: `on_queue_depth_change` metrics hook never fires
- **Severity:** Low
- **Status:** [ ] Not started
- **Location:** `crates/a2a-server/src/streaming/event_queue.rs`
- **Problem:** `EventQueueManager` has no access to the `Metrics` object. Queue create/destroy never calls `on_queue_depth_change`.
- **Fix:** Pass an `Arc<dyn Metrics>` to `EventQueueManager` during construction and call `on_queue_depth_change` in `get_or_create` and `destroy`.

---

## Gaps

### Gap 1: CancelTask not tested in agent-team
- **Status:** [ ] Not started
- **Problem:** Features list claims `[x] CancelTask executor override` but no test calls `cancel_task()`.
- **Fix:** Add Test 14: send a streaming message to BuildMonitor, cancel mid-stream, verify Canceled state.

### Gap 2: Client lib.rs doc example incorrect
- **Status:** [ ] Not started
- **Location:** `crates/a2a-client/src/lib.rs:94`
- **Problem:** Shows `A2aClient::from_card(&card)?` but real API is `ClientBuilder::from_card(card).build()?`.
- **Fix:** Correct the doc example.

---

## Hardcoded Configs → Configurable

### High Priority: DispatchConfig (centralize duplicated constants)
- **Status:** [ ] Not started
- **Constants to centralize:**
  - `MAX_REQUEST_BODY_SIZE` (4 MiB) — duplicated in rest.rs and jsonrpc.rs
  - `BODY_READ_TIMEOUT` (30s) — duplicated in rest.rs and jsonrpc.rs
  - `MAX_QUERY_STRING_LENGTH` (4096) — rest.rs only
- **Approach:** Create `DispatchConfig` struct, pass to both dispatchers, expose via builder or dispatcher constructors.

### High Priority: Push retry policy
- **Status:** [ ] Not started
- **Constants:** `MAX_PUSH_ATTEMPTS` (3), `PUSH_RETRY_BACKOFF` ([1s, 2s]) in push/sender.rs
- **Approach:** Create `PushRetryPolicy` struct on `HttpPushSender`.

### High Priority: Handler limits
- **Status:** [ ] Not started
- **Constants to expose via RequestHandlerBuilder:**
  - `MAX_ID_LENGTH` (1024) — handler.rs
  - `MAX_METADATA_SIZE` (1 MiB) — handler.rs
  - `MAX_CANCELLATION_TOKENS` (10,000) — handler.rs
  - `MAX_TOKEN_AGE` (1 hour) — handler.rs
  - Push webhook timeout (5s) — handler.rs:730
- **Approach:** Create `HandlerLimits` struct with defaults, add `with_handler_limits()` to builder.

### Medium Priority: Store limits
- **Status:** [ ] Not started
- **Constants:**
  - `EVICTION_INTERVAL` (64) — task_store.rs
  - `MAX_PAGE_SIZE` (1000) — task_store.rs
  - `MAX_PUSH_CONFIGS_PER_TASK` (100) — push/config_store.rs
- **Approach:** Add fields to `TaskStoreConfig` and push config store.

### Medium Priority: SSE / Event queue limits
- **Status:** [ ] Not started
- **Constants:**
  - `DEFAULT_WRITE_TIMEOUT` (5s) — event_queue.rs (not exposed in builder)
  - `DEFAULT_KEEP_ALIVE` (30s) — sse.rs
  - SSE channel capacity (64) hardcoded in sse.rs (should use builder value)
- **Approach:** Add to builder, pass through to SSE response builder.

### Fix: Client/server max event size mismatch
- **Status:** [ ] Not started
- **Problem:** Server DEFAULT_MAX_EVENT_SIZE = 16 MiB, Client DEFAULT_MAX_EVENT_SIZE = 4 MiB. Server can produce events the client rejects.
- **Fix:** Align defaults. Document the mismatch risk in configuration reference.

---

## Documentation Updates Needed

- [ ] Update `book/src/reference/configuration.md` with new configurable options
- [ ] Update `book/src/deployment/dogfooding.md` with Bug 4-7 findings
- [ ] Update `book/src/reference/changelog.md` notes for 0.2.1
- [ ] Update `README.md` agent-team section (add agent-team example)
- [ ] Fix client lib.rs doc example

---

## Commit Log

| Commit | Description |
|--------|-------------|
| — | (tracking will be filled as commits are made) |
