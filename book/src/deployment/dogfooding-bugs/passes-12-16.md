<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Passes 12 to 16

Part of [Bugs Found & Fixed](../dogfooding-bugs.md). Fix descriptions are as first applied.

## Pass 12: Pre-Release Hardening (4 bugs)

### Bug 44: `truncate_body` Panics on Multi-Byte UTF-8

**Severity:** Critical | **Component:** Client transport error formatting

`truncate_body` sliced the response body at a fixed byte offset (`body[..512]`). For non-ASCII responses (common with international error messages, or JSON containing Unicode), the offset could fall inside a multi-byte UTF-8 character, causing a panic: Rust's `&str` indexing requires byte offsets to land on char boundaries.

**Why tests missed it:** All test error bodies used ASCII-only content.

**Fix:** Replaced `&body[..MAX_ERROR_BODY_LEN]` with a loop that finds the nearest char boundary at or before the limit using `is_char_boundary()`. Added a multi-byte UTF-8 regression test.

### Bug 45: SSE Parser `line_buf` Grows Without Bound

**Severity:** High | **Component:** Client SSE parser

The SSE parser's internal `line_buf` appended every non-newline byte without any size check. The `max_event_size` guard only triggered when a complete line was processed (after a newline). A malicious server sending a single very long line without newlines could grow `line_buf` until OOM.

**Why tests missed it:** All test SSE streams used well-formed lines with newlines.

**Fix:** Capped `line_buf` growth at 2× `max_event_size`; bytes beyond the limit were dropped silently. The parser now refuses an over-long line with an `EventTooLarge` error instead, at `max_event_size` plus the longest field prefix, and buffers none of the rest.

### Bug 46: `GetExtendedAgentCard` Discards Interceptor Params

**Severity:** High | **Component:** Client `extended_card` method

The method ran `before` interceptors (which could modify `req.params`), but then sent `serde_json::Value::Object(Map::new())` — an empty object — instead of the potentially-modified `req.params`. Any interceptor modifications to the request params were silently discarded.

**Why tests missed it:** The test `MockTransport` ignores params. No test verified that interceptor-modified params were forwarded.

**Fix:** Changed `serde_json::Value::Object(serde_json::Map::new())` to `req.params`.

### Bug 47: REST Path Parameters Not Percent-Encoded

**Severity:** Medium | **Component:** Client REST transport

Path parameters (task IDs, push config IDs) were interpolated into REST URLs via `String::replace` without any encoding. A task ID containing `/` or `..` (e.g., `"../admin"`) would be interpolated literally, causing path traversal: `/tasks/../admin`. Query values were already properly percent-encoded.

**Why tests missed it:** All test IDs used simple alphanumeric strings.

**Fix:** Applied `encode_query_value()` to path parameters before interpolation, ensuring `/`, `..`, and other special characters are percent-encoded.

---

## Pass 15: Concurrency, Security & Robustness (18 bugs)

### Bug C1: gRPC Transport Mutex Serializes Concurrent Requests

**Severity:** High | **Component:** Client gRPC transport

The gRPC transport wrapped the tonic `Channel` in a `Mutex`, serializing all concurrent requests and destroying throughput. Since tonic `Channel` is internally multiplexed over HTTP/2 and cheap to clone, the Mutex was entirely unnecessary.

**Why tests missed it:** Single-request tests don't expose serialization. The throughput loss only manifests under concurrent load.

**Fix:** Removed the Mutex. The `Channel` is now cloned per request, enabling full concurrent throughput.

### Bug C2: WebSocket Transport Reader Lock Causes Deadlock

**Severity:** High | **Component:** Client WebSocket transport

The WebSocket transport held a `Mutex` on the reader for the entire duration of reading a response. With multiple concurrent requests, each request waited for the reader lock while the reader was blocked waiting for a response that might not be the one needed — a classic deadlock.

**Why tests missed it:** Single-request tests and sequential request tests don't trigger the deadlock. Only concurrent in-flight requests expose it.

**Fix:** Redesigned with a dedicated background reader task that reads all incoming messages and routes them to the correct pending request via `HashMap<RequestId, PendingRequest>`. This eliminates the deadlock and enables true concurrent request/response multiplexing.

### Bug C3: WebSocket Upgrade Request Missing Auth Headers

**Severity:** Medium | **Component:** Client WebSocket transport

Auth interceptor headers were applied to JSON-RPC and REST HTTP requests but not to the WebSocket upgrade HTTP request. This caused authentication failures when connecting to WebSocket endpoints that require authentication on the upgrade request.

**Why tests missed it:** Test WebSocket endpoints don't require authentication on the upgrade request.

**Fix:** Extra headers (including auth interceptor headers) are now applied to the WebSocket upgrade HTTP request via the tungstenite `IntoClientRequest` trait.

### Bug H6: SSRF DNS Rebinding Bypass

**Severity:** High | **Component:** Server push notification validation

`validate_webhook_url()` checked the IP address of the resolved hostname, but DNS resolution happened separately during the actual HTTP request. An attacker could set up a DNS record that resolves to a public IP during validation but a private/loopback IP during the actual request (DNS rebinding).

**Why tests missed it:** Tests use static IPs or localhost. DNS rebinding requires a specially configured DNS server.

**Fix (initial):** Added `validate_webhook_url_with_dns()` that resolves DNS before IP validation and checks every resolved IP against the private/loopback ranges.

**Fix (hardening, 2026-04-15):** The initial fix validated a set of IPs but still allowed the HTTP client to re-resolve the hostname on connect, leaving a narrow TOCTOU window. `validate_webhook_url_with_dns()` now returns the `SocketAddr` it validated (`None` when the host is already a literal IP), and `HttpPushSender::send()` rewrites an `http://` webhook's URI so the request connects directly to the validated IP (`http://<validated-ip>:<port>/…`), with the original hostname in an explicit `Host:` header, so the client never re-resolves the name. An `https://` request is not pinned: it must present the hostname for SNI and certificate verification, and TLS validation is what defeats a rebind to a private address. See `crates/a2a-protocol-server/src/push/pinning.rs` (`rewrite_uri_with_pinned_addr`, `pin_target`) and the `rewrite_uri_*`, `host_header_*` and `pin_target_*` tests in `push/sender.rs`.

### Bug H7: Retry Transport Deep-Clones serde_json::Value Per Attempt

**Severity:** Medium | **Component:** Client retry transport

The retry transport deep-cloned the `serde_json::Value` params tree on every retry attempt. For large request bodies with multiple retries, this caused significant allocations.

**Why tests missed it:** Test payloads are small. The overhead only matters with large request bodies and multiple retries.

**Fix:** Params are now serialized to bytes once before the retry loop, then deserialized from bytes for each attempt. Deserialization from bytes is cheaper than recursive deep-clone.

### Bug H8: Agent Card Fetch Has No Body Size Limit

**Severity:** Medium | **Component:** Client agent card resolver

`CachingCardResolver` fetched agent cards from remote URLs without any body size limit. A malicious or misconfigured endpoint could send an arbitrarily large response, causing OOM.

**Why tests missed it:** Test agent cards are small. No test used an oversized response.

**Fix:** Added a 2 MiB body size limit on agent card fetch responses.

### Bug M3: `find_task_by_context` Silently Swallows Errors

**Severity:** Medium | **Component:** Server request handler

`find_task_by_context` used a pattern that silently converted store errors into "not found" results. A failing store backend would appear as if no task existed for the context, rather than surfacing the actual error.

**Why tests missed it:** In-memory stores don't fail. The bug only manifests with persistent backends under error conditions.

**Fix:** Changed to propagate errors via `?` operator.

### Bug M8: JSON-RPC Batch Size Limit (Confirmed Existing)

**Severity:** Low | **Component:** Server JSON-RPC dispatcher

Confirmed that `max_batch_size` in `DispatchConfig` already limits JSON-RPC batch request sizes, preventing resource exhaustion from oversized batches. No code change needed.

### Bug M9: WebSocket Unbounded Concurrent Task Spawning

**Severity:** Medium | **Component:** Server WebSocket dispatcher

The WebSocket dispatcher spawned a new Tokio task for every incoming message without any concurrency limit. A single client could exhaust server resources by sending many requests in rapid succession.

**Why tests missed it:** Tests send a small number of sequential requests. The resource exhaustion only manifests under adversarial concurrent load.

**Fix:** Added a per-connection `Semaphore(64)` that limits concurrent spawned tasks per WebSocket connection.

### Bug M10: WebSocket No Incoming Message Size Limit

**Severity:** Medium | **Component:** Server WebSocket dispatcher

The WebSocket dispatcher accepted incoming messages of any size. A client could send arbitrarily large WebSocket frames, causing OOM on the server.

**Why tests missed it:** Test messages are small. No test sent oversized frames.

**Fix:** Added a 4 MiB message size check for incoming WebSocket frames. Oversized frames are rejected with an appropriate close code.

### Bug M13: TaskId/ContextId Accept Empty Strings

**Severity:** Medium | **Component:** Protocol types

`TaskId` and `ContextId` accepted empty and whitespace-only strings, which are semantically invalid and could cause downstream issues (empty store keys, confusing error messages).

**Why tests missed it:** All tests use valid non-empty IDs.

**Fix:** Added `TaskId::try_new` and `ContextId::try_new`, which reject empty and whitespace-only strings. `new` and the `From` impls still accept them for backward compatibility.

### Bug M14: CachingCardResolver Silently Produces Empty URLs

**Severity:** Medium | **Component:** Client agent card resolver

`CachingCardResolver::new()` and `with_path()` silently produced empty URLs when given invalid base URLs. This led to confusing errors later when the empty URL was used for HTTP requests.

**Why tests missed it:** Tests always use valid URLs.

**Fix:** `new()` and `with_path()` now return `ClientResult<Self>` instead of silently producing empty URLs.

### Bug M16: REST Dispatch Drops Tenant on Handler Calls

**Severity:** Medium | **Component:** Server REST dispatcher

The REST dispatcher extracted the tenant from the URL path (e.g., `/tenants/{tenant}/tasks/{taskId}`) but did not forward it to all handler method calls. Some methods received `None` for the tenant, causing requests to be processed in the wrong tenant context.

**Why tests missed it:** Tests without multi-tenancy don't exercise tenant propagation. Multi-tenant tests may not have covered all handler methods.

**Fix:** All REST dispatch handler methods now receive the extracted tenant from the URL path.

### Bug M17: FileContent Allows Neither bytes Nor uri

**Severity:** Medium | **Component:** Protocol types

A `FileContent` with neither `bytes` nor `uri` set is semantically invalid but was accepted without error. This could propagate through the system and cause confusing failures downstream.

**Why tests missed it:** All tests construct FileContent with at least one field set.

**Fix:** Added `validate()` method that checks at least one of `bytes`/`uri` is set.

### Bug M19: Push Notification Config URL Not Validated

**Severity:** Medium | **Component:** Protocol types

`TaskPushNotificationConfig` accepted any string as a URL without validation. Malformed URLs would only fail when the push sender attempted to use them.

**Why tests missed it:** Tests use valid URLs.

**Fix:** Added `validate()` method on `TaskPushNotificationConfig` for URL format validation.

### Bug L4: Shutdown Polling Interval Too Coarse

**Severity:** Low | **Component:** Server shutdown

The shutdown polling loop used a 50ms interval, wasting up to 50ms per cycle. This made graceful shutdown slower than necessary.

**Fix:** Reduced polling interval from 50ms to 10ms with deadline-aware sleep (sleeping the minimum of the remaining deadline and the interval).

### Bug L7: SQLite LIMIT Uses String Interpolation

**Severity:** Low | **Component:** Server SQLite store

The SQLite store used `format!` string interpolation for the `LIMIT` clause instead of parameterized queries. While the limit value was internally controlled (not user input), this violated the principle of always using parameterized queries.

**Fix:** Changed `LIMIT` from `format!` interpolation to parameterized queries.

### Bug L14: TaskStatus Timestamp Not Validated

**Severity:** Low | **Component:** Protocol types

`TaskStatus` timestamps were not validated for RFC 3339 conformance. Invalid timestamps could propagate through the system without detection.

**Fix:** Added `has_valid_timestamp()` method to `TaskStatus` for RFC 3339 timestamp validation.

---

## Pass 16: Deep E2E Dogfood (3 bugs)

### Bug P16-1: ListPushConfigs Response Format Mismatch (CRITICAL)

**Severity:** Critical | **Component:** REST + JSON-RPC dispatchers

Both the REST and JSON-RPC dispatchers serialized `ListTaskPushNotificationConfigs` results as a bare `Vec<TaskPushNotificationConfig>`, but the client expected `ListPushConfigsResponse { configs, next_page_token }`. This caused deserialization failure (`"invalid type: map, expected a sequence"`) on every push config list operation via REST and JSON-RPC.

The Axum adapter and gRPC service correctly wrapped results in `ListPushConfigsResponse`, so this bug only affected the two core dispatchers.

**Why tests missed it:** Previous tests either (a) used mock transports that didn't go through dispatchers, (b) tested at the handler level (which returns `Vec`), or (c) asserted the response was "not an error" without parsing the typed response.

**Fix:** Both `RestDispatcher::handle_list_push_configs` and `JsonRpcDispatcher` now wrap results in `ListPushConfigsResponse { configs, next_page_token: None }`, matching the Axum and gRPC implementations.

### Bug P16-2: Push Config URL Validation Ignores allow_private_urls (CRITICAL)

**Severity:** Critical | **Component:** Server handler push config

`RequestHandler::on_set_push_config()` unconditionally called `validate_webhook_url()` which rejects loopback/private URLs. But the `HttpPushSender` has an `allow_private_urls` flag specifically for testing. The handler's validation did not consult this flag, so push configs targeting `127.0.0.1` were always rejected — even when the push sender explicitly allowed private URLs.

**Why tests missed it:** The build monitor's push config tests happened to work because previous tests used REST transport. The JSON-RPC health monitor path hit the validation first. No test explicitly verified that `allow_private_urls` propagated from the push sender to config validation.

**Fix:** Added `allows_private_urls()` method to the `PushSender` trait (default: `false`). `HttpPushSender` implements it returning its `allow_private_urls` field. `on_set_push_config()` now skips URL validation when the push sender allows private URLs.

### Bug P16-3: Background Processor Silently Exits on Store Read Failure

**Severity:** Medium | **Component:** Server background event processor

The background event processor's `task_store.get()` call used `let Ok(Some(mut last_task)) = ... else { return; }`, silently exiting when the store read failed or returned `None`. This left no diagnostic information about why the processor stopped.

**Why tests missed it:** In-memory stores don't fail. The bug only manifests with persistent backends under error conditions.

**Fix:** Replaced with explicit match that logs distinct error messages for "task not found" and "store read error" cases.
