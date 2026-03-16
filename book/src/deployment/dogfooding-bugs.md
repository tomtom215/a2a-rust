# Dogfooding: Bugs Found & Fixed

Four dogfooding passes across `v0.1.0` and `v0.2.0` uncovered **13 real bugs** that 600+ unit tests, integration tests, property tests, and fuzz tests did not catch. All 13 have been fixed.

## Pass 1: Initial Dogfood (3 bugs)

### Bug 1: REST Transport Strips Required Fields from Push Config Body

**Severity:** Medium | **Component:** Client REST transport + Server dispatch

The client's REST transport extracts path parameters from the serialized JSON body to interpolate URL templates. For `CreateTaskPushNotificationConfig`, the route is `/tasks/{taskId}/pushNotificationConfigs`, so the transport extracts `taskId` from the body and *removes it*. But the server handler requires `taskId` in the body.

**Why tests missed it:** Unit tests test client transport and server dispatch in isolation. The bug only appears when they interact.

**Fix:** Server-side `handle_set_push_config` injects `taskId` from the URL path back into the body before parsing.

### Bug 2: `on_response` Metrics Hook Never Called

**Severity:** Low | **Component:** `RequestHandler`

`Metrics::on_response` showed 0 responses after 17 successful requests. The hook was defined but never called in any handler method.

**Fix:** Added `on_request()`/`on_response()` calls to all 10 handler methods.

### Bug 3: Protocol Binding Mismatch Produces Confusing Errors

**Severity:** Low | **Component:** Client JSON-RPC transport

When a JSON-RPC client called a REST-only server, the error was an opaque parsing failure rather than "wrong protocol binding."

**Fix:** (1) New `ClientError::ProtocolBindingMismatch` variant, (2) JSON-RPC transport detects non-JSON-RPC responses, (3) HealthMonitor uses agent card discovery to select correct transport.

## Pass 2: Hardening Audit (6 bugs)

### Bug 4: `list_push_configs` REST Response Format Mismatch

**Severity:** Medium | **Component:** Both dispatchers

Dispatchers serialized results as raw `Vec<TaskPushNotificationConfig>`, but the client expected `ListPushConfigsResponse { configs, next_page_token }`.

**Fix:** Both dispatchers wrap results in `ListPushConfigsResponse`.

### Bug 5: Push Notification Test Task ID Mismatch

**Severity:** Medium | **Component:** Test design

Push config was registered on Task A, but a subsequent `send_message` created Task B. No config existed for Task B.

**Fix:** Restructured as push config CRUD lifecycle test.

### Bug 6: `on_error` Metrics Hook Never Fired

**Severity:** Low | **Component:** `RequestHandler`

All handler error paths used `?` to propagate without invoking `on_error`.

**Fix:** All 10 handler methods restructured with async block + match on `on_response`/`on_error`.

### Bug 7: `on_queue_depth_change` Metrics Hook Never Fired

**Severity:** Low | **Component:** `EventQueueManager`

`EventQueueManager` had no access to the `Metrics` object.

**Fix:** Added `Arc<dyn Metrics>` to `EventQueueManager`, wired from builder.

### Bug 8: `JsonRpcDispatcher` Does Not Serve Agent Cards

**Severity:** Medium | **Component:** `JsonRpcDispatcher`

`resolve_agent_card()` failed for JSON-RPC agents because only `RestDispatcher` served `/.well-known/agent.json`.

**Fix:** Added `StaticAgentCardHandler` to `JsonRpcDispatcher`.

### Bug 9: `SubscribeToTask` Fails with Concurrent SSE Streams

**Severity:** High | **Component:** `EventQueueManager`

`mpsc` channels allow only a single reader. Once `stream_message` took the reader, `subscribe_to_task` failed with "no active event queue."

**Fix:** Redesigned from `mpsc` to `tokio::sync::broadcast` channels. `subscribe()` creates additional readers from the same sender.

## Pass 3: Stress Testing (1 bug)

### Bug 10: Client Ignores `return_immediately` Config

**Severity:** High | **Component:** Client `send_message()`

`ClientBuilder::with_return_immediately(true)` stored the flag in `ClientConfig` but `send_message()` never injected it into `MessageSendParams.configuration`. The server never saw the flag, so tasks always ran to completion.

**Why tests missed it:** The server-side `return_immediately` logic was correct. The bug was in client-to-server config propagation — a seam that only E2E testing exercises.

**Fix:** Added `apply_client_config()` that merges client-level `return_immediately`, `history_length`, and `accepted_output_modes` into params before sending. Per-request values take precedence over client defaults.

## Pass 4: SDK Regression Testing (3 bugs)

### Bug 11: JSON-RPC `ListTaskPushNotificationConfigs` Param Type Mismatch

**Severity:** Critical | **Component:** `JsonRpcDispatcher`

The JSON-RPC dispatcher parsed `ListTaskPushNotificationConfigs` params as `TaskIdParams` (field `id`), but the client sends `ListPushConfigsParams` (field `task_id`). This caused silent deserialization failure — push config listing via JSON-RPC was completely broken. REST worked because it uses path-based routing.

**Why tests missed it:** Previous push config tests used REST transport or tested create/get/delete but not list. The JSON-RPC list path was never exercised end-to-end.

**Fix:** Changed `parse_params::<TaskIdParams>` to `parse_params::<ListPushConfigsParams>` and `p.id` to `p.task_id` in `jsonrpc.rs`.

### Bug 12: Agent Card URLs Set to "http://placeholder"

**Severity:** Critical | **Component:** Agent-team example

Agent cards were constructed with `code_analyzer_card("http://placeholder")` *before* the server bound to a port. The actual address was only known after `TcpListener::bind()`. This meant `/.well-known/agent.json` served a card with a URL that didn't match the actual server address.

**Why tests missed it:** Tests used URLs from `TestContext` (the real bound addresses), not from the agent card. Only `resolve_agent_card()` tests would have caught this, and those didn't exist.

**Fix:** Introduced `bind_listener()` that pre-binds TCP listeners to get addresses *before* handler construction. Cards are now built with correct URLs.

### Bug 13: Push Notification Event Classification Broken

**Severity:** Medium | **Component:** Agent-team webhook receiver

The webhook receiver classified events by checking `value.get("status")` and `value.get("artifact")`, but `StreamResponse` serializes as `{"statusUpdate": {...}}` / `{"artifactUpdate": {...}}` (camelCase variant names). All push events were classified as "Unknown".

**Fix:** Check `statusUpdate`/`artifactUpdate`/`task` instead.

## Configuration Hardening (Pass 2)

Extracted all hardcoded constants into configurable structs:

| Struct | Fields | Where |
|---|---|---|
| `DispatchConfig` | `max_request_body_size`, `body_read_timeout`, `max_query_string_length` | Both dispatchers |
| `PushRetryPolicy` | `max_attempts`, `backoff` | `HttpPushSender` |
| `HandlerLimits` | `max_id_length`, `max_metadata_size`, `max_cancellation_tokens`, `max_token_age` | `RequestHandler` |

Aligned client `DEFAULT_MAX_EVENT_SIZE` from 4 MiB to 16 MiB to match server default.

### Remaining Hardcoded Constants (Deferred)

These use sensible defaults but are not yet user-configurable:

| Constant | Value | Location |
|---|---|---|
| `EVICTION_INTERVAL` | 64 writes | `InMemoryTaskStore` |
| `MAX_PAGE_SIZE` | 1000 | `InMemoryTaskStore` |
| `MAX_PUSH_CONFIGS_PER_TASK` | 100 | `InMemoryPushConfigStore` |
| `DEFAULT_WRITE_TIMEOUT` | 5s | SSE streaming |
| `DEFAULT_KEEP_ALIVE` | 30s | SSE streaming |
