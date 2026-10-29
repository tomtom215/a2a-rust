<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Fixed defects

Part of [Pitfalls & Lessons Learned](../pitfalls.md). Each of these was a defect in the SDK and is fixed; none needs anything from you. They are kept as history, beside the fuller write-ups in [Bugs Found & Fixed](../../deployment/dogfooding-bugs.md), because some record what changed after the first fix.

## Scale & Durability Pitfalls

### SSE parser queue can grow unbounded (fixed)

Write-up: [Bug 37](../../deployment/dogfooding-bugs/passes-7-10.md#bug-37-sse-parser-unbounded-error-queue-oom-risk).

A malicious SSE stream sending many oversized events could fill the parser's internal frame queue without bound. The fix caps the queue at 4096 frames (configurable via `with_max_queued_frames`), dropping the oldest when full.

### Retry backoff can overflow to infinity (fixed)

Write-up: [Bug 39](../../deployment/dogfooding-bugs/passes-7-10.md#bug-39-retry-backoff-float-overflow-can-panic).

`cap_backoff()` computed `Duration::from_secs_f64(current * multiplier)`. With extreme multipliers, this produces `f64::INFINITY` which panics in `from_secs_f64`. The fix checks for non-finite results and clamps to `max_backoff`; `cap_backoff` now uses `Duration::try_from_secs_f64`, which also refuses a finite value too large for a `Duration`, and clamps any value it refuses to the maximum (`crates/a2a-protocol-client/src/retry.rs`).

### Background event processor can miss fast executor events (fixed)

Write-up: [Bug 38](../../deployment/dogfooding-bugs/passes-7-10.md#bug-38-background-event-processor-misses-fast-executor-events-fixed).

In streaming mode, the background event processor previously subscribed to the broadcast channel *after* the executor started. For very fast executors, events could be written before the subscription was active, meaning the task store was not updated. The fix introduces a dedicated persistence channel (mpsc) that is independent of the broadcast channel used for SSE delivery, so a fast executor cannot outrun the subscription; a persistence channel still full after the queue's write timeout fails the write with an error rather than dropping the event.

### Event queue writer silently swallows serialization errors (fixed)

Write-up: [Bug 41](../../deployment/dogfooding-bugs/passes-7-10.md#bug-41-event-queue-serialization-error-silently-swallowed).

`InMemoryQueueWriter::write()` used `unwrap_or(0)` when measuring event size via `CountingWriter`. If serialization failed, the size was reported as 0 and the event was sent through the channel without error. The fix propagates the serialization error via `?` operator, returning `A2aError::internal("event serialization failed: ...")`.

### Capacity eviction cannot evict non-terminal tasks (fixed)

Write-up: [Bug 42](../../deployment/dogfooding-bugs/passes-7-10.md#bug-42-capacity-eviction-fails-when-insufficient-terminal-tasks).

`InMemoryTaskStore` capacity eviction only targeted terminal tasks. If the store was full of non-terminal (Working/Submitted) tasks, eviction found nothing to remove and silently gave up — leaving the store permanently over capacity. The fix adds a fallback path that evicts the oldest non-terminal tasks when there aren't enough terminal tasks.

### Lagged event queue reader drops diagnostic count (fixed)

Write-up: [Bug 43](../../deployment/dogfooding-bugs/passes-7-10.md#bug-43-lagged-event-count-not-exposed-in-warning).

The broadcast channel `Lagged(n)` error provides the exact count of dropped events, but the reader used `_n` (underscore prefix), discarding the count. The warning message said "skipping missed events" without saying how many. The fix exposes the count in the log: `"event queue reader lagged, {n} events dropped"`. The reader no longer skips ahead either: it returns `A2aError::stream_lagged(n)`, and the stream ends with that error. Since the event-log catch-up (spec §3.5.2), a stream reads the dropped events from the task's log first, and ends with that error only when the log cannot supply them all.

## Client Pitfalls

### `truncate_body` panics on multi-byte UTF-8 (fixed)

Write-up: [Bug 44](../../deployment/dogfooding-bugs/passes-12-16.md#bug-44-truncate_body-panics-on-multi-byte-utf-8).

The error body truncation helper sliced at a fixed byte offset (`body[..512]`). For non-ASCII responses (common with international error messages), the offset could fall inside a multi-byte UTF-8 character, causing a panic. The fix uses `is_char_boundary()` to find the nearest safe truncation point.

### SSE parser `line_buf` can grow without bound (fixed)

Write-up: [Bug 45](../../deployment/dogfooding-bugs/passes-12-16.md#bug-45-sse-parser-line_buf-grows-without-bound).

The SSE parser's internal line buffer grew without limit for lines without newlines. A malicious server sending a single very long line could cause OOM. It was first capped at 2× `max_event_size`, past which bytes were dropped silently; a line that outgrows `max_event_size` is now refused as soon as it does, with `SseParseError::EventTooLarge`, and none of the rest of it is buffered.

### REST path parameters are not percent-encoded (fixed)

Write-up: [Bug 47](../../deployment/dogfooding-bugs/passes-12-16.md#bug-47-rest-path-parameters-not-percent-encoded).

Path parameters (task IDs, config IDs) were interpolated into REST URLs without encoding. An ID containing `/` or `..` could cause path traversal. The fix percent-encodes all path parameters using the same encoder as query parameters.

### `GetExtendedAgentCard` discards interceptor params (fixed)

Write-up: [Bug 46](../../deployment/dogfooding-bugs/passes-12-16.md#bug-46-getextendedagentcard-discards-interceptor-params).

The `get_extended_agent_card` method created an empty params object after running interceptors, discarding any modifications made by `before` interceptors. The fix forwards `req.params` instead.
